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

mod actions;
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
mod margin_tests;
#[cfg(test)]
mod fixing_tests;
#[cfg(test)]
mod new_context_tests;
#[cfg(test)]
mod counter_tests;
mod menu;
mod rows;
mod slug;
mod view;

#[cfg(test)]
mod fixture;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
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
use view::{Effect, View};
use super::viewlog::ViewLog;
use crate::trace::Trace;

/// A view takes at most one snapshot per this long, whatever the Event rate (commands.md §17).
const SNAPSHOT_EVERY: Duration = Duration::from_millis(500);
/// What one poll of the reader thread found: each read apart, so each has its own error (`runner::kept`).
struct Polled {
    changes: Result<read::Changes, CpdError>,
    engine: Result<bool, CpdError>,
    merging: Result<BTreeSet<String>, CpdError>,
}

/// What the reader thread brings back.
enum Read {
    Poll(Polled),
    Snapshot(Result<(Option<OffsetDateTime>, read::Tree), CpdError>),
}

/// A new snapshot is due: an Event came, or a pull request row changed (a pull request's checks change with no Event,
/// §17), and `since` the last snapshot reaches [`SNAPSHOT_EVERY`]. With nothing changed none is ever due.
fn snapshot_due(dirty: bool, since: Duration) -> bool {
    dirty && since >= SNAPSHOT_EVERY
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
    let mut dirty = false;
    let mut last_snapshot = Instant::now();
    let mut last_poll = Instant::now();
    // a failed snapshot is retried at the next refresh, with no Event needed; a failed poll (of the Events or of the
    // engine's lock) at the next poll. All are shown (runner::kept): the footer says the snapshot's error, else the
    // follow's, else the lock's.
    let (mut snapshot_error, mut follow_error, mut engine_error, mut open_error) = (None, None, None, None);
    // what the last draw showed of the clock (the spinner's frame, the second the Inbox items' ages were taken at), and
    // whether anything changed since: the pane is written to only then
    let (mut redraw, mut drawn_frame, mut drawn_second) = (true, palette::frame(SystemTime::now()), unix_second());
    // the commands and the popup run off this thread (tui-W7): the loop goes on drawing, reading keys and the follower
    // meanwhile. A popup returns when it closes, so it does not hold up the commands.
    let mut actions = Background::<Result<String, Failed>>::new();
    let mut popups = Background::<Result<(), String>>::new();
    // the reads run on the reader thread (R4), one at a time: none is asked for while one is out
    let mut reads = Reads::<Read>::new();
    let mut reading = false;
    for line in view.keep_folds(FoldsFile::at(home)) {
        show(log.line(&line), &mut popups, runner);
    }
    loop {
        let error = view.error.clone();
        while let Some(result) = actions.finished() {
            show(said(&mut view, &mut log, result), &mut popups, runner);
            // the tree shows the result on this loop's snapshot
            dirty = true;
            last_snapshot = Instant::now() - SNAPSHOT_EVERY;
        }
        while let Some(result) = popups.finished() {
            // only a failure is logged: a popup that failed cannot be asked to say so
            if let Err(why) = result {
                show(log.line(&format!("failed: popup: {why}")), &mut popups, runner);
            }
        }
        while let Some(done) = trace.read(|| reads.take()) {
            reading = false;
            match done {
                Read::Poll(poll) => {
                    dirty |= kept(poll.changes, &mut follow_error).is_some_and(|changes| changes.any());
                    dirty |= kept(poll.engine, &mut engine_error).is_some_and(|on| on != view.tree.engine_running);
                    if let Some(merging) = kept(poll.merging, &mut open_error) {
                        redraw |= view.set_merging(merging);
                    }
                }
                Read::Snapshot(next) => {
                    if let Some((end, tree)) = kept(next, &mut snapshot_error) {
                        snooze_end = end;
                        let t = &view.tree;
                        if (&tree.contexts, &tree.inbox, tree.engine_running) != (&t.contexts, &t.inbox, t.engine_running) {
                            view.refresh(tree);
                            redraw = true;
                        }
                    }
                }
            }
            view.error = snapshot_error.clone().or_else(|| follow_error.clone()).or_else(|| engine_error.clone()).or_else(|| open_error.clone());
        }
        // a due snapshot goes before a poll: when the frame is longer than `poll`, the poll would always take the turn
        if snapshot_due(dirty || snapshot_error.is_some(), last_snapshot.elapsed()) && !reading {
            // what comes while the snapshot is read (an action's end) is seen by the next
            dirty = false;
            last_snapshot = Instant::now();
            let shared = Arc::clone(&reader);
            reading = true;
            reads.request(move || {
                Read::Snapshot(pace::Owned::read(&shared, |store, _| read::snooze_end(store).and_then(|end| Ok((end, read::tree(store)?)))))
            });
        }
        if last_poll.elapsed() >= poll && !reading {
            last_poll = Instant::now();
            // the Inbox changes with no Event when a snooze ends or the engine stops or starts (`engine-down`, §17); a merge
            // starts and ends with no Event: its marker (hs-d495) is read on each poll
            dirty |= snooze_end.is_some_and(|end| OffsetDateTime::now_utc() >= end);
            let (shared, home) = (Arc::clone(&reader), home.clone());
            reading = true;
            reads.request(move || {
                let (changes, merging) = (
                    pace::Owned::read(&shared, |store, follow| follow.poll_changes(store)),
                    pace::Owned::read(&shared, |store, _| read::open_pulls(store)).map(|open| crate::merge::marker::live(&home, &open)),
                );
                Read::Poll(Polled { changes, engine: model::engine_running(&home), merging })
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
        redraw |= view.error != error || (view.animated() && frame != drawn_frame);
        // the Inbox items' ages are redrawn every second, from `opened_at`
        redraw |= !view.tree.inbox.is_empty() && second != drawn_second;
        if redraw {
            let (drawn, written) = trace.draw(epoch, &log.file, frame, || terminal.draw(|f| view.draw(f, frame)).map(drop));
            drawn?;
            if let Err(why) = written {
                show(log.line(&format!("failed: trace: {why}")), &mut popups, runner);
            }
            (redraw, drawn_frame, drawn_second) = (false, frame, second);
        }
        if !event::poll(pace::wait_now())? {
            continue;
        }
        redraw = true; // a key, a click, a resize, the focus
        let effect = match event::read()? {
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
                        dirty = true;
                        last_snapshot = Instant::now() - SNAPSHOT_EVERY;
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
                dirty = true;
            }
            Effect::Curate(kind, context) => {
                // the checklist popup returns when it closes; a refusal is said in a `text` dialog by the runner
                let runner = Arc::clone(runner);
                popups.submit(move || runner.curate(kind, &context));
                dirty = true;
            }
            Effect::Run(action) => {
                let runner = Arc::clone(runner);
                actions.submit(move || runner.run(&action));
            }
            text @ Effect::Text { .. } => show(text, &mut popups, runner),
            Effect::Steps(steps) => {
                show(said(&mut view, &mut log, runner.run_all(&steps)), &mut popups, runner);
                dirty = true;
                last_snapshot = Instant::now() - SNAPSHOT_EVERY;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_due_on_a_change_after_half_a_second_and_never_with_none() {
        let ms = Duration::from_millis;
        assert!(!snapshot_due(false, ms(0)));
        assert!(!snapshot_due(false, ms(2000)), "no change: no snapshot (tui-F3)");
        assert!(!snapshot_due(false, Duration::from_secs(3600)));
        assert!(!snapshot_due(true, ms(499)), "at most one snapshot per 500 ms");
        assert!(snapshot_due(true, ms(500)));
    }
}
