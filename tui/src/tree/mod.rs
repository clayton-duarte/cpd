//! The `tree` view (commands.md §13.2, §17; contract §6.2): the cockpit's left
//! column, ported from the spike's Context view (branch `cpd/cpd-ui`:
//! `context.rs`, `sessions.rs`, `cursor.rs`, `actions.rs`, `prompt.rs`,
//! `separator.rs`; the prompt is gone, its questions are dialog popups now) onto the read model.
//!
//! - [`rows`]: Contexts, their Plans (each Plan's row is its Lead's) and the forked Sessions after them (B1), labelled by title and
//!   name, and the archived ones after the live ones inside the Context's block;
//! - [`view`]: the state, keys and drawing; [`cursor`], [`menu`]: its parts (the right-click menu); [`ask`]: the dialog popups (the dialog kit's) it opens
//!   before a command, with no inline prompt row;
//! - [`actions`]: what it does, as `cpd` commands (`session open`, `plan new`, `session close`, the menu's), and the
//!   legend popup; [`slug`]: the id of a new Context or Plan, from its title;
//! - [`folds`]: the folds (collapsed Contexts, expanded `…` rows) kept in `run/tui/tree-folds.json` across restarts;
//! - [`super::background`]: a command or the legend popup runs on a thread, one command at a time; the footer says
//!   `running: <command>` meanwhile and then its result, and the loop keeps drawing and reading keys (tui-W7);
//! - [`kinds`]: the Events it follows, in process ([`read::Follow`]), which tell it when to take a new snapshot.
//!
//! It never restarts a stopped engine (the supervisor does, contract §6.6); it tells the live Managers when the engine has
//! been down for 60 s ([`view::View::watch_engine`], hs-d363). It writes nothing to its terminal outside the frame: its
//! lines go to `run/tui/tree.log` ([`super::viewlog`]), and a failure the User must see opens a `text` dialog
//! ([`crate::dialog::show_error`]); no view draws a note (commands.md §17 "No notes").
//!
//! Keys and clicks are the reference's (commands.md §13.2, `tui/README.md` "Context column" on `main`): arrows
//! or `j`/`k` move the cursor between Sessions (shown after the first arrow key, while the pane has focus);
//! Enter opens the cursor's Session in the chat slot and focuses it; `n` starts a new Plan; `x` closes a
//! Session's pane; `?` opens the legend. A Context is a fold ([`crate::fold`]): only a click on its `╰󰬦 expand` / `╰󰬬 collapse` row shows or hides its archived rows (its live rows always show; with none archived it closes with `╰─`, with no row at all it ends with the terminator `╰󰬪`; hs-d574); a click on the title row of a live Context opens its Manager (`context manager`, then `session open`), on a live Session opens it, on an archived Session shows its history; a right click opens the menu.

pub(crate) mod actions;
mod ask;
mod cursor;
pub(crate) mod folds;
#[cfg(test)]
mod archived_folder_tests;
mod kinds;
#[cfg(test)]
mod look;
#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod scroll_tests;
#[cfg(test)]
mod fixing_tests;
#[cfg(test)]
mod new_context_tests;
#[cfg(test)]
#[path = "another_owner_tests.rs"]
mod another_owner_tests;
#[cfg(test)]
mod counter_tests;
#[cfg(test)]
mod asleep_tests;
#[cfg(test)]
mod loop_guard_tests;
mod menu;
#[cfg(test)]
mod prepared_tests;
mod rows;
mod slug;
mod view;
#[cfg(test)]
pub(crate) use view::layout_fold_sample;
#[cfg(test)]
pub(crate) use view::layout_sample;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod margin_tests;
#[cfg(test)]
mod needs_tests;

/// The clocks the tree's screen needs: `frame` is `view.animated()`, `second` is false (no row shows a time), `at` is `None`
/// (the safety read covers the engine's lock, which the dots read through the Inbox, and a snooze's end).
fn needs(view: &View) -> crate::pump::Needs {
    crate::pump::Needs { frame: view.animated(), second: false, at: None }
}

use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::io::Write;

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event};
use crate::pump::{self, Pump, Schedule, Signal};
use time::OffsetDateTime;

use super::background::Background;
use super::pace::{self, Reads};
use super::runner::kept;
use super::view_ports::{Ctx, Input, Tty, frame_now, run_view_with, snooze_over, wall_ms};
use super::{Capture, run_view_on};
use super::read;
use cpd_core::clock;
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::inbox::model;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use actions::{Failed, Runner};
use folds::FoldsFile;
use kinds::KINDS;
pub(crate) use view::View;
use view::Effect;
use super::viewlog::ViewLog;
use crate::trace::Trace;

/// What one poll of the reader thread found: each read apart, so each has its own error (`runner::kept`).
struct Polled {
    changes: Result<read::Changes, CpdError>,
    engine: Result<bool, CpdError>,
}

/// What the reader thread brings back: the poll, and the snapshot when the poll (or a force) called for one.
struct Read {
    poll: Polled,
    snapshot: Option<Result<(Option<OffsetDateTime>, read::Tree), CpdError>>,
}

/// What the view reads before it draws: the home, the first snapshot's view, the reader (and when the first snooze ends),
/// the poll and the runner of the commands.
type Opened = (CpdHome, View, (Arc<Mutex<pace::Owned<read::Follow>>>, Option<OffsetDateTime>), Duration, Arc<Runner>);

/// The first read: a failure is the command's error, with the screen untouched.
fn open(ports: &Ports) -> Result<Opened, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let now = clock::timestamp(ports.clock.utc());
    let snooze_end = read::snooze_end_at(&mut store, Some(&now))?;
    let view = View::new(read::tree_at(&mut store, Some(&now))?, false);
    let follow = read::Follow::new(&mut store, KINDS, view.tree.events_cursor)?;
    let poll = Duration::from_millis(u64::from(config.events.poll_ms));
    let runner = Arc::new(Runner::new(home.clone(), &config, ports));
    // the reads after them run on the reader thread, with a store of its own (R4)
    let reader = pace::Owned::new(home.clone(), u64::from(config.timeouts.store_busy_ms), follow);
    Ok((home, view, (reader, snooze_end), poll, runner))
}

pub(super) fn run(ports: &Ports) -> Result<Output, CpdError> {
    run_view_on(ports, "tree", Capture::MouseAndFocus, || open(ports), |ctx, opened| Ok(event_loop(ctx, opened, Driven::No)?))
}

/// [`run`] on a terminal, its facts and its input that the caller gives (a test's `TestBackend`, scripted input and a
/// [`Tty`] that says whether there is a terminal): the same refusals before anything is drawn. The loop is driven by
/// `input` (nothing is read on a thread, nothing waits in real time) and watches no directory: it polls.
pub fn run_on<B: Backend, W: Write>(
    ports: &Ports,
    tty: &mut dyn Tty,
    out: &mut W,
    terminal: Terminal<B>,
    input: &mut dyn Input,
) -> Result<Output, CpdError> {
    run_view_with(ports, tty, out, terminal, input, "tree", Capture::MouseAndFocus, || open(ports), |ctx, opened| Ok(event_loop(ctx, opened, Driven::Yes)?))
}

/// Whether the loop is driven by the [`Ctx`]'s input ([`run_on`]) or reads the terminal on a thread and watches the store's directory.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Driven {
    Yes,
    No,
}

fn event_loop<B: Backend>(
    ctx: &mut Ctx<B>,
    (home, mut view, (reader, mut snooze_end), poll, runner): Opened,
    driven: Driven,
) -> std::io::Result<()> {
    let ports = ctx.ports;
    let (runner, home) = (&runner, &home);
    let mut log = Log { file: ViewLog::of(home, "tree"), unwritable_said: false };
    let mut trace = Trace::new(std::env::var("CPD_TUI_TRACE").ok().as_deref());
    let epoch = ports.clock.now();
    let pump = if driven == Driven::Yes { Pump::undriven() } else { Pump::new(event::read) };
    // the store's directory is watched once; a watcher that cannot start is said in the error row, and the schedule polls.
    // A driven loop (a test's) watches nothing, says nothing of it and polls.
    let (_watch, watch_error, mut schedule) = if driven == Driven::Yes {
        (None, None, Schedule::new(ports.clock.now(), poll))
    } else {
        match read::watch::Watch::start(home.root(), pump.waker()) {
            Ok(watch) => (Some(watch), None, Schedule::new(ports.clock.now(), pump::SAFETY)),
            Err(why) => (
                None,
                Some(format!("no commit watcher: {}; polling every {} ms", why.message(), poll.as_millis())),
                Schedule::new(ports.clock.now(), poll),
            ),
        }
    };
    // a failed snapshot is retried at the next refresh, with no Event needed; a failed poll (of the Events or of the
    // engine's lock) at the next poll. All are shown (runner::kept): the footer says the snapshot's error, else the
    // follow's, else the lock's.
    let (mut snapshot_error, mut follow_error, mut engine_error) = (None, None, None);
    // what the last draw showed of the clock (the spinner's frame), and whether anything changed since: the pane is written
    // to only then
    let (mut redraw, mut drawn_frame) = (true, frame_now(ports));
    // the commands and the popup run off this thread (tui-W7): the loop goes on drawing, reading keys and the follower
    // meanwhile. A popup returns when it closes, so it does not hold up the commands.
    let mut actions = Background::<Result<String, Failed>>::with_waker(pump.waker());
    let mut popups = Background::<Result<(), String>>::with_waker(pump.waker());
    // the reads run on the reader thread (R4), one at a time: none is asked for while one is out
    let mut reads = Reads::<Read>::with_waker(pump.waker());
    view.error = watch_error.clone();
    for line in view.keep_folds(FoldsFile::at(home)) {
        show(log.line(&line), &mut popups, runner);
    }
    loop {
        let error = view.error.clone();
        while let Some(result) = actions.finished() {
            show(said(&mut view, &mut log, result), &mut popups, runner);
            // the tree shows the result on this loop's snapshot
            schedule.force();
        }
        while let Some(result) = popups.finished() {
            // only a failure is logged: a popup that failed cannot be asked to say so
            if let Err(why) = result {
                show(log.line(&format!("failed: popup: {why}")), &mut popups, runner);
            }
        }
        while let Some(done) = trace.read(|| reads.take()) {
            schedule.done();
            let poll = done.poll;
            kept(poll.changes, &mut follow_error);
            kept(poll.engine, &mut engine_error);
            if let Some(next) = done.snapshot {
                ctx.snapshot_read();
                if let Some((end, tree)) = kept(next, &mut snapshot_error) {
                    snooze_end = end;
                    let t = &view.tree;
                    if (&tree.contexts, &tree.inbox, tree.engine_running) != (&t.contexts, &t.inbox, t.engine_running) {
                        view.refresh(tree);
                        redraw = true;
                    }
                }
            }
            view.error = snapshot_error
                .clone()
                .or_else(|| follow_error.clone())
                .or_else(|| engine_error.clone())
                .or_else(|| watch_error.clone());
        }
        if let Some(kind) = schedule.take(ports.clock.now()) {
            // one job does the poll, then the snapshot when something changed (a pull request's checks change with no
            // Event, §17; the Inbox changes with no Event when a snooze ends or the engine stops or starts, `engine-down`),
            // or on `force`, or after a failed snapshot
            let (shared, home) = (Arc::clone(&reader), home.clone());
            let now_text = clock::timestamp(ports.clock.utc());
            let engine_shown = view.tree.engine_running;
            let retry = snapshot_error.is_some() || kind.force || snooze_over(ports, snooze_end);
            reads.request(move || {
                let changes = pace::Owned::read(&shared, |store, follow| follow.poll_changes(store));
                let engine = model::engine_running(&home);
                let due = retry
                    || changes.as_ref().is_ok_and(|c| c.any())
                    || engine.as_ref().is_ok_and(|on| *on != engine_shown);
                let snapshot = due.then(|| {
                    pace::Owned::read(&shared, |store, _| {
                        read::snooze_end_at(store, Some(&now_text)).and_then(|end| Ok((end, read::tree_at(store, Some(&now_text))?)))
                    })
                });
                Read { poll: Polled { changes, engine }, snapshot }
            });
        }
        // An engine down for 60 s is told to the Managers (commands.md §13.2, hs-d363): this view only, not every pane. Its
        // lines go to the log, never to the terminal or the frame; a log that cannot be written is a dialog.
        let lines = view.watch_engine(ports.clock.now(), &mut |action| actions::call_with(&*ports.runner, action), &mut || runner.live_managers());
        for line in lines {
            show(log.line(&line), &mut popups, runner);
        }
        // the folds file follows every change of the folds (a write only when they changed)
        if let Some(line) = view.save_folds_with(ports) {
            show(log.line(&line), &mut popups, runner);
        }
        let frame = frame_now(ports);
        let wants = needs(&view);
        redraw |= view.error != error || (wants.frame && frame != drawn_frame);
        if redraw {
            let (drawn, written) = trace.draw(epoch, &log.file, frame, || ctx.draw(|f| view.draw(f, frame)));
            drawn?;
            if let Err(why) = written {
                show(log.line(&format!("failed: trace: {why}")), &mut popups, runner);
            }
            (redraw, drawn_frame) = (false, frame);
        }
        let wall_ms = wall_ms(ports);
        let now = ports.clock.now();
        let wake = pump::wake(now, wall_ms, &wants, schedule.deadline(now));
        let signal = match driven {
            Driven::No => pump.wait(wake),
            Driven::Yes => {
                let busy = schedule.is_out() || actions.busy() || popups.busy();
                pump.wait_driven(ports, &mut *ctx.input, wake, busy)
            }
        };
        let event = match signal {
            Signal::Input(event) => event,
            Signal::InputFailed(why) => return Err(why),
            Signal::Commit => {
                schedule.commit();
                continue;
            }
            Signal::Read | Signal::Due => continue,
        };
        redraw = true; // a key, a click, a resize, the focus
        let frame = frame_now(ports);
        let effect = match event {
            Event::FocusGained => {
                view.focus(true);
                Effect::None
            }
            Event::FocusLost => {
                view.focus(false);
                Effect::None
            }
            Event::Key(key) => {
                view.focus(true); // a key reached this pane
                view.key(key)
            }
            Event::Mouse(m) => view.mouse(m),
            _ => Effect::None,
        };
        match effect {
            Effect::None => {}
            Effect::Ask(question) => {
                // `display-popup` returns when the popup closes; a cancel runs nothing. The tree is drawn first, so it
                // shows the item picked, not the menu, behind the popup.
                let (drawn, written) = trace.draw(epoch, &log.file, frame, || ctx.draw(|f| view.draw(f, frame)));
                drawn?;
                if let Err(why) = written {
                    show(log.line(&format!("failed: trace: {why}")), &mut popups, runner);
                }
                let title = question.title();
                match runner.ask(question) {
                    Ok(None) => {}
                    Ok(Some(action)) => {
                        show(said(&mut view, &mut log, runner.run(&action)), &mut popups, runner);
                        // the tree shows the result on this loop's snapshot
                        schedule.force();
                    }
                    // the dialog failed, or the answer is refused (a title no id can be made of): the User must see it
                    Err(why) => {
                        show(log.line(&format!("failed: {title}: {why}")), &mut popups, runner);
                        show(Effect::Text { title: title.to_owned(), text: why }, &mut popups, runner);
                    }
                }
            }
            Effect::Legend => {
                let runner = Arc::clone(runner);
                popups.submit(move || runner.legend());
            }
            Effect::Run(action) => {
                let runner = Arc::clone(runner);
                actions.submit(move || runner.run(&action));
            }
            text @ Effect::Text { .. } => show(text, &mut popups, runner),
        }
    }
}

/// The view's log, `run/tui/tree.log`. A log that cannot be written is a failure the User must see, said once.
struct Log {
    file: ViewLog,
    unwritable_said: bool,
}

impl Log {
    /// Appends `line`; the dialog to open when the log cannot be written (the first time only), else [`Effect::None`].
    fn line(&mut self, line: &str) -> Effect {
        match self.file.append(line) {
            Err(why) if !std::mem::replace(&mut self.unwritable_said, true) => Effect::Text { title: "Tree Log".into(), text: why },
            _ => Effect::None,
        }
    }
}

/// The result of an action: a line of the log, and the dialog of a failure (a success says nothing).
fn said(view: &mut View, log: &mut Log, result: Result<String, Failed>) -> Effect {
    match result {
        Ok(said) => log.line(&format!("ok: {said}")),
        Err(Failed { step, why }) => {
            let logged = log.line(&format!("failed: {}: {why}", step.argv().join(" ")));
            match view.finished(&step, Err(why)) {
                Effect::None => logged,
                dialog => dialog,
            }
        }
    }
}

/// Opens the `text` dialog of `effect`, if it is one, off this thread: the loop goes on drawing while it is open.
fn show(effect: Effect, popups: &mut Background<Result<(), String>>, runner: &Arc<Runner>) {
    if let Effect::Text { title, text } = effect {
        let runner = Arc::clone(runner);
        popups.submit(move || runner.error(&title, &text));
    }
}
