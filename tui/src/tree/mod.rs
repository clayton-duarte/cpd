//! The `tree` view (commands.md §13.2, §17; contract §6.2): the cockpit's left
//! column, ported from the spike's Context view (branch `cpd/cpd-ui`:
//! `context.rs`, `sessions.rs`, `cursor.rs`, `actions.rs`, `prompt.rs`,
//! `separator.rs`; the prompt is gone, its questions are dialog popups now) onto the read model.
//!
//! - [`rows`]: Contexts, their Plans (each Plan's row is its Lead's) and the forked Sessions after them (B1), labelled by title and
//!   name, and the archived ones behind `…` rows;
//! - [`view`]: the state, keys and drawing; [`cursor`], [`menu`]: its parts (the right-click menu); [`ask`]: the dialog popups (the dialog kit's) it opens
//!   before a command, with no inline prompt row;
//! - [`actions`]: what it does, as `cpd` commands (`session open`, `plan new`, `session close`, the menu's), and the
//!   legend popup; [`slug`]: the id of a new Context or Plan, from its title;
//! - [`inbox`]: the Inbox section at the column's bottom (header, up to 3 items, the fold widget's `└ expand` / `└ collapse` row) and what a click on an item does:
//!   `session open`, or nothing (the label is an OSC 8 link);
//! - [`super::background`]: a command or the legend popup runs on a thread, one command at a time; the footer says
//!   `running: <command>` meanwhile and then its result, and the loop keeps drawing and reading keys (tui-W7);
//! - [`kinds`]: the Events it follows, in process ([`read::Follow`]), which tell it when to take a new snapshot.
//!
//! It is also the one view that restarts a stopped engine (`cpd watch --ensure`, at most once a minute) and tells the
//! live Managers when that fails ([`view::View::restart_engine`], hs-d318).
//!
//! Keys and clicks are the reference's (commands.md §13.2, `tui/README.md` "Context column" on `main`): arrows
//! or `j`/`k` move the cursor between Sessions (shown after the first arrow key, while the pane has focus);
//! Enter opens the cursor's Session in the chat slot and focuses it; `n` starts a new Plan; `x` closes a
//! Session's pane; `?` opens the legend. After the last Session the cursor goes on into the Inbox's item rows (Enter jumps, `s` snoozes, `d` dismisses; the right-click menu of an item has the same entries). A Context and a `…` group are folds ([`crate::fold`]): only a click on their `└ expand` / `└ collapse` row folds or unfolds them; a click on the title row of a live Context opens its Manager (`context manager`, then `session open`), on a Session opens it, on a `…` title row does nothing; a right click opens the menu.

mod actions;
mod ask;
mod cursor;
mod inbox;
mod kinds;
mod menu;
mod rows;
mod slug;
mod view;

#[cfg(test)]
mod fixture;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
use time::OffsetDateTime;

use super::background::Background;
use super::runner::kept;
use super::{Capture, run_view};
use super::{palette, read};
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::inbox::model;
use cpd_core::output::Output;
use cpd_core::store::Store;
use actions::Runner;
use kinds::KINDS;
use view::{Effect, View};

/// A view takes at most one snapshot per this long, whatever the Event rate (commands.md §17).
const SNAPSHOT_EVERY: Duration = Duration::from_millis(500);
/// How long the loop waits for a key or a click before it looks at the follower and the clock again. The spinner turns
/// every 100 ms (`palette::frame`): while one is on screen ([`View::animated`]) the tree is redrawn that often, else only
/// when something changed.
const TICK: Duration = Duration::from_millis(100);

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
            let view = View::new(read::tree(&mut store)?, false);
            let follow = read::Follow::new(&mut store, KINDS, view.tree.events_cursor)?;
            let poll = Duration::from_millis(u64::from(config.events.poll_ms));
            let runner = Arc::new(Runner::new(home.clone(), &config));
            Ok((store, home, view, (follow, snooze_end), poll, runner))
        },
        |terminal, (mut store, home, view, follow, poll, runner)| {
            Ok(event_loop(terminal, &mut store, &home, view, follow, poll, &runner)?)
        },
    )
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    store: &mut Store,
    home: &CpdHome,
    mut view: View,
    (mut follow, mut snooze_end): (read::Follow, Option<OffsetDateTime>),
    poll: Duration,
    runner: &Arc<Runner>,
) -> std::io::Result<()> {
    let mut dirty = false;
    let mut last_snapshot = Instant::now();
    let mut last_poll = Instant::now();
    // a failed snapshot is retried at the next refresh, with no Event needed; a failed poll (of the Events or of the
    // engine's lock) at the next poll. All are shown (runner::kept): the footer says the snapshot's error, else the
    // follow's, else the lock's.
    let (mut snapshot_error, mut follow_error, mut engine_error) = (None, None, None);
    // what the last draw showed of the clock (the spinner's frame, the second the Inbox items' ages were taken at), and
    // whether anything changed since: the pane is written to only then
    let (mut redraw, mut drawn_frame, mut drawn_second) = (true, palette::frame(SystemTime::now()), unix_second());
    // the commands and the popup run off this thread (tui-W7): the loop goes on drawing, reading keys and the follower
    // meanwhile. A popup returns when it closes, so it does not hold up the commands.
    let mut actions = Background::<Result<String, String>>::new();
    let mut popups = Background::<Result<(), String>>::new();
    loop {
        let error = view.error.clone();
        while let Some(result) = actions.finished() {
            view.done(result);
            redraw = true;
            // the tree shows the result on this loop's snapshot
            dirty = true;
            last_snapshot = Instant::now() - SNAPSHOT_EVERY;
        }
        while let Some(result) = popups.finished() {
            // only a failure is said
            if let Err(why) = result {
                view.done(Err(why));
                redraw = true;
            }
        }
        if last_poll.elapsed() >= poll {
            last_poll = Instant::now();
            dirty |= kept(follow.poll_changes(store), &mut follow_error).is_some_and(|changes| changes.any());
            // the Inbox changes with no Event when a snooze ends or the engine stops or starts (`engine-down`, §17)
            dirty |= snooze_end.is_some_and(|end| OffsetDateTime::now_utc() >= end);
            dirty |= kept(model::engine_running(home), &mut engine_error).is_some_and(|on| on != view.tree.engine_running);
            view.error = snapshot_error.clone().or_else(|| follow_error.clone()).or_else(|| engine_error.clone());
        }
        if snapshot_due(dirty || snapshot_error.is_some(), last_snapshot.elapsed()) {
            let next = read::snooze_end(store).and_then(|end| Ok((end, read::tree(store)?)));
            if let Some((end, tree)) = kept(next, &mut snapshot_error) {
                snooze_end = end;
                let t = &view.tree;
                if (&tree.contexts, &tree.inbox, tree.engine_running) != (&t.contexts, &t.inbox, t.engine_running) {
                    view.refresh(tree);
                    redraw = true;
                }
            }
            dirty = false;
            last_snapshot = Instant::now();
            view.error = snapshot_error.clone().or_else(|| follow_error.clone()).or_else(|| engine_error.clone());
        }
        // A stopped engine restarts itself, at most once a minute (commands.md §13.2, hs-d318): this view only, not every pane.
        if let Some(line) = view.restart_engine(Instant::now(), &mut |action| actions::call(action), &mut || runner.live_managers()) {
            eprintln!("{line}");
            terminal.clear()?; // the line was written over the screen this view owns
            redraw = true;
        }
        let (frame, second) = (palette::frame(SystemTime::now()), unix_second());
        redraw |= view.error != error || (view.animated() && frame != drawn_frame);
        // the Inbox items' ages are redrawn every second, from `opened_at`
        redraw |= !view.tree.inbox.is_empty() && second != drawn_second;
        if redraw {
            terminal.draw(|f| view.draw(f, frame))?;
            (redraw, drawn_frame, drawn_second) = (false, frame, second);
        }
        if !event::poll(TICK)? {
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
                terminal.draw(|f| view.draw(f, frame))?;
                match runner.ask(question) {
                    Ok(None) => {}
                    Ok(Some(action)) => {
                        view.done(runner.run(&action));
                        // the tree shows the result on this loop's snapshot
                        dirty = true;
                        last_snapshot = Instant::now() - SNAPSHOT_EVERY;
                    }
                    Err(why) => view.done(Err(why)),
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
            Effect::Run(action) => {
                view.done(Ok(format!("running: {}", action.argv().join(" "))));
                let runner = Arc::clone(runner);
                actions.submit(move || runner.run(&action));
            }
            Effect::Steps(steps) => {
                view.done(runner.run_all(&steps));
                dirty = true;
                last_snapshot = Instant::now() - SNAPSHOT_EVERY;
            }
        }
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

    #[test]
    fn the_loop_redraws_at_least_every_frame() {
        assert!(TICK <= Duration::from_millis(100));
    }
}
