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
//! - [`follow`]: `cpd events follow`, which tells it when to take a new snapshot.
//!
//! Keys and clicks are the reference's (commands.md §13.2, `tui/README.md` "Context column" on `main`): arrows
//! or `j`/`k` move the cursor between Sessions (shown after the first arrow key, while the pane has focus);
//! Enter opens the cursor's Session in the chat slot and focuses it; `n` starts a new Plan; `x` closes a
//! Session's pane; `?` opens the legend. A click on a Context's fold glyph collapses or expands it, elsewhere on a live Context row opens its Manager (`context manager`, then `session open`), on a Session opens it, on a `…` row expands the archived rows
//! (muted, dotted guides) and on their dotted line collapses them; a right click opens the menu.

mod actions;
mod cursor;
mod follow;
mod menu;
mod prompt;
mod rows;
mod slug;
mod view;

#[cfg(test)]
mod fixture;

use std::time::{Duration, Instant, SystemTime};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event};
use ratatui::crossterm::execute;

use super::{palette, read};
use super::{not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::store::Store;
use actions::Runner;
use follow::{Follower, Message};
use view::{Effect, View};

/// A view takes at most one snapshot per this long, whatever the Event rate (commands.md §17).
const SNAPSHOT_EVERY: Duration = Duration::from_millis(500);
/// A snapshot is taken at least this often even with no Event: a pull request's checks can change with none (§17).
const SNAPSHOT_AT_LEAST_EVERY: Duration = Duration::from_secs(2);
/// How long the loop waits for a key or a click before it looks at the follower again and redraws. The spinner turns
/// every 250 ms (`palette::frame`), so the tree is redrawn at least that often.
const TICK: Duration = Duration::from_millis(100);

/// A new snapshot is due: an Event came and `since` the last one reaches [`SNAPSHOT_EVERY`], or `since` reaches
/// [`SNAPSHOT_AT_LEAST_EVERY`].
fn snapshot_due(dirty: bool, since: Duration) -> bool {
    (dirty && since >= SNAPSHOT_EVERY) || since >= SNAPSHOT_AT_LEAST_EVERY
}

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view tree"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let view = View::new(read::tree(&mut store)?, false);
    let runner = Runner::new(&config);
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableFocusChange);
    let result = event_loop(&mut terminal, &mut store, view, &runner);
    let _ = execute!(std::io::stdout(), DisableFocusChange, DisableMouseCapture);
    ratatui::restore();
    result.map(|()| Output::Owned).map_err(|e| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view tree failed: {e}."))
            .with("command", "tui view tree")
    })
}

fn event_loop(terminal: &mut DefaultTerminal, store: &mut Store, mut view: View, runner: &Runner) -> std::io::Result<()> {
    let mut follower = Follower::start(view.tree.events_cursor).ok();
    let mut dirty = false;
    let mut last_snapshot = Instant::now();
    loop {
        if let Some(f) = &follower {
            while let Ok(message) = f.messages.try_recv() {
                match message {
                    Message::Event => dirty = true,
                    Message::Ended => {
                        // take a new snapshot and follow again from its cursor
                        follower = None;
                        dirty = true;
                        break;
                    }
                }
            }
        }
        if snapshot_due(dirty, last_snapshot.elapsed()) {
            match read::tree(store) {
                Ok(tree) => view.refresh(tree),
                Err(e) => view.note = e.message().to_owned(),
            }
            dirty = false;
            last_snapshot = Instant::now();
            if follower.is_none() {
                follower = Follower::start(view.tree.events_cursor).ok();
            }
        }
        let frame = palette::frame(SystemTime::now());
        terminal.draw(|f| view.draw(f, frame))?;
        if !event::poll(TICK)? {
            continue;
        }
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
                // `display-popup` returns when the popup closes; only a failure is said
                if let Err(why) = runner.legend() {
                    view.done(Err(why));
                }
            }
            Effect::Run(action) => {
                view.done(runner.run(&action));
                // the tree shows the result on this loop's snapshot
                dirty = true;
                last_snapshot = Instant::now() - SNAPSHOT_EVERY;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_due_on_an_event_after_half_a_second_and_with_none_after_two() {
        let ms = Duration::from_millis;
        assert!(!snapshot_due(false, ms(0)));
        assert!(!snapshot_due(false, ms(1999)), "no Event: not yet");
        assert!(snapshot_due(false, ms(2000)), "no Event, yet pull request checks may have changed");
        assert!(!snapshot_due(true, ms(499)), "at most one snapshot per 500 ms");
        assert!(snapshot_due(true, ms(500)));
    }

    #[test]
    fn the_loop_redraws_at_least_every_quarter_second() {
        assert!(TICK <= Duration::from_millis(250));
    }
}
