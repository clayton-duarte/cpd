//! The `statusbar` view (commands.md §13.2, §17): one row across the full width at the bottom of the cockpit.
//! Left, the ⧉ reset-layout button and the legend button, then the note of a failed click; right, the workers
//! indicator `workers N/M`. It has no keys and never takes keyboard focus (the root mouse binding passes a click on
//! with `send-keys -M`, §6.3).
//!
//! It refreshes like the `widgets` view: it looks at the store's highest Event id every [`TICK`] and re-reads the
//! slots when it moved, at most once per [`COALESCE`], and at least every [`REFRESH`]. No subprocess, no `gh`.
//!
//! - [`bar`]: the row's drawing and its clickable items;
//! - [`workers`]: the indicator;
//! - [`layout`]: reset-layout, which the ⧉ button runs and `cpd2 tui` runs after building a new `main`.

mod bar;
pub(crate) mod layout;
mod workers;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use self::bar::{Action, Item};
use self::workers::Workers;
use super::read;
use super::widgets::open_legend;
use super::{not_a_terminal, on_a_terminal};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::highest_id;
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::output::Output;
use crate::store::Store;

/// How often the view looks at the Events and at the mouse.
const TICK: Duration = Duration::from_millis(200);
/// At most one read per this long, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);
/// A read at least this often.
const REFRESH: Duration = Duration::from_secs(2);

/// The reset button: `⧉` is one cell wide, and the space is part of the button.
const RESET_BUTTON: &str = "⧉ ";
const LEGEND_BUTTON: &str = " ? legend ";

/// The bar's items: the two buttons, then the failure note if any (left), and the workers indicator if it fits
/// (right).
fn items(workers: &Workers, note: &str, width: u16) -> (Vec<Item>, Vec<Item>) {
    let button = |text: &'static str, action| Item {
        line: Line::from(text).style(Style::new().add_modifier(Modifier::REVERSED)),
        action: Some(action),
    };
    let mut left = vec![button(RESET_BUTTON, Action::ResetLayout), button(LEGEND_BUTTON, Action::Legend)];
    if !note.is_empty() {
        left.push(Item { line: Line::from(note.to_owned()), action: None });
    }
    // what is left of the row for the right item: the left items, and a column of spacing after each and before it
    let taken = left.iter().map(|i| i.line.width()).sum::<usize>() + left.len() + 1;
    let room = u16::try_from(usize::from(width).saturating_sub(taken)).unwrap_or(u16::MAX);
    let right = workers.line(room).map(|line| Item { line, action: None }).into_iter().collect();
    (left, right)
}

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view statusbar"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let first = read::widgets(&mut store, config.concurrency, 0)?;
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.map_err(terminal_failed).and_then(|()| {
        show(&mut terminal, &mut store, &config, &tmux, Workers::of(&first.slots), first.events_cursor)
    });
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.map(|()| Output::Owned)
}

fn terminal_failed(e: std::io::Error) -> CpdError {
    CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd2 tui view statusbar failed: {e}."))
        .with("command", "tui view statusbar")
}

/// Draws, re-reads and takes clicks until the terminal fails. A failed click becomes the note.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    config: &Config,
    tmux: &Tmux,
    mut workers: Workers,
    mut cursor: i64,
) -> Result<(), CpdError> {
    let mut clickable: Vec<(Rect, Action)> = Vec::new();
    let mut note = String::new();
    let mut read_at = Instant::now();
    loop {
        terminal
            .draw(|f| {
                let (left, right) = items(&workers, &note, f.area().width);
                clickable = bar::draw(f, f.area(), &left, &right);
            })
            .map_err(terminal_failed)?;
        if event::poll(TICK).map_err(terminal_failed)? {
            if let Event::Mouse(m) = event::read().map_err(terminal_failed)? {
                if m.kind == MouseEventKind::Down(MouseButton::Left) {
                    note.clear();
                    let at = Position::new(m.column, m.row);
                    let result = match clickable.iter().find(|(r, _)| r.contains(at)) {
                        Some((_, Action::Legend)) => open_legend(tmux),
                        Some((_, Action::ResetLayout)) => layout::reset(tmux, &config.tmux.cockpit_session),
                        None => Ok(()),
                    };
                    if let Err(e) = result {
                        note = e.message().to_owned();
                    }
                }
            }
        }
        if read_at.elapsed() >= COALESCE {
            let moved = store.read(highest_id)? != cursor;
            if moved || read_at.elapsed() >= REFRESH {
                let next = read::widgets(store, config.concurrency, 0)?;
                (workers, cursor) = (Workers::of(&next.slots), next.events_cursor);
                read_at = Instant::now();
            }
        }
    }
}
