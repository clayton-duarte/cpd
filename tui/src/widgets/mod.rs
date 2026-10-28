//! The `widgets` view (commands.md §13.2, §17): the right column of the cockpit. It takes one snapshot of the Inbox
//! ([`read::inbox_at`]) and the merge markers, then reads the markers again on every refresh and the Inbox when its mark
//! ([`read::inbox_mark`]) moved (an in-place label change has no Event), a click forced it, an error is shown, a snooze ended or
//! the engine lock changed: it blocks on the [`Pump`] and reads on the reader thread when the store committed (the watcher) or
//! the safety poll is due, at most one read per frame; an idle view takes no snapshot. The CPU, GPU and memory % are sampled every [`SAMPLE_EVERY`], since they
//! are no Event. It draws when a read result, the CPU, GPU and memory, the error, the size, a click or the wheel changed, and at
//! each frame while a spinner turns. It makes no write of its own, but for what a click or the menu of an Inbox item runs
//! (commands.md §13.2, pro-d130). A right click on an Inbox item opens its menu (on a `pr-ready` row Merge or Post by its status, Go to Lead, Copy Link; Claim on a `review-requested` row only, Snooze 15 min, Snooze 1 h, Snooze until tomorrow 09:00, Dismiss); a
//! menu then takes every key and click until it closes. Otherwise it has no key; mouse capture is on, for the clicks and the
//! wheel over the Inbox section.
//!
//! - [`column`]: the drawing of the machine; [`inbox`]: the Inbox section under it and what a click on an item does;
//! - [`host`]: the machine's CPU %, GPU % and memory %;
//! - [`legend`]: the key legend, `cpd tui view legend`, which the tree's `?` and
//!   the StatusBar's button (`cpd tui action legend`) open in a tmux popup (the columns are narrower than the legend);
//! - the colours and icons are [`crate::palette`]'s.

pub(crate) mod column;
#[cfg(test)]
mod deploy_tests;
#[cfg(test)]
mod draw_tests;
#[cfg(test)]
mod inbox_indent;
#[cfg(test)]
mod inbox_margin_tests;
#[cfg(test)]
mod inbox_width_tests;
#[cfg(test)]
mod needs_tests;
#[cfg(test)]
mod window_tests;
pub(crate) mod host;
pub(crate) mod inbox;
pub mod legend;

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::path::Path;
use cpd_core::host::runner::Runner;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{Event, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::layout::{Margin, Position, Rect};

use super::dialog;
use super::pace::{self, Reads};
use super::popup;
use super::pump::{self, Feed, Schedule, Signal};
use super::viewlog::ViewLog;
use super::menu::{Menu, Outcome};
use super::read;
use super::tree::actions::{self, Failed};
use inbox::{Entry as InboxEntry, Hit as InboxHit, Jump};
use cpd_core::clock;
use cpd_core::inbox::model::{self, Item};
use crate::menu::Entry;
use super::runner::{ViewError, kept};
use super::view_ports::{Ctx, Input, Tty, frame_now, run_view_with, snooze_over, wall_ms};
use time::OffsetDateTime;
use super::{Capture, RunCommand, run_view_on};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::host::{Pane, Tmux, chat_slot};
use cpd_core::ids::SessionId;
use cpd_core::session::PROGRAM;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

pub use column::{Drawn, Machine, draw, draw_error};
pub use host::Usage;
use host::Sampler;
pub use host::ProcReader;
pub(crate) use column::{Window, max_scroll, window};

/// The CPU, GPU and memory sample this often: they are no Event, and the CPU share is over the time since the last sample.
const SAMPLE_EVERY: Duration = Duration::from_secs(2);

pub(super) fn run(ports: &Ports, run_command: RunCommand) -> Result<Output, CpdError> {
    run_view_on(
        ports,
        "widgets",
        Capture::Mouse,
        || prepare_with(ports, CpdHome::from_env()?, host::real_proc()),
        |ctx, prepared| {
            let feed = Feed::real(prepared.home.root());
            show_with(ctx, feed, prepared, run_command)
        },
    )
}

/// [`run`] on a terminal, its facts and its input that the caller gives (a test's `TestBackend`, scripted input and a
/// [`Tty`] that says whether there is a terminal): the same refusals before anything is drawn. The loop is driven by `input`
/// (nothing is read on a thread, nothing waits in real time) and watches no directory: it polls.
pub fn run_on<B: Backend, W: Write>(
    ports: &Ports,
    run_command: RunCommand,
    tty: &mut dyn Tty,
    out: &mut W,
    terminal: Terminal<B>,
    input: &mut dyn Input,
) -> Result<Output, CpdError> {
    run_view_with(
        ports,
        tty,
        out,
        terminal,
        input,
        "widgets",
        Capture::Mouse,
        || prepare(ports, CpdHome::from_env()?),
        |ctx, prepared| show_with(ctx, Feed::driven(), prepared, run_command),
    )
}

/// What the view starts from: the home and its config, the first snapshot of the Inbox and of the machine, and the reader.
pub struct Prepared {
    reader: Arc<Mutex<pace::Owned<Option<read::InboxMark>>>>,
    home: CpdHome,
    config: Config,
    /// The Inbox's open visible items (`engine-down` first).
    inbox: Vec<Item>,
    /// Whether an engine holds the lock, and when the first snooze ends.
    engine_running: bool,
    snooze_end: Option<OffsetDateTime>,
    /// The pull requests whose merge runs, as of the first snapshot of the merge markers.
    merging: BTreeSet<String>,
    machine: Machine,
    sampler: Sampler,
}

/// [`prepare_with`] on a machine whose `/proc` reads nothing (what a test of the view wants: it reads no real machine).
pub fn prepare(ports: &Ports, home: CpdHome) -> Result<Prepared, CpdError> {
    prepare_with(ports, home, host::unreadable())
}

/// The first snapshot of `home`, and the first sample of the machine, whose `/proc` (Linux) is read through `proc`.
pub fn prepare_with(ports: &Ports, home: CpdHome, proc: host::ProcReader) -> Result<Prepared, CpdError> {
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let mut sampler = Sampler::new(proc);
    let machine = Machine { name: hostname(ports), usage: sampler.sample_with(ports) };
    let now = clock::timestamp(ports.clock.utc());
    // the mark and the snooze's end are read before the items: a change between the reads is read again
    let mark = read::inbox_mark(&mut store)?;
    let snooze_end = read::snooze_end_at(&mut store, Some(&now))?;
    let (inbox, engine_running) = read::inbox_at(&mut store, Some(&now))?;
    // the first read of the merges is the first snapshot's: it runs here, before the terminal is set up
    let open = read::open_pulls(&mut store)?;
    let merging = crate::merge::marker::live_with(ports, &home, &open);
    // the reads after them run on the reader thread, with a store of its own
    let reader = pace::Owned::new(home.clone(), u64::from(config.timeouts.store_busy_ms), Some(mark));
    Ok(Prepared { reader, home, config, inbox, engine_running, snooze_end, merging, machine, sampler })
}

/// [`show`] on what [`prepare`] made: the tmux of the config's socket, `run_command` for what a click runs, and `feed`'s pump
/// for the one wait. A test's `feed` is scripted (signals, the `ManualClock` of `ctx.ports`, reads inline, no watcher) and
/// `ctx.input` is not read; the end of a script is `Signal::InputFailed`.
pub fn show_with<B: Backend>(ctx: &mut Ctx<B>, feed: Feed, prepared: Prepared, run_command: RunCommand) -> Result<(), ViewError> {
    let Prepared { reader, home, config, inbox, engine_running, snooze_end, merging, machine, sampler } = prepared;
    let tmux = Tmux::new(config.tmux.socket.clone()).with_runner(ctx.ports.runner.clone());
    let log = ViewLog::of(&home, "widgets");
    let runner = actions::Runner::new(home.clone(), &config, ctx.ports);
    let lead = Opener { home: &home, busy_ms: u64::from(config.timeouts.store_busy_ms), run_command, tmux: &tmux, log: &log, runner: &runner };
    show(ctx, feed, &reader, (inbox, engine_running, snooze_end, merging), machine, sampler, &lead, config.events.poll_ms)
}

/// The area the Deploy column's right-click menu is clamped in and hit-tested against: the pane less a blank column on
/// each side.
pub(crate) fn menu_pane(pane: Rect) -> Rect {
    pane.inner(Margin::new(1, 0))
}

/// What a click on an Inbox item runs. `log` is `run/tui/widgets.log`.
struct Opener<'a> {
    home: &'a CpdHome,
    /// `timeouts.store_busy_ms`, for the read a pick makes.
    busy_ms: u64,
    run_command: RunCommand,
    tmux: &'a Tmux,
    log: &'a ViewLog,
    /// What runs the actions of an Inbox item's jump (the tree's, shared).
    runner: &'a actions::Runner,
}

/// The right-click menu of an Inbox item, while it is open: its entries and the item it is about.
struct InboxMenu {
    menu: Menu<InboxEntry>,
    /// The item itself: what Merge, Post, Go to Lead and Copy Link act on.
    item: Item,
}

/// What an open menu does with `event`: it takes every key and click.
fn drive<E: Entry>(menu: &mut Menu<E>, pane: Rect, event: &Event) -> Outcome<E> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => menu.key(*key),
        Event::Mouse(m) => menu.mouse(menu_pane(pane), *m),
        _ => Outcome::Stay,
    }
}

impl Opener<'_> {
    /// A left click on `item`: its jump ([`inbox::click`]), run on the tree's runner; a failure is the error dialog and a
    /// line of the log. The Sessions are read as of `now` (contract §1.3 text).
    fn jump(&self, item: &Item, now: &str) {
        let tree = match self.runner.tree(now) {
            Ok(tree) => tree,
            Err(why) => return dialog::report_why(self.tmux, self.log, "Inbox", why),
        };
        match inbox::click(&tree, item) {
            None => {}
            Some(Jump::Run(action)) => self.said(self.runner.run(&action)),
            Some(Jump::Steps(steps)) => self.said(self.runner.run_all(&steps)),
            Some(Jump::Models(provider, at)) => {
                let shown = inbox::models_body(self.home, &provider, &at).map_err(|e| format!("{}: {}", e.code(), e.message()));
                match shown {
                    Ok(body) => {
                        // a dialog that cannot open is logged: another dialog would fail the same way
                        if let Err(e) = dialog::text(self.tmux, &format!("Models: {provider}"), &body) {
                            let _ = self.log.append(&format!("failed: Models: dialog: {}", e.message()));
                        }
                    }
                    Err(why) => dialog::report_why(self.tmux, self.log, "Models", why),
                }
            }
            // the checklist popup returns when it closes; a refusal is said in a `text` dialog by the runner
            Some(Jump::Curate(kind, context)) => {
                if let Err(why) = self.runner.curate(kind, &context) {
                    dialog::report_why(self.tmux, self.log, "Curate", why);
                }
            }
        }
    }

    /// The result of an action: a failure is the error dialog and a line of the log (a success says nothing).
    fn said(&self, result: Result<String, Failed>) {
        if let Err(Failed { step, why }) = result {
            dialog::report_why(self.tmux, self.log, &step.argv().join(" "), why);
        }
    }

    /// `Post`: `pr post --pr <key> --plan <plan>`, `<key>` read now from the `prs` row of the item's Plan that points at the item's
    /// pull request; no popup, the cockpit waits. A refusal, or no such row, is the failure dialog.
    fn post(&self, item: &Item) {
        const TITLE: &str = "Post Pull Request";
        let (Some(plan), Some(pull)) = (item.plan.as_ref(), inbox::Pull::from_subject(&item.subject)) else { return };
        let key = Store::open(self.home, self.busy_ms).and_then(|mut store| {
            store.read(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT r.key FROM prs r JOIN pull_requests q ON q.id = r.pull_request \
                     WHERE r.plan = ?1 AND q.repo = ?2 AND q.number = ?3 ORDER BY r.key LIMIT 1",
                )?;
                let keys = stmt.query_map(rusqlite::params![plan.as_str(), pull.repo, pull.number], |r| r.get::<_, String>(0))?;
                Ok(keys.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().next())
            })
        });
        match key {
            Err(e) => dialog::report(self.tmux, self.log, TITLE, &e),
            Ok(None) => dialog::report_why(self.tmux, self.log, TITLE, format!("No PR of Plan {plan} points at {pull}.")),
            Ok(Some(key)) => {
                let argv = ["post", "--pr", &key, "--plan", plan.as_str()];
                let all = std::iter::once(PROGRAM).chain(["pr"]).chain(argv).map(std::ffi::OsString::from).collect();
                if let Err(e) = (self.run_command)(all) {
                    dialog::report(self.tmux, self.log, TITLE, &e);
                }
            }
        }
    }

    /// `Copy Link`: the item's url into tmux's buffer and the client's clipboard.
    fn copy_link(&self, item: &Item) {
        let Some(url) = item.url.as_ref() else { return };
        let args = ["set-buffer", "-w", "--", url.as_str()].map(str::to_owned);
        if let Err(e) = self.tmux.run(&args) {
            dialog::report(self.tmux, self.log, "Copy Link", &e);
        }
    }

    /// The entry `choice` of the menu of the item `id`: `inbox snooze` or `inbox dismiss`, through `run_command`.
    fn choose(&self, choice: inbox::Choice, id: i64) {
        let argv = choice.argv(id);
        let words = argv.join(" ");
        let all = std::iter::once(PROGRAM.to_owned()).chain(argv).map(std::ffi::OsString::from).collect();
        if let Err(e) = (self.run_command)(all) {
            dialog::report(self.tmux, self.log, &words, &e);
        }
    }
}

/// Draws and follows, for as long as the process runs: no key ends it. A failed read keeps what is on screen and shows its error in the last row, until a read
/// succeeds ([`kept`]). The wheel over the Inbox section scrolls it. A left click on a link opens its url, on an Inbox item its jump; a failure is the error dialog and a line of the log (commands.md §17 "No notes"). A right click on an
/// Inbox item opens its menu, which takes every key and click until it closes.
fn show<B: Backend>(
    ctx: &mut Ctx<B>,
    feed: Feed,
    reader: &Arc<Mutex<pace::Owned<Option<read::InboxMark>>>>,
    (mut inbox, mut engine_running, mut snooze_end, mut merging): Seen,
    mut machine: Machine,
    mut sampler: Sampler,
    opener: &Opener,
    poll_ms: u32,
) -> Result<(), ViewError> {
    let ports = ctx.ports;
    let Feed { pump, watch } = feed;
    let mut reads = Reads::<Read>::on(&pump);
    let poll = Duration::from_millis(u64::from(poll_ms));
    let (every, watch_failure) = match &watch {
        Ok(Some(_)) => (pump::SAFETY, None),
        // a driven loop (run_on) watches nothing, says nothing of it and polls
        Ok(None) if pump.is_driven() => (poll, None),
        Ok(None) => (pump::SAFETY, None),
        Err(e) => (poll, Some(format!("no commit watcher: {}; polling every {poll_ms} ms", e.message()))),
    };
    let mut schedule = Schedule::new(ports.clock.now(), every);
    let mut sampled_at = ports.clock.now();
    let mut last = None::<Drawn>;
    let mut error = None::<String>;
    let mut redraw = true;
    let mut drawn_frame = None::<usize>;
    let mut inbox_menu = None::<InboxMenu>;
    let mut pane = Rect::default();
    // The Inbox section: whether it is unfolded and its offset, in items (memory only, kept by a refresh and clamped by the
    // drawing), and what the last draw showed of the clock's second (the items' ages).
    let (mut inbox_expanded, mut inbox_scroll) = (false, 0usize);
    let mut drawn_second = wall_ms(ports) / 1000;
    loop {
        // the spinner of a working Lead turns: a new frame redraws (commands.md §13.2)
        let (frame, second) = (frame_now(ports), wall_ms(ports) / 1000);
        let wants = needs(&inbox, &merging, sampled_at);
        // the Inbox items' ages are redrawn every second, from `opened_at`
        if redraw || (wants.frame && Some(frame) != drawn_frame) || (wants.second && second != drawn_second) {
            drawn_second = second;
            ctx.draw(|f| {
                pane = f.area();
                // an open Inbox menu is drawn over the labels: a link piece (2 cells, forced width) under its border would swallow
                // the border's cell, so while it is open the labels are drawn without their OSC 8 links
                let unlinked: Vec<model::Item> = inbox_menu.iter().flat_map(|_| inbox.iter().cloned().map(|i| model::Item { url: None, ..i })).collect();
                let shown = if inbox_menu.is_some() { &unlinked } else { &inbox };
                last = Some(draw(f, pane, &machine, frame, shown, (inbox_expanded, inbox_scroll), ports.clock.utc(), &merging));
                if let Some(error) = error.as_ref().or(watch_failure.as_ref()) {
                    draw_error(f, pane, error);
                }
                if let Some(open) = &inbox_menu {
                    open.menu.draw_at(f, menu_pane(pane), frame);
                }
            })?;
            (redraw, drawn_frame) = (false, Some(frame));
        }
        let drawn = last.as_ref().expect("the terminal drew");
        inbox_scroll = inbox_scroll.min(drawn.inbox_shape.max_scroll());
        let now = ports.clock.now();
        let wall_ms = wall_ms(ports);
        let until = pump::wake(now, wall_ms, &needs(&inbox, &merging, sampled_at), schedule.deadline(now));
        let signal = if pump.is_driven() {
            pump.wait_driven(ports, &mut *ctx.input, until, schedule.is_out())
        } else {
            pump.wait(until)
        };
        match signal {
            Signal::Input(event) => {
            redraw = true; // a click, the wheel, a key or a resize
            if matches!(event, Event::Resize(..)) {
                // a new pane starts the read pacing afresh: the next commit reads at once, not a frame after the last read
                schedule = Schedule::new(ports.clock.now(), every);
            }
            if let Some(open) = inbox_menu.as_mut() {
                // an open menu takes every key and click
                match drive(&mut open.menu, pane, &event) {
                    Outcome::Stay => {}
                    Outcome::Close => inbox_menu = None,
                    Outcome::Pick(entry) => {
                        let InboxMenu { item, .. } = inbox_menu.take().expect("the menu was open");
                        match entry {
                            InboxEntry::Choice(choice) => opener.choose(choice, item.id),
                            InboxEntry::Merge => {
                                if let Some(pull) = inbox::Pull::from_subject(&item.subject) {
                                    if let Err(e) = crate::merge::start(opener.tmux, &pull.to_string()) {
                                        dialog::report(opener.tmux, opener.log, "Merge Pull Request", &e);
                                    }
                                }
                            }
                            InboxEntry::Post => opener.post(&item),
                            InboxEntry::GoToLead => opener.jump(&item, &clock::timestamp(ports.clock.utc())),
                            InboxEntry::CopyLink => opener.copy_link(&item),
                            InboxEntry::Merging => {}
                        }
                        schedule.force(); // the Inbox shows the result on the next read
                    }
                }
            } else {
                match event {
                    // a right click on an item row of the Inbox opens its menu; the title and the toggle row have none, and
                    // `engine-down` is no item to snooze
                    Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Right) && drawn.inbox.contains(Position::new(m.column, m.row)) => {
                        if let Some(InboxHit::Item(i)) = drawn.inbox_shape.hit(m.row - drawn.inbox.y) {
                            let item = &inbox[i];
                            if item.kind != model::Kind::EngineDown {
                                let at = Position::new(m.column, m.row);
                                let merging = inbox::merges(&merging, item);
                                inbox_menu = Some(InboxMenu { menu: Menu::new(InboxEntry::entries(item, merging), at), item: item.clone() });
                            }
                        }
                    }
                    // a left click on the Inbox's toggle row folds or unfolds it, on an item row jumps to its cause
                    Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) && drawn.inbox.contains(Position::new(m.column, m.row)) && drawn.links.at(Position::new(m.column, m.row)).is_none() => {
                        match drawn.inbox_shape.hit(m.row - drawn.inbox.y) {
                            Some(InboxHit::Toggle) => {
                                inbox_expanded = !inbox_expanded;
                                inbox_scroll = 0;
                            }
                            Some(InboxHit::Item(i)) => {
                                opener.jump(&inbox[i], &clock::timestamp(ports.clock.utc()));
                                schedule.force(); // the result shows on the next read
                            }
                            Some(InboxHit::Header) | None => {}
                        }
                    }
                    Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                        let at = Position::new(m.column, m.row);
                        if let Some(url) = drawn.links.at(at) {
                            if let Err(e) = crate::link::open_url(&*ports.runner, url) {
                                dialog::report(opener.tmux, opener.log, "Open Link", &e);
                            }
                        }
                        schedule.force();
                    }
                    Event::Mouse(m) if drawn.inbox.contains(Position::new(m.column, m.row)) => match m.kind {
                        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                            inbox_scroll = drawn.inbox_shape.wheeled(m.kind == MouseEventKind::ScrollDown);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            }
            Signal::Commit => schedule.commit(),
            Signal::Read => {
                while let Some(result) = reads.take() {
                    schedule.done();
                    let shown = error.clone();
                    if let Some((fresh, live)) = kept(result, &mut error) {
                        if live != merging {
                            merging = live;
                            redraw = true;
                        }
                        // a cycle that read no Inbox took no snapshot and keeps what is shown
                        if let Some((items, running, end)) = fresh {
                            ctx.snapshot_read();
                            (engine_running, snooze_end) = (running, end);
                            if items != inbox {
                                inbox = items;
                                redraw = true;
                            }
                        }
                    }
                    redraw |= error != shown;
                }
            }
            Signal::InputFailed(e) => return Err(e.into()),
            Signal::Due => {}
        }
        let now = ports.clock.now();
        // The CPU share is over the time since the last sample, so it is taken on its own timer, not at each Event.
        if now >= sampled_at + SAMPLE_EVERY {
            let usage = sampler.sample_with(ports);
            sampled_at = now;
            if usage != machine.usage {
                machine.usage = usage;
                redraw = true;
            }
        }
        if let Some(kind) = schedule.take(now) {
            // Every read takes the merge markers; the Inbox (items, engine lock, snooze end) only when its mark moved (a label
            // changes in place with no Event), a click forced it, an error is shown, a snooze ended or the engine lock changed
            // (an engine stops with no Event either).
            let (shared, home, retry, ended, now_text) =
                (Arc::clone(reader), opener.home.clone(), error.is_some(), snooze_over(ports, snooze_end), clock::timestamp(ports.clock.utc()));
            let force = kind.force;
            let ports = ports.clone();
            reads.request(move || {
                pace::Owned::read(&shared, |store, last| {
                    let open = read::open_pulls(store)?;
                    let live = crate::merge::marker::live_with(&ports, &home, &open);
                    let mark = read::inbox_mark(store)?;
                    let engine = model::engine_running(&home)?;
                    let fresh = if last.as_ref() != Some(&mark) || force || retry || ended || engine != engine_running {
                        // the mark was read before the items: a change between the reads is read again
                        let end = read::snooze_end_at(store, Some(&now_text))?;
                        let (items, running) = read::inbox_at(store, Some(&now_text))?;
                        *last = Some(mark);
                        Some((items, running, end))
                    } else {
                        None
                    };
                    Ok((fresh, live))
                })
            });
        }
    }
}

/// What the view shows besides the machine: the Inbox's items, whether an engine holds the lock, when the first snooze ends and the pull requests whose merge runs.
type Seen = (Vec<Item>, bool, Option<OffsetDateTime>, BTreeSet<String>);

/// What a read of the reader thread returns: the Inbox (its items, the lock and the snooze's end) when it was read, and the
/// pull requests with a live merge marker.
type Read = Result<(Option<(Vec<Item>, bool, Option<OffsetDateTime>)>, BTreeSet<String>), CpdError>;

/// The merge of a `pr-ready` item runs: its spinner turns.
fn spins(inbox: &[Item], merging: &BTreeSet<String>) -> bool {
    inbox.iter().any(|item| inbox::merges(merging, item))
}

/// The clocks the column's screen needs: `frame` is `spins(inbox, merging)`; `second` is `!inbox.is_empty()` (an
/// age text is on screen); `at` is always `Some(sampled_at + SAMPLE_EVERY)`.
fn needs(inbox: &[Item], merging: &BTreeSet<String>, sampled_at: Instant) -> pump::Needs {
    pump::Needs { frame: spins(inbox, merging), second: !inbox.is_empty(), at: Some(sampled_at + SAMPLE_EVERY) }
}

/// A click on a tab: `session open <id>`, which moves that Lead into the chat slot, then `select-pane`
/// on the chat. The plan view's tabs use it.
pub(crate) fn open_lead(run_command: RunCommand, tmux: &Tmux, cockpit: &str, session: &SessionId) -> Result<(), CpdError> {
    let argv = [PROGRAM, "session", "open", session.as_str()].map(std::ffi::OsString::from).into();
    run_command(argv)?;
    if let Some(chat) = chat_slot(&main_panes(tmux, cockpit)?, cockpit) {
        tmux.run(&["select-pane", "-t", chat.pane.as_str()].map(str::to_owned))?;
    }
    Ok(())
}

/// The panes of the cockpit's `main` window.
pub(crate) fn main_panes(tmux: &Tmux, cockpit: &str) -> Result<Vec<Pane>, CpdError> {
    let all = tmux.list_panes()?;
    Ok(all.into_iter().filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW).collect())
}

/// What the column draws for a hostname it could not read.
const UNKNOWN: &str = "?";

/// The machine's name: the kernel's (`/proc/sys/kernel/hostname`), else `$HOSTNAME`, else what the `hostname` program
/// prints (macOS has no `/proc`), else `?`: the name is one decorative line, so no failure ends the view.
fn hostname(ports: &Ports) -> String {
    hostname_with(&*ports.runner, Path::new("/proc/sys/kernel/hostname"), std::env::var("HOSTNAME").ok().as_deref())
}

/// [`hostname`] with its three sources given: the kernel's file at `file`, `$HOSTNAME` as `env`, and the `hostname` program on `runner`.
pub(crate) fn hostname_with(runner: &dyn Runner, file: &Path, env: Option<&str>) -> String {
    read_hostname(file)
        .or_else(|| env.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()))
        .or_else(|| host::run_with(runner, "hostname", &[]).map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()))
        .unwrap_or_else(|| UNKNOWN.to_owned())
}

/// The first line of the file at `path`, when it has one.
fn read_hostname(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().next()?.trim().to_owned()).filter(|n| !n.is_empty())
}

/// The legend in a tmux popup over the cockpit, sized to it and at most the client's size.
pub(crate) fn open_legend(tmux: &Tmux) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = popup::newest_client(&clients).map(|(_, size)| size).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&legend::popup_args(client)).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpd_core::clock::ManualClock;
    use cpd_core::host::runner::{FakeRunner, Reply};

    #[test]
    fn the_legend_popup_is_sized_by_the_most_recently_active_client() {
        let size = |listing: &str| popup::newest_client(listing).map(|(_, size)| size);
        assert_eq!(size("100 80 24\n300 215 62\n200 120 30\n"), Some((215, 62)));
        assert_eq!(size(""), None);
        assert_eq!(size("junk\n"), None);
    }

    /// tui-W10: a name that cannot be read is no error of the view (nor are the CPU, GPU and memory, `host::Sampler`).
    #[test]
    fn a_missing_hostname_file_or_load_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hostname");
        assert_eq!(read_hostname(&file), None, "no file");
        std::fs::write(&file, "\n").unwrap();
        assert_eq!(read_hostname(&file), None, "an empty one");
        std::fs::write(&file, "homeserver\n").unwrap();
        assert_eq!(read_hostname(&file).as_deref(), Some("homeserver"));
        // the `hostname` program is the runner's: a fake, never the machine's
        let fake = FakeRunner::new(ManualClock::new()).on("hostname", |_| true, Reply::ok("mac-mini.local\n"));
        assert_eq!(hostname_with(&fake, &file, None), "homeserver", "the kernel's file comes first");
        assert_eq!(hostname_with(&fake, &dir.path().join("none"), Some("")), "mac-mini.local");
        assert!(!hostname_with(&fake, &file, None).is_empty());
    }
}
