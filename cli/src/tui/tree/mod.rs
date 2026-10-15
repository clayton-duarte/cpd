//! The `tree` view (commands.md §13.2, §17; contract §6.2): the cockpit's left
//! column, ported from the spike's Context view (branch `cpd/cpd-ui`:
//! `context.rs`, `sessions.rs`, `cursor.rs`, `actions.rs`, `prompt.rs`,
//! `separator.rs`) onto the read model.
//!
//! - [`rows`]: Contexts, their Plans (each Plan's row is its Lead's) and a Plan's side forks;
//! - [`view`]: the state, keys and drawing; [`cursor`], [`prompt`], [`separator`]: its parts;
//! - [`actions`]: what it does, as `cpd2` commands (`session open`, `session park`, `session fork`,
//!   `session rename`, `plan new`);
//! - [`follow`]: `cpd2 events follow`, which tells it when to take a new snapshot.
//!
//! Keys: arrows or `j`/`k` move the cursor (shown after the first arrow key, while the pane has focus);
//! Enter opens the Lead of the row in the chat slot (on a Context it collapses or expands it); Space
//! collapses or expands a Context or a Plan with forks; `p` parks the row's Session; `f` forks it
//! (a side session of its Plan); `r` renames it; `n` starts a new Plan in the row's Context; `q` ends the
//! view (the cockpit respawns the pane). A click on a Context collapses or expands it, on a Plan or fork opens it.

mod actions;
mod cursor;
mod follow;
mod prompt;
mod rows;
mod separator;
mod view;

#[cfg(test)]
mod fixture;

use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event};
use ratatui::crossterm::execute;

use super::read;
use super::{not_a_terminal, on_a_terminal};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::store::Store;
use follow::{Follower, Message};
use view::{Effect, View};

/// A view takes at most one snapshot per this long, whatever the Event rate (commands.md §17).
const SNAPSHOT_EVERY: Duration = Duration::from_millis(500);
/// How long the loop waits for a key or a click before it looks at the follower again.
const TICK: Duration = Duration::from_millis(100);

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view tree"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let view = View::new(read::tree(&mut store)?, false);
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableFocusChange);
    let result = event_loop(&mut terminal, &mut store, view);
    let _ = execute!(std::io::stdout(), DisableFocusChange, DisableMouseCapture);
    ratatui::restore();
    result.map(|()| Output::Owned).map_err(|e| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd2 tui view tree failed: {e}."))
            .with("command", "tui view tree")
    })
}

fn event_loop(terminal: &mut DefaultTerminal, store: &mut Store, mut view: View) -> std::io::Result<()> {
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
        if dirty && last_snapshot.elapsed() >= SNAPSHOT_EVERY {
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
        terminal.draw(|f| view.draw(f))?;
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
            Effect::Quit => return Ok(()),
            Effect::Run(action) => {
                view.note = format!("running cpd2 {}…", action.argv().join(" "));
                terminal.draw(|f| view.draw(f))?;
                view.done(actions::run(&action));
                // the tree shows the result on this loop's snapshot
                dirty = true;
                last_snapshot = Instant::now() - SNAPSHOT_EVERY;
            }
        }
    }
}
