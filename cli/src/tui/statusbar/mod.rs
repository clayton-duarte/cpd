//! The `statusbar` view (commands.md §13.2, §17): one row across the full width at the bottom of the cockpit.
//! Left, the ⧉ reset-layout, legend and logs buttons, then the note of a failed click; right, the workers
//! indicator `workers N/M`. It has no keys and never takes keyboard focus (the root mouse binding passes a click on
//! with `send-keys -M`, §6.3).
//!
//! It refreshes like the `widgets` view: it looks at the store's highest Event id every [`TICK`] and re-reads the
//! slots when it moved, at most once per [`COALESCE`], and at least every [`REFRESH`]. No subprocess, no `gh`.
//!
//! - [`bar`]: the row's drawing and its clickable items;
//! - [`workers`]: the indicator;
//! - [`layout`]: reset-layout, which the ⧉ button runs and `cpd tui` runs after building a new `main`.

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
use super::logs;
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
const LOGS_BUTTON: &str = " ≡ logs ";

/// The bar's items: the three buttons, then the failure note if any (left), and the workers indicator (right), which
/// gets the full `width` as in the reference (`main.rs` `statusbar_items`).
fn items(workers: &Workers, note: &str, width: u16) -> (Vec<Item>, Vec<Item>) {
    let button = |text: &'static str, action| Item {
        line: Line::from(text).style(Style::new().add_modifier(Modifier::REVERSED)),
        action: Some(action),
    };
    let mut left = vec![
        button(RESET_BUTTON, Action::ResetLayout),
        button(LEGEND_BUTTON, Action::Legend),
        button(LOGS_BUTTON, Action::Logs),
    ];
    if !note.is_empty() {
        left.push(Item { line: Line::from(note.to_owned()), action: None });
    }
    let right = workers.line(width).map(|line| Item { line, action: None }).into_iter().collect();
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
    CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view statusbar failed: {e}."))
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
                        Some((_, Action::Logs)) => logs::open(tmux),
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

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    /// The reference's `the_statusbar_has_the_legend_button_left_and_the_workers_right` (`tui/src/main.rs:1028`).
    #[test]
    fn the_statusbar_has_the_legend_button_left_and_the_workers_right() {
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|f| {
                let (left, right) = items(&Workers { running: 1, limit: 4 }, "", f.area().width);
                hits = bar::draw(f, f.area(), &left, &right);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let left = format!("{RESET_BUTTON} {LEGEND_BUTTON} {LOGS_BUTTON}");
        assert_eq!(text(buf, 0), format!("{left}{}workers 1/4", " ".repeat(40 - left.chars().count() - "workers 1/4".len())));
        assert_eq!(Line::from("⧉").width(), 1);
        assert_eq!(
            hits,
            [
                (Rect::new(0, 0, 2, 1), Action::ResetLayout),
                (Rect::new(3, 0, LEGEND_BUTTON.len() as u16, 1), Action::Legend),
                (Rect::new(14, 0, LOGS_BUTTON.chars().count() as u16, 1), Action::Logs),
            ]
        );
        assert_eq!(buf[(0, 0)].symbol(), "⧉");
        assert!((0..2).all(|x| buf[(x, 0)].modifier.contains(Modifier::REVERSED)));
        assert!(!buf[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert!((3..3 + LEGEND_BUTTON.len() as u16).all(|x| buf[(x, 0)].modifier.contains(Modifier::REVERSED)));
        assert!((14..22).all(|x| buf[(x, 0)].modifier.contains(Modifier::REVERSED)));
    }

    /// The workers get the whole width, not what the buttons leave: the long form is chosen whenever it fits the row.
    #[test]
    fn the_workers_indicator_is_given_the_full_width() {
        let workers = Workers { running: 0, limit: 4 };
        let right = |width| items(&workers, "", width).1.first().map(|i| i.line.width());
        assert_eq!(right(20), Some("workers 0/4".len()));
        assert_eq!(right(11), Some("workers 0/4".len()));
        assert_eq!(right(10), Some("0/4".len()));
        assert_eq!(right(2), None);
    }
}
