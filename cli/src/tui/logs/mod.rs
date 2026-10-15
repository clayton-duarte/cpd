//! The `logs` view (commands.md §13.2, §17; R36): the Event log, in a tmux popup opened by the StatusBar's
//! ` ≡ logs ` button. It takes the latest [`RECENT`] Events, then looks at the store's highest Event id every
//! `events.poll_ms` and appends what is new. ↑↓, j/k and PgUp/PgDn scroll; `q` or Esc closes it. It makes no
//! write of its own.
//!
//! - [`view`]: the state and the drawing;
//! - [`open`] and [`popup_args`]: the popup, which the button runs.

mod view;

use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

pub use self::view::{Log, draw};
use super::read;
use super::widgets::{legend, newest_client};
use super::{not_a_terminal, on_a_terminal};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::highest_id;
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::output::Output;
use crate::store::Store;

/// How many Events the view starts with (the `events list` default).
pub const RECENT: u32 = 500;
/// The popup's share of the client, in percent, at most.
const SHARE: u16 = 80;

/// The tmux arguments that open the log in a popup: sized like the legend's, at most [`SHARE`] % of the client's
/// size `(columns, rows)`.
pub fn popup_args(client: (u16, u16)) -> Vec<String> {
    let cap = |n: u16| (u32::from(n) * u32::from(SHARE) / 100) as u16;
    let mut args = legend::popup_args((cap(client.0), cap(client.1)));
    args[3] = " logs ".to_owned();
    *args.last_mut().expect("popup_args ends with the command") = "cpd tui view logs".to_owned();
    args
}

/// The StatusBar's ` ≡ logs ` click: the log in a tmux popup over the cockpit.
pub(crate) fn open(tmux: &Tmux) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&popup_args(client)).map(drop)
}

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view logs"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let first = read::logs(&mut store, RECENT)?;
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let poll = Duration::from_millis(u64::from(config.events.poll_ms));
    let result = show(&mut terminal, &mut store, poll, first);
    ratatui::restore();
    result.map(|()| Output::Owned)
}

/// Draws and follows until `q` or Esc.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    poll: Duration,
    first: read::Logs,
) -> Result<(), CpdError> {
    let terminal_failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view logs failed: {e}."))
            .with("command", "tui view logs")
    };
    let mut cursor = first.events_cursor;
    let mut log = Log::new(first.events);
    loop {
        terminal
            .draw(|f| {
                log.fit(f.area().height);
                draw(f, f.area(), &log);
            })
            .map_err(terminal_failed)?;
        if event::poll(poll).map_err(terminal_failed)? {
            if let Event::Key(key) = event::read().map_err(terminal_failed)? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Up | KeyCode::Char('k') => log.scroll_up(1),
                        KeyCode::Down | KeyCode::Char('j') => log.scroll_down(1),
                        KeyCode::PageUp => log.scroll_up(log.page()),
                        KeyCode::PageDown => log.scroll_down(log.page()),
                        _ => {}
                    }
                }
            }
        }
        if store.read(highest_id)? != cursor {
            let next = read::logs_after(store, cursor)?;
            cursor = next.events_cursor;
            log.push(next.events);
        }
    }
}
