//! The `widgets` view (commands.md §13.2, §17): the Dispatch column, the
//! machine-wide right column of the cockpit. It takes one snapshot of the read
//! model ([`read::open_prs`]: the one thing it draws), then follows the Events: it blocks on the [`Pump`] and reads on
//! the reader thread when the store committed (the watcher) or the safety poll is due, at most one read per frame; the
//! CPU, GPU and memory % are sampled every [`SAMPLE_EVERY`], since they are no Event. It draws when a read result, the
//! CPU, GPU and memory, the error, the size, a click or the wheel changed, and at each frame while a spinner turns. It makes no write of
//! its own, but for what a click on an owned PR row asks of `cpd session open`, and the merge popup a click on a merge
//! icon opens. A right click on a PR row opens its menu ([`crate::menu`]: Merge, Copy Link, Go to Lead), which then
//! takes every key and click until it closes. Otherwise it has no key; mouse capture is on, for the clicks and the
//! wheel over the PRs section.
//!
//! - [`column`]: the drawing of the machine and the PRs;
//! - [`host`]: the machine's CPU %, GPU % and memory %;
//! - [`legend`]: the key legend, `cpd tui view legend`, which the tree's `?` and
//!   the StatusBar's button (`cpd tui action legend`) open in a tmux popup (the columns are narrower than the legend);
//! - the colours and icons are [`crate::palette`]'s.

pub(crate) mod column;
#[cfg(test)]
mod draw_tests;
#[cfg(test)]
mod column_link_tests;
#[cfg(test)]
mod kinds_tests;
#[cfg(test)]
mod needs_tests;
pub(crate) mod host;
pub mod legend;

use std::collections::{BTreeMap, BTreeSet};
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
use super::read::{self, OpenPr};
use super::runner::{ViewError, kept};
use super::view_ports::{Ctx, Input, Tty, frame_now, run_view_with, wall_ms};
use super::{Capture, RunCommand, run_view_on};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::Kind;
use cpd_core::home::CpdHome;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, View, chat_slot, view_pane};
use cpd_core::ids::{AttemptId, PlanId, SessionId};
use cpd_core::session::PROGRAM;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

pub use column::{Drawn, Hit, Machine, MergeIcon, PrItem, PrRow, draw, draw_error};
pub use host::Usage;
use host::Sampler;
pub(crate) use column::{Window, max_scroll, window};

/// The CPU, GPU and memory sample this often: they are no Event, and the CPU share is over the time since the last sample.
const SAMPLE_EVERY: Duration = Duration::from_secs(2);

pub(super) fn run(ports: &Ports, run_command: RunCommand) -> Result<Output, CpdError> {
    run_view_on(
        ports,
        "widgets",
        Capture::Mouse,
        || prepare(ports, CpdHome::from_env()?),
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

/// What the view starts from: the home and its config, the first snapshot of the PRs and of the machine, and the follow's reader.
pub struct Prepared {
    reader: Arc<Mutex<pace::Owned<read::Follow>>>,
    home: CpdHome,
    config: Config,
    prs: Vec<OpenPr>,
    machine: Machine,
    sampler: Sampler,
}

/// The first snapshot of `home`, and the first sample of the machine.
pub fn prepare(ports: &Ports, home: CpdHome) -> Result<Prepared, CpdError> {
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let mut sampler = Sampler::default();
    let machine = Machine { name: hostname(ports), usage: sampler.sample_with(ports) };
    let (mut prs, cursor) = read::open_prs(&mut store)?;
    // the first read of the merges is the first snapshot's: it runs here, before the terminal is set up
    let open = read::open_pulls(&mut store)?;
    mark_merging(&mut prs, &crate::merge::marker::live_with(ports, &home, &open));
    let follow = read::Follow::new(&mut store, KINDS, cursor)?;
    // the reads after them run on the reader thread, with a store of its own
    let reader = pace::Owned::new(home.clone(), u64::from(config.timeouts.store_busy_ms), follow);
    Ok(Prepared { reader, home, config, prs, machine, sampler })
}

/// [`show`] on what [`prepare`] made: the tmux of the config's socket, `run_command` for what a click runs, and `feed`'s pump
/// for the one wait. A test's `feed` is scripted (signals, the `ManualClock` of `ctx.ports`, reads inline, no watcher) and
/// `ctx.input` is not read; the end of a script is `Signal::InputFailed`.
pub fn show_with<B: Backend>(ctx: &mut Ctx<B>, feed: Feed, prepared: Prepared, run_command: RunCommand) -> Result<(), ViewError> {
    let Prepared { reader, home, config, prs, machine, sampler } = prepared;
    let tmux = Tmux::new(config.tmux.socket.clone()).with_runner(ctx.ports.runner.clone());
    let log = ViewLog::of(&home, "widgets");
    let lead = Opener { home: &home, run_command, tmux: &tmux, cockpit: &config.tmux.cockpit_session, log: &log };
    show(ctx, feed, &reader, prs, machine, sampler, &lead, config.events.poll_ms)
}

/// The Events that change what the column draws besides the pull request rows (whose changes have no Event): a pull
/// request gets its PR (`pr-opened`), a Plan its Lead (`plan-created`, `session-registered`), the chat slot moves, a Lead's dot is a fact the column draws (`session-state`).
/// The update Job's status moves its PR's `conflict` to `updating` and back (hs-d532): a Job is added, started (its
/// `attempt-started`; there is no `job-started` Event), retried, merged, failed or cancelled.
const KINDS: &[Kind] = &[
    Kind::PrOpened,
    Kind::PlanCreated,
    Kind::SessionRegistered,
    Kind::SessionMoved,
    Kind::SessionState,
    Kind::JobAdded,
    Kind::AttemptStarted,
    Kind::JobRetried,
    Kind::JobMerged,
    Kind::JobFailed,
    Kind::JobCancelled,
];

/// The area the Deploy column's right-click menu is clamped in and hit-tested against: the pane less a blank column on
/// each side.
pub(crate) fn menu_pane(pane: Rect) -> Rect {
    pane.inner(Margin::new(1, 0))
}

/// The layout test's sample (`layout_tests`): a menu of a PR row opened by a right click at `case`'s cell in a `width` x
/// `height` pane, drawn as the view draws it, in [`menu_pane`].
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::Case) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Case, Kind, Placed, Sample};
    let Case::Menu(click) = case else { panic!("the Deploy column samples a menu") };
    let (column, row) = click.at(width, height);
    let menu = Menu::new(vec![column::PrItem::Merge, column::PrItem::CopyLink, column::PrItem::GoToLead], Position::new(column, row));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| menu.draw_at(f, menu_pane(f.area()), 0)).unwrap();
    let pane = Rect::new(0, 0, width, height);
    let boxes = vec![Placed { name: "deploy menu", kind: Kind::Full, rect: menu.rect(menu_pane(pane)), exempt: Vec::new() }];
    Sample { buf: terminal.backend().buffer().clone(), container: pane, boxes }
}

/// What a click on an owned PR row runs: `session open <lead>` through `run_command`, then the focus on the chat slot.
/// `log` is `run/tui/widgets.log`.
struct Opener<'a> {
    home: &'a CpdHome,
    run_command: RunCommand,
    tmux: &'a Tmux,
    cockpit: &'a str,
    log: &'a ViewLog,
}

/// The right-click menu of a PR row, while it is open: its entries and the row it is about, as drawn when it opened.
struct PrMenu {
    menu: Menu<PrItem>,
    row: PrRow,
}

/// What the entry `item` of `row`'s menu does: the title of the dialog that says it failed (what failed), and the result.
/// A success says nothing (commands.md §17 "No notes").
fn pick(item: PrItem, row: &PrRow, opener: &Opener) -> (&'static str, Result<(), CpdError>) {
    match item {
        PrItem::Merge => ("Merge Pull Request", crate::merge::open(opener.tmux, &row.pull)),
        PrItem::Merging => ("Merge Pull Request", Ok(())),
        PrItem::CopyLink => {
            ("Copy Link", opener.tmux.run(&["set-buffer", "-w", "--", row.url.as_str()].map(str::to_owned)).map(drop))
        }
        PrItem::GoToLead => {
            let lead = row.lead.as_ref().expect("Go to Lead is offered only on an owned PR");
            ("Go to Lead", open_lead(opener.run_command, opener.tmux, opener.cockpit, lead))
        }
    }
}

/// Draws and follows, for as long as the process runs: no key ends it. A failed read keeps what is on screen and shows its error in the last row, until a read
/// succeeds ([`kept`]). The wheel over the PRs section scrolls it by one pull request per notch. A left click on an owned
/// PR row (not on its `󱆭` or its merge icon) opens its Lead in the chat slot; a left click on a merge icon opens the merge
/// popup; a failure is the error dialog and a line of the log (commands.md §17 "No notes"). A right click on a PR row
/// opens its menu, which takes every key and click until it closes.
fn show<B: Backend>(
    ctx: &mut Ctx<B>,
    feed: Feed,
    reader: &Arc<Mutex<pace::Owned<read::Follow>>>,
    mut prs: Vec<OpenPr>,
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
    let mut menu = None::<PrMenu>;
    let mut pane = Rect::default();
    // The PRs section's offset, in pull requests: in memory only, kept by a refresh and clamped by the drawing.
    let mut scroll = 0usize;
    loop {
        // the spinner of a working Lead turns: a new frame redraws (commands.md §13.2)
        let frame = frame_now(ports);
        if redraw || (spins(&prs) && Some(frame) != drawn_frame) {
            ctx.draw(|f| {
                pane = f.area();
                last = Some(draw(f, pane, &prs, &machine, scroll, frame));
                if let Some(error) = error.as_ref().or(watch_failure.as_ref()) {
                    draw_error(f, pane, error);
                }
                if let Some(open) = &menu {
                    open.menu.draw_at(f, menu_pane(pane), frame);
                }
            })?;
            (redraw, drawn_frame) = (false, Some(frame));
        }
        let drawn = last.as_ref().expect("the terminal drew");
        scroll = scroll.min(drawn.max_scroll);
        let now = ports.clock.now();
        let wall_ms = wall_ms(ports);
        let until = pump::wake(now, wall_ms, &needs(&prs, sampled_at), schedule.deadline(now));
        let signal = if pump.is_driven() {
            pump.wait_driven(ports, &mut *ctx.input, until, schedule.is_out())
        } else {
            pump.wait(until)
        };
        match signal {
            Signal::Input(event) => {
            redraw = true; // a click, the wheel, a key or a resize
            if let Some(open) = menu.as_mut() {
                // an open menu takes every key and click
                let outcome = match &event {
                    Event::Key(key) if key.kind == KeyEventKind::Press => open.menu.key(*key),
                    Event::Mouse(m) => open.menu.mouse(menu_pane(pane), *m),
                    _ => Outcome::Stay,
                };
                match outcome {
                    Outcome::Stay => {}
                    Outcome::Close => menu = None,
                    Outcome::Pick(item) => {
                        let PrMenu { row, .. } = menu.take().expect("the menu was open");
                        if let (what, Err(e)) = pick(item, &row, opener) {
                            dialog::report(opener.tmux, opener.log, what, &e);
                        }
                        schedule.force(); // a merge starts with no Event
                    }
                }
            } else {
                match event {
                    Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Right) => {
                        let at = Position::new(m.column, m.row);
                        menu = drawn.rows.iter().find(|r| r.area.contains(at)).map(|row| PrMenu { menu: Menu::new(row.items(), at), row: row.clone() });
                    }
                    Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                        let at = Position::new(m.column, m.row);
                        let result = if let Some(icon) = drawn.icons.iter().find(|i| i.area.contains(at)) {
                            Some(("Merge Pull Request", crate::merge::open(opener.tmux, &icon.pull)))
                        } else {
                            drawn.hits.iter().find(|h| h.contains(at)).map(|hit| {
                                ("Go to Lead", open_lead(opener.run_command, opener.tmux, opener.cockpit, &hit.lead))
                            })
                        };
                        if let Some((what, Err(e))) = result {
                            dialog::report(opener.tmux, opener.log, what, &e);
                        }
                        schedule.force();
                    }
                    Event::Mouse(m) if drawn.prs.contains(Position::new(m.column, m.row)) => match m.kind {
                        MouseEventKind::ScrollDown => scroll = (scroll + 1).min(drawn.max_scroll),
                        MouseEventKind::ScrollUp => scroll = scroll.saturating_sub(1),
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
                    if let Some((next, live)) = kept(result, &mut error) {
                        if next.is_some() {
                            ctx.snapshot_read();
                        }
                        // A merge starts and ends with no Event of the store: the markers are read on each snapshot.
                        let mut next = next.unwrap_or_else(|| prs.clone());
                        mark_merging(&mut next, &live);
                        if next != prs {
                            prs = next;
                            redraw = true;
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
            let (shared, home, retry, ports) = (Arc::clone(reader), opener.home.clone(), error.is_some(), ports.clone());
            reads.request(move || {
                pace::Owned::read(&shared, |store, follow| {
                    let moved = follow.poll_changes(store)?.any();
                    let prs = if moved || kind.force || retry { Some(read::open_prs(store)?.0) } else { None };
                    let open = read::open_pulls(store)?;
                    Ok((prs, crate::merge::marker::live_with(&ports, &home, &open)))
                })
            });
        }
    }
}

/// What a read of the reader thread returns: the snapshot when it was taken, and the pull requests with a live merge marker.
type Read = Result<(Option<Vec<OpenPr>>, BTreeSet<String>), CpdError>;

/// A shown PR's Lead is working, or its merge runs (a clean one, which shows the spinner in place of the merge icon): its
/// spinner turns.
fn spins(prs: &[OpenPr]) -> bool {
    prs.iter().any(|p| p.lead_working || (p.merging && p.clean))
}

/// The clocks the Dispatch column's screen needs: `frame` is `spins(prs)` (a shown PR's Lead works, or a clean PR's
/// merge runs); `second` is false; `at` is always `Some(sampled_at + SAMPLE_EVERY)`.
fn needs(prs: &[OpenPr], sampled_at: Instant) -> pump::Needs {
    pump::Needs { frame: spins(prs), second: false, at: Some(sampled_at + SAMPLE_EVERY) }
}

/// Sets `merging` of each PR whose merge runs: `live` is the pull requests with a live merge marker (hs-d495).
fn mark_merging(prs: &mut [OpenPr], live: &std::collections::BTreeSet<String>) {
    for pr in prs {
        pr.merging = live.contains(&format!("{}#{}", pr.repo, pr.number));
    }
}

/// A click on a tab or on an owned PR row: `session open <id>`, which moves that Lead into the chat slot, then `select-pane`
/// on the chat. The plan view's tabs and this column's rows share it.
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

/// The pane option holding the Job a Worker pane shows (its border title shows it), so a second click on the same Job
/// closes it.
const OPT_JOB: &str = "@cpd_job";

/// The pane option holding the Attempt a Worker pane shows.
const OPT_ATTEMPT: &str = "@cpd_attempt";

/// The Job a Worker pane shows.
fn worker_shows(tmux: &Tmux, pane: &PaneId) -> Result<String, CpdError> {
    let shown = tmux.run(&["display", "-p", "-t", pane.as_str(), &format!("#{{{OPT_JOB}}}")].map(str::to_owned))?;
    Ok(shown.trim_end_matches('\n').to_owned())
}

/// `set -p` of the Worker pane's options: the Job and its current Attempt.
fn mark_worker(tmux: &Tmux, pane: &PaneId, job: &str, attempt: &str) -> Result<(), CpdError> {
    for (option, value) in [(OPT_JOB, job), (OPT_ATTEMPT, attempt)] {
        tmux.run(&["set", "-p", "-t", pane.as_str(), option, value].map(str::to_owned))?;
    }
    Ok(())
}

/// The Worker pane's program, in cpd's home: the Attempt form `tui view worker <attempt> --plan <plan>`, or, for a Job with
/// no Attempt (`attempt` empty), the Job form `tui view worker --job <job> --plan <plan>`.
fn worker_spawn(home: &CpdHome, plan: &PlanId, job: &str, attempt: &str) -> Spawn {
    let target: Vec<&str> = if attempt.is_empty() { vec!["--job", job] } else { vec![attempt] };
    let mut argv: Vec<String> = [PROGRAM, "tui", "view", "worker"].map(str::to_owned).into();
    argv.extend(target.into_iter().map(str::to_owned));
    argv.extend(["--plan", plan.as_str()].map(str::to_owned));
    Spawn {
        cwd: Some(home.root().to_owned()),
        env: BTreeMap::from([("CPD_HOME".to_owned(), home.root().display().to_string())]),
        argv,
    }
}

/// Opens the read-only Worker pane beside the chat (`split-window -h -d -l 50%`, so the focus stays) for `job`'s
/// `attempt` (none: the Job form, `@cpd_attempt` empty), and retargets it (`respawn-pane -k`) when it already shows a Worker. The plan view's pills and the tree's
/// `job-stuck` click share it.
pub(crate) fn open_worker(
    home: &CpdHome,
    tmux: &Tmux,
    cockpit: &str,
    plan: &PlanId,
    job: &str,
    attempt: Option<&AttemptId>,
) -> Result<(), CpdError> {
    let panes = main_panes(tmux, cockpit)?;
    let attempt = attempt.map(ToString::to_string).unwrap_or_default();
    if let Some(pane) = view_pane(&panes, cockpit, View::Worker) {
        let spawn = worker_spawn(home, plan, job, &attempt);
        let mut args = ["respawn-pane", "-k", "-t", pane.pane.as_str()].map(str::to_owned).to_vec();
        if let Some(cwd) = &spawn.cwd {
            args.extend(["-c".to_owned(), cwd.display().to_string()]);
        }
        for (key, value) in &spawn.env {
            args.extend(["-e".to_owned(), format!("{key}={value}")]);
        }
        args.push("--".to_owned());
        args.extend(spawn.argv);
        tmux.run(&args)?;
        return mark_worker(tmux, &pane.pane, job, &attempt);
    }
    let Some(chat) = chat_slot(&panes, cockpit) else {
        return Err(CpdError::new(ErrorCode::TmuxFailed, "There is no chat slot to open the Worker beside."));
    };
    let pane = tmux.split_window(&chat.pane, Split::Right, Some("50%"), &worker_spawn(home, plan, job, &attempt))?;
    tmux.set_pane_view(&pane, View::Worker)?;
    mark_worker(tmux, &pane, job, &attempt)
}

/// A click on a pill: [`open_worker`], or closes the Worker pane when it already shows `job`. Returns whether it is open now.
pub(crate) fn toggle_worker(
    home: &CpdHome,
    tmux: &Tmux,
    cockpit: &str,
    plan: &PlanId,
    job: &str,
    attempt: Option<&AttemptId>,
) -> Result<bool, CpdError> {
    let panes = main_panes(tmux, cockpit)?;
    if let Some(pane) = view_pane(&panes, cockpit, View::Worker) {
        if worker_shows(tmux, &pane.pane)? == job {
            tmux.kill_pane(&pane.pane)?;
            return Ok(false);
        }
    }
    open_worker(home, tmux, cockpit, plan, job, attempt)?;
    Ok(true)
}

/// Closes the Worker pane when it shows `job`; any other Worker pane stays.
pub(crate) fn close_worker(tmux: &Tmux, cockpit: &str, job: &str) -> Result<(), CpdError> {
    let panes = main_panes(tmux, cockpit)?;
    if let Some(pane) = view_pane(&panes, cockpit, View::Worker) {
        if worker_shows(tmux, &pane.pane)? == job {
            tmux.kill_pane(&pane.pane)?;
        }
    }
    Ok(())
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

    fn pr(working: bool) -> read::OpenPr {
        read::OpenPr {
            repo: "acme/repo".to_owned(),
            number: 1,
            url: String::new(),
            title: String::new(),
            draft: false,
            checks: read::Checks::Passed,
            review_decision: read::ReviewDecision::None,
            conflicting: false,
            update: None,
            clean: false,
            active: false,
            lead: Some(SessionId::parse("l-a").unwrap()),
            lead_working: working,
            plan_failed: false,
            plan_phase: None,
            merging: false,
        }
    }

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
