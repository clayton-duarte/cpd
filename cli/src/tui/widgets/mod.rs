//! The `widgets` view (commands.md §13.2, §17): the Dispatch column, the
//! machine-wide right column of the cockpit. It takes one snapshot of the read
//! model ([`read::widgets`]), then follows the Events: it looks at the highest
//! Event id every [`TICK`] and re-reads when it moved, at most once per
//! [`COALESCE`]; the engine's pid and the clocks are re-read at least every
//! [`REFRESH`] since a crash is no Event. It makes no write of its own.
//!
//! - [`column`]: the drawing of the engine, slots, PRs, Escalations and Events;
//! - [`legend`]: the key legend, `cpd2 tui view legend`, which `?` opens in a
//!   tmux popup (the column is narrower than the legend);
//! - the colours and icons are [`crate::tui::palette`]'s.

mod column;
pub mod legend;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use time::OffsetDateTime;

use super::read::{self, Widgets};
use super::{not_a_terminal, on_a_terminal};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::highest_id;
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::output::Output;
use crate::store::Store;

pub use column::{HINT, draw};

/// How many Events the column asks for: more than any pane shows.
pub const RECENT: u32 = 50;
/// How often the view looks at the Events and at keys.
const TICK: Duration = Duration::from_millis(200);
/// At most one snapshot per this long, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);
/// A snapshot at least this often: the engine's process can die without an Event.
const REFRESH: Duration = Duration::from_secs(2);

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view widgets"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let first = read::widgets(&mut store, config.concurrency, RECENT)?;
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let result = show(&mut terminal, &mut store, config.concurrency, &tmux, first);
    ratatui::restore();
    result.map(|()| Output::Owned)
}

/// Draws and follows until `q`. Reading failures end the view with their error; a failed popup is shown
/// in the hint's row.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    concurrency: u32,
    tmux: &Tmux,
    mut widgets: Widgets,
) -> Result<(), CpdError> {
    let terminal_failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd2 tui view widgets failed: {e}."))
            .with("command", "tui view widgets")
    };
    let mut read_at = Instant::now();
    let mut note: Option<String> = None;
    loop {
        terminal
            .draw(|f| draw(f, f.area(), &widgets, OffsetDateTime::now_utc(), note.as_deref()))
            .map_err(terminal_failed)?;
        if event::poll(TICK).map_err(terminal_failed)? {
            if let Event::Key(key) = event::read().map_err(terminal_failed)? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('?') => note = open_legend(tmux).err().map(|e| e.to_string()),
                        _ => {}
                    }
                }
            }
        }
        if read_at.elapsed() >= COALESCE {
            let moved = store.read(highest_id)? != widgets.events_cursor;
            if moved || read_at.elapsed() >= REFRESH {
                widgets = read::widgets(store, concurrency, RECENT)?;
                read_at = Instant::now();
            }
        }
    }
}

/// `?`: the legend in a tmux popup over the cockpit, sized to it and at most the client's size.
pub(crate) fn open_legend(tmux: &Tmux) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&legend::popup_args(client)).map(drop)
}

/// The size `(columns, rows)` of the most recently active client in `list-clients`'
/// `#{client_activity} #{client_width} #{client_height}` lines.
fn newest_client(listing: &str) -> Option<(u16, u16)> {
    listing
        .lines()
        .filter_map(|line| {
            let n: Vec<u64> = line.split(' ').filter_map(|x| x.parse().ok()).collect();
            Some((*n.first()?, u16::try_from(*n.get(1)?).ok()?, u16::try_from(*n.get(2)?).ok()?))
        })
        .max()
        .map(|(_, w, h)| (w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_legend_popup_is_sized_by_the_most_recently_active_client() {
        assert_eq!(newest_client("100 80 24\n300 215 62\n200 120 30\n"), Some((215, 62)));
        assert_eq!(newest_client(""), None);
        assert_eq!(newest_client("junk\n"), None);
    }
}
