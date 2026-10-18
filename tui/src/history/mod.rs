//! The `history` view (commands.md §13.2, §17): an archived Session's pi file,
//! read-only, in the chat slot. `cpd session history <id>` puts it there.
//!
//! `cpd tui view history <id>` reads the Session's `file` from the store and
//! draws it once with the Worker view's renderer
//! ([`crate::worker::render`]); nothing is followed or re-read. An unknown
//! Session is `session_not_found`, a Session with no readable file
//! `file_unreadable`, both before anything is drawn. The top line is
//! `<name> (archived, read-only)`.
//!
//! Keys: Up/`k`, Down/`j`, PageUp, PageDown and the mouse wheel scroll; End or
//! `G` follows the end again (the view starts there); `q` or Esc leaves the
//! history for the chat slot's placeholder text.

mod view;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::DefaultTerminal;
use rusqlite::OptionalExtension;

use super::{chat, not_a_terminal, on_a_terminal};
use crate::worker::render::render;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::output::Output;
use cpd_core::store::Store;
use view::View;

/// `cpd tui view history <id>`.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// The Session whose pi file to show
    #[arg(value_name = "id")]
    id: SessionId,
}

/// What the view shows: the Session's name and whether it is archived, and its file's text.
struct History {
    name: String,
    archived: bool,
    text: String,
}

pub(super) fn run(args: Args) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    // `session_not_found` and `file_unreadable` refuse before anything is drawn.
    let history = load(&mut store, &args.id)?;
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view history"));
    }
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = event_loop(&mut terminal, history);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.map_err(|e| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view history failed: {e}."))
            .with("command", "tui view history")
    })?;
    // `q` or Esc: the pane shows the placeholder text, as the chat slot's placeholder does.
    chat::run()
}

fn load(store: &mut Store, id: &SessionId) -> Result<History, CpdError> {
    let (name, file, archived): (String, Option<String>, bool) = store
        .read(|conn| {
            Ok(conn
                .query_row("SELECT name, file, archived_at IS NOT NULL FROM sessions WHERE id = ?1", [id.as_str()], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?)
        })?
        .ok_or_else(|| CpdError::new(ErrorCode::SessionNotFound, format!("No Session {id}.")).with("session", id.as_str()))?;
    let unreadable = |why: String| CpdError::new(ErrorCode::FileUnreadable, why).with("session", id.as_str());
    let file = file.ok_or_else(|| unreadable(format!("Session {id} has no session file.")))?;
    let text = std::fs::read_to_string(&file).map_err(|e| unreadable(format!("Cannot read {file} as UTF-8 text: {e}.")))?;
    Ok(History { name, archived, text })
}

fn event_loop(terminal: &mut DefaultTerminal, history: History) -> std::io::Result<()> {
    let mut view = View::new(&history.name, history.archived);
    view.set_lines(render(&history.text));
    loop {
        terminal.draw(|f| view.draw(f))?;
        match event::read()? {
            Event::Key(key) => {
                if view.key(key) {
                    return Ok(());
                }
            }
            Event::Mouse(m) => view.mouse(m),
            _ => {}
        }
    }
}
