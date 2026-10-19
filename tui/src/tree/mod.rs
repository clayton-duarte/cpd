//! The `tree` view (commands.md §13.2, §17; contract §6.2): the cockpit's left
//! column, ported from the spike's Context view (branch `cpd/cpd-ui`:
//! `context.rs`, `sessions.rs`, `cursor.rs`, `actions.rs`, `prompt.rs`,
//! `separator.rs`) onto the read model.
//!
//! - [`rows`]: Contexts, their Plans (each Plan's row is its Lead's) and the forked Sessions after them (B1), labelled by title and
//!   name, and the archived ones behind `…` rows;
//! - [`view`]: the state, keys and drawing; [`cursor`], [`prompt`], [`menu`]: its parts (the right-click menu);
//! - [`actions`]: what it does, as `cpd` commands (`session open`, `plan new`, `session close`, the menu's), and the
//!   legend popup; [`slug`]: the id of a new Context or Plan, from its title;
//! - [`super::background`]: a command or the legend popup runs on a thread, one command at a time; the footer says
//!   `running: <command>` meanwhile and then its result, and the loop keeps drawing and reading keys (tui-W7);
//! - [`kinds`]: the Events it follows, in process ([`read::Follow`]), which tell it when to take a new snapshot.
//!
//! Keys and clicks are the reference's (commands.md §13.2, `tui/README.md` "Context column" on `main`): arrows
//! or `j`/`k` move the cursor between Sessions (shown after the first arrow key, while the pane has focus);
//! Enter opens the cursor's Session in the chat slot and focuses it; `n` starts a new Plan; `x` closes a
//! Session's pane; `?` opens the legend. A click on a Context's fold glyph collapses or expands it, elsewhere on a live Context row opens its Manager (`context manager`, then `session open`), on a Session opens it, on a `…` row expands the archived rows
//! (muted, dotted guides) and on their dotted line collapses them; a right click opens the menu.

mod actions;
mod cursor;
mod kinds;
mod menu;
mod prompt;
mod rows;
mod slug;
mod view;

#[cfg(test)]
mod fixture;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};

use super::background::Background;
use super::runner::kept;
use super::{Capture, run_view};
use super::{palette, read};
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::store::Store;
use actions::Runner;
use kinds::KINDS;
use view::{Effect, View};

/// A view takes at most one snapshot per this long, whatever the Event rate (commands.md §17).
const SNAPSHOT_EVERY: Duration = Duration::from_millis(500);
/// How long the loop waits for a key or a click before it looks at the follower and the clock again. The spinner turns
/// every 250 ms (`palette::frame`): while one is on screen ([`View::animated`]) the tree is redrawn that often, else only
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
            let view = View::new(read::tree(&mut store)?, false);
            let follow = read::Follow::new(&mut store, KINDS, view.tree.events_cursor)?;
            let poll = Duration::from_millis(u64::from(config.events.poll_ms));
            Ok((store, view, follow, poll, Arc::new(Runner::new(&config))))
        },
        |terminal, (mut store, view, follow, poll, runner)| {
            Ok(event_loop(terminal, &mut store, view, follow, poll, &runner)?)
        },
    )
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    store: &mut Store,
    mut view: View,
    mut follow: read::Follow,
    poll: Duration,
    runner: &Arc<Runner>,
) -> std::io::Result<()> {
    let mut dirty = false;
    let mut last_snapshot = Instant::now();
    let mut last_poll = Instant::now();
    // a failed snapshot is retried at the next refresh, with no Event needed; a failed poll at the next poll. Both are
    // shown (runner::kept): the footer says the snapshot's error, else the follow's.
    let (mut snapshot_error, mut follow_error) = (None, None);
    // what the last draw showed of the clock, and whether anything changed since: the pane is written to only then
    let (mut redraw, mut drawn_frame) = (true, palette::frame(SystemTime::now()));
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
            view.error = snapshot_error.clone().or_else(|| follow_error.clone());
        }
        if snapshot_due(dirty || snapshot_error.is_some(), last_snapshot.elapsed()) {
            if let Some(tree) = kept(read::tree(store), &mut snapshot_error) {
                if tree.contexts != view.tree.contexts {
                    view.refresh(tree);
                    redraw = true;
                }
            }
            dirty = false;
            last_snapshot = Instant::now();
            view.error = snapshot_error.clone().or_else(|| follow_error.clone());
        }
        let frame = palette::frame(SystemTime::now());
        redraw |= view.error != error || (view.animated() && frame != drawn_frame);
        if redraw {
            terminal.draw(|f| view.draw(f, frame))?;
            (redraw, drawn_frame) = (false, frame);
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
            Effect::Legend => {
                let runner = Arc::clone(runner);
                popups.submit(move || runner.legend());
            }
            Effect::Run(action) => {
                view.done(Ok(format!("running: {}", action.argv().join(" "))));
                let runner = Arc::clone(runner);
                actions.submit(move || runner.run(&action));
            }
        }
    }
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
    fn the_loop_redraws_at_least_every_quarter_second() {
        assert!(TICK <= Duration::from_millis(250));
    }
}
