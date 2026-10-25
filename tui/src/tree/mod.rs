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
//! - [`inbox`]: the Inbox section at the column's bottom (an empty row, the pill title, up to 3 items, the fold widget's `╰󰬦 expand` / `╰󰬬 collapse` row) and what a click on an item does:
//!   `session open`, or nothing (the label is an OSC 8 link);
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
//! Session's pane; `?` opens the legend. After the last Session the cursor goes on into the Inbox's item rows (Enter jumps, `s` snoozes, `d` dismisses; the right-click menu of an item has the same entries). A Context is a fold ([`crate::fold`]): only a click on its `╰󰬦 expand` / `╰󰬬 collapse` row shows or hides its archived rows (its live rows always show; with none archived it closes with `╰─`, with no row at all it ends with the terminator `╰󰬪`; hs-d574); a click on the title row of a live Context opens its Manager (`context manager`, then `session open`), on a live Session opens it, on an archived Session shows its history; a right click opens the menu.

pub(crate) mod actions;
mod ask;
mod cursor;
mod folds;
mod inbox;
#[cfg(test)]
mod inbox_indent;
mod kinds;
#[cfg(test)]
mod look;
#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod margin_tests;
#[cfg(test)]
mod scroll_tests;
#[cfg(test)]
mod width_tests;
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
pub(crate) use inbox::layout_sample as layout_inbox_sample;
#[cfg(test)]
pub(crate) use view::layout_fold_sample;
#[cfg(test)]
pub(crate) use view::layout_sample;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod needs_tests;

/// The clocks the tree's screen needs: `frame` is `view.animated()`, `second` is `!view.tree.inbox.is_empty()` (an
/// age text is on screen), `at` is `None` (the safety read covers the engine's lock, a snooze's end and the merge marker).
fn needs(view: &View) -> crate::pump::Needs {
    crate::pump::Needs { frame: view.animated(), second: !view.tree.inbox.is_empty(), at: None }
}

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
use crate::pump::{self, Pump, Schedule, Signal};
use time::OffsetDateTime;

use super::background::Background;
use super::pace::{self, Reads};
use super::runner::kept;
use super::{Capture, run_view};
use super::{palette, read};
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::inbox::model;
use cpd_core::output::Output;
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
    merging: Result<BTreeSet<String>, CpdError>,
}

/// What the reader thread brings back: the poll, and the snapshot when the poll (or a force) called for one.
struct Read {
    poll: Polled,
    snapshot: Option<Result<(Option<OffsetDateTime>, read::Tree), CpdError>>,
}

pub(super) fn run() -> Result<Output, CpdError> {
    run_view(
        "tree",
        Capture::MouseAndFocus,
        || {
            let home = CpdHome::from_env()?;
            let config = Config::load(&home)?;
            let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
            let snooze_end = read::snooze_end(&mut store)?;
            let mut view = View::new(read::tree(&mut store)?, false);
            // the first read of the merges is the first snapshot's: it runs here, before the terminal is set up
            view.set_merging(crate::merge::marker::live(&home, &read::open_pulls(&mut store).unwrap_or_default()));
            let follow = read::Follow::new(&mut store, KINDS, view.tree.events_cursor)?;
            let poll = Duration::from_millis(u64::from(config.events.poll_ms));
            let runner = Arc::new(Runner::new(home.clone(), &config));
            // the reads after them run on the reader thread, with a store of its own (R4)
            let reader = pace::Owned::new(home.clone(), u64::from(config.timeouts.store_busy_ms), follow);
            Ok((home, view, (reader, snooze_end), poll, runner))
        },
        |terminal, (home, view, reader, poll, runner)| Ok(event_loop(terminal, &home, view, reader, poll, &runner)?),
    )
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    home: &CpdHome,
    mut view: View,
    (reader, mut snooze_end): (Arc<Mutex<pace::Owned<read::Follow>>>, Option<OffsetDateTime>),
    poll: Duration,
    runner: &Arc<Runner>,
) -> std::io::Result<()> {
    let mut log = Log { file: ViewLog::of(home, "tree"), unwritable_said: false };
    let mut trace = Trace::new(std::env::var("CPD_TUI_TRACE").ok().as_deref());
    let epoch = Instant::now();
    let pump = Pump::new(event::read);
    // the store's directory is watched once; a watcher that cannot start is said in the error row, and the schedule polls
    let (_watch, watch_error, mut schedule) = match read::watch::Watch::start(home.root(), pump.waker()) {
        Ok(watch) => (Some(watch), None, Schedule::new(Instant::now(), pump::SAFETY)),
        Err(why) => (
            None,
            Some(format!("no commit watcher: {}; polling every {} ms", why.message(), poll.as_millis())),
            Schedule::new(Instant::now(), poll),
        ),
    };
    // a failed snapshot is retried at the next refresh, with no Event needed; a failed poll (of the Events or of the
    // engine's lock) at the next poll. All are shown (runner::kept): the footer says the snapshot's error, else the
    // follow's, else the lock's.
    let (mut snapshot_error, mut follow_error, mut engine_error, mut open_error) = (None, None, None, None);
    // what the last draw showed of the clock (the spinner's frame, the second the Inbox items' ages were taken at), and
    // whether anything changed since: the pane is written to only then
    let (mut redraw, mut drawn_frame, mut drawn_second) = (true, palette::frame(SystemTime::now()), unix_second());
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
            if let Some(merging) = kept(poll.merging, &mut open_error) {
                redraw |= view.set_merging(merging);
            }
            if let Some(next) = done.snapshot {
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
                .or_else(|| open_error.clone())
                .or_else(|| watch_error.clone());
        }
        if let Some(kind) = schedule.take(Instant::now()) {
            // one job does the poll, then the snapshot when something changed (a pull request's checks change with no
            // Event, §17; the Inbox changes with no Event when a snooze ends or the engine stops or starts, `engine-down`;
            // a merge starts and ends with no Event: its marker, hs-d495, is read on each poll), or on `force`, or after a
            // failed snapshot
            let (shared, home) = (Arc::clone(&reader), home.clone());
            let (engine_shown, merging_shown) = (view.tree.engine_running, view.merging().clone());
            let retry = snapshot_error.is_some() || kind.force || snooze_end.is_some_and(|end| OffsetDateTime::now_utc() >= end);
            reads.request(move || {
                let (changes, merging) = (
                    pace::Owned::read(&shared, |store, follow| follow.poll_changes(store)),
                    pace::Owned::read(&shared, |store, _| read::open_pulls(store)).map(|open| crate::merge::marker::live(&home, &open)),
                );
                let engine = model::engine_running(&home);
                let due = retry
                    || changes.as_ref().is_ok_and(|c| c.any())
                    || engine.as_ref().is_ok_and(|on| *on != engine_shown)
                    || merging.as_ref().is_ok_and(|m| *m != merging_shown);
                let snapshot = due.then(|| {
                    pace::Owned::read(&shared, |store, _| read::snooze_end(store).and_then(|end| Ok((end, read::tree(store)?))))
                });
                Read { poll: Polled { changes, engine, merging }, snapshot }
            });
        }
        // An engine down for 60 s is told to the Managers (commands.md §13.2, hs-d363): this view only, not every pane. Its
        // lines go to the log, never to the terminal or the frame; a log that cannot be written is a dialog.
        let lines = view.watch_engine(Instant::now(), &mut |action| actions::call(action), &mut || runner.live_managers());
        for line in lines {
            show(log.line(&line), &mut popups, runner);
        }
        // the folds file follows every change of the folds (a write only when they changed)
        if let Some(line) = view.save_folds() {
            show(log.line(&line), &mut popups, runner);
        }
        let (frame, second) = (palette::frame(SystemTime::now()), unix_second());
        let wants = needs(&view);
        redraw |= view.error != error || (wants.frame && frame != drawn_frame);
        // the Inbox items' ages are redrawn every second, from `opened_at`
        redraw |= wants.second && second != drawn_second;
        if redraw {
            let (drawn, written) = trace.draw(epoch, &log.file, frame, || terminal.draw(|f| view.draw(f, frame)).map(drop));
            drawn?;
            if let Err(why) = written {
                show(log.line(&format!("failed: trace: {why}")), &mut popups, runner);
            }
            (redraw, drawn_frame, drawn_second) = (false, frame, second);
        }
        let wall_ms = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let now = Instant::now();
        let event = match pump.wait(pump::wake(now, wall_ms, &wants, schedule.deadline(now))) {
            Signal::Input(event) => event,
            Signal::InputFailed(why) => return Err(why),
            Signal::Commit => {
                schedule.commit();
                continue;
            }
            Signal::Read | Signal::Due => continue,
        };
        redraw = true; // a key, a click, a resize, the focus
        let frame = palette::frame(SystemTime::now());
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
                let (drawn, written) = trace.draw(epoch, &log.file, frame, || terminal.draw(|f| view.draw(f, frame)).map(drop));
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
            Effect::Merge(pull) => {
                // the merge popup returns when it closes; only a failure is said, and a merge shows on the next snapshot
                let runner = Arc::clone(runner);
                popups.submit(move || runner.merge(&pull));
                schedule.force();
            }
            Effect::Curate(kind, context) => {
                // the checklist popup returns when it closes; a refusal is said in a `text` dialog by the runner
                let runner = Arc::clone(runner);
                popups.submit(move || runner.curate(kind, &context));
                schedule.force();
            }
            Effect::Run(action) => {
                let runner = Arc::clone(runner);
                actions.submit(move || runner.run(&action));
            }
            text @ Effect::Text { .. } => show(text, &mut popups, runner),
            Effect::Steps(steps) => {
                show(said(&mut view, &mut log, runner.run_all(&steps)), &mut popups, runner);
                schedule.force();
            }
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

/// The Unix second now: the Inbox items' ages change with it.
fn unix_second() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
