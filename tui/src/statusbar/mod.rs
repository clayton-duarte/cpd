//! The `statusbar` view (commands.md §13.2, §17): one row across the full width at the bottom of the cockpit.
//! Left, the ⧉ reset-layout, ? legend and ▤ logs buttons, then the note of a failed click; right, the Inbox counter
//! `inbox <n>` and the workers indicator `workers N/M`. It has no keys and never takes keyboard focus (the root mouse binding passes a click on
//! with `send-keys -M`, §6.3).
//!
//! It refreshes like the `widgets` view: it looks at the store's highest Event id every [`TICK`] and re-reads the
//! slots when it moved, at most once per [`COALESCE`], and at least every [`REFRESH`]. No subprocess, no `gh`.
//!
//! - [`bar`]: the row's drawing and its clickable items;
//! - [`inbox`]: the Inbox counter;
//! - [`workers`]: the indicator;
//! - [`layout`]: reset-layout, which the ⧉ button runs and `cpd tui` runs after building a new `main`.

mod bar;
mod inbox;
pub(crate) mod layout;
mod workers;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::Line;

use self::bar::{Action, Item};
use self::inbox::Inbox;
use self::workers::Workers;
use super::logs;
use super::palette::{self, Meaning};
use super::read;
use super::widgets::open_legend;
use super::{not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::highest_id;
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::store::Store;

/// How often the view looks at the Events and at the mouse.
const TICK: Duration = Duration::from_millis(200);
/// At most one read per this long, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);
/// A read at least this often.
const REFRESH: Duration = Duration::from_secs(2);

/// The buttons: one glyph, one cell each.
const RESET_BUTTON: &str = "⧉";
const LEGEND_BUTTON: &str = "?";
const LOGS_BUTTON: &str = "▤";

/// The bar's items: the three buttons, then the failure note if any (left), and the Inbox counter and the workers
/// indicator (right). The workers indicator gets the full `width` as in the reference (`main.rs` `statusbar_items`);
/// the counter is dropped before it, when the row cannot hold both and the left items.
fn items(inbox: &Inbox, workers: &Workers, note: &str, width: u16) -> (Vec<Item>, Vec<Item>) {
    let button = |text: &'static str, action| Item {
        line: Line::from(text).style(palette::fg(Meaning::Nothing).add_modifier(Modifier::REVERSED)),
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
    let mut right: Vec<Item> = workers.line(width).map(|line| Item { line, action: None }).into_iter().collect();
    let used = |items: &[Item]| items.iter().map(|i| i.line.width()).sum::<usize>();
    let counter = inbox.line();
    // each item is followed by one column of spacing, the expanding middle included
    let needed = used(&left) + used(&right) + counter.width() + left.len() + right.len() + 1;
    if !right.is_empty() && needed <= usize::from(width) {
        right.insert(0, Item { line: counter, action: None });
    }
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
    let inbox = Inbox::read(&mut store)?;
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.map_err(terminal_failed).and_then(|()| {
        show(&mut terminal, &mut store, &config, &tmux, (inbox, Workers::of(&first.slots)), first.events_cursor)
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
    (mut inbox, mut workers): (Inbox, Workers),
    mut cursor: i64,
) -> Result<(), CpdError> {
    let mut clickable: Vec<(Rect, Action)> = Vec::new();
    let mut note = String::new();
    let mut read_at = Instant::now();
    loop {
        terminal
            .draw(|f| {
                let (left, right) = items(&inbox, &workers, &note, f.area().width);
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
                (inbox, workers, cursor) = (Inbox::read(store)?, Workers::of(&next.slots), next.events_cursor);
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
    use crate::read::Dot;

    use cpd_core::inbox::model::Urgency;

    use super::*;

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    const CALM: Inbox = Inbox { count: 0, urgency: None };

    /// Each button is one grey reversed cell, one space apart; the workers end at the right edge, the Inbox counter
    /// before them.
    #[test]
    fn the_statusbar_has_the_glyph_buttons_left_and_the_inbox_and_workers_right() {
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        let mut hits = Vec::new();
        let inbox = Inbox { count: 3, urgency: Some(Urgency::Red) };
        terminal
            .draw(|f| {
                let (left, right) = items(&inbox, &Workers { running: 1, limit: 4 }, "", f.area().width);
                hits = bar::draw(f, f.area(), &left, &right);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let tail = "inbox 3 workers 1/4";
        assert_eq!(text(buf, 0), format!("⧉ ? ▤{}{tail}", " ".repeat(40 - 5 - tail.len())));
        assert_eq!(Line::from("⧉").width(), 1);
        assert_eq!(
            hits,
            [
                (Rect::new(0, 0, 1, 1), Action::ResetLayout),
                (Rect::new(2, 0, 1, 1), Action::Legend),
                (Rect::new(4, 0, 1, 1), Action::Logs),
            ]
        );
        for x in [0, 2, 4] {
            let cell = &buf[(x, 0)];
            assert_eq!(cell.fg, palette::fg(Meaning::Nothing).fg.unwrap(), "x={x}");
            assert!(cell.modifier.contains(Modifier::REVERSED), "x={x}");
        }
        for x in [1, 3, 5] {
            assert!(!buf[(x, 0)].modifier.contains(Modifier::REVERSED), "x={x}");
        }
        let at = 40 - tail.len() as u16;
        for x in at..at + "inbox 3".len() as u16 {
            assert_eq!(buf[(x, 0)].fg, palette::dot_span(Dot::Red, 0, false).style.fg.unwrap(), "x={x}: the counter is red");
        }
    }

    /// With no visible item the counter is a dim `inbox 0`, and no colour.
    #[test]
    fn no_item_is_a_dim_inbox_zero() {
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|f| {
                let (left, right) = items(&CALM, &Workers { running: 1, limit: 4 }, "", f.area().width);
                bar::draw(f, f.area(), &left, &right);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(text(buf, 0).ends_with("inbox 0 workers 1/4"), "{:?}", text(buf, 0));
        let x = 40 - "inbox 0 workers 1/4".len() as u16;
        assert!(buf[(x, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(buf[(x, 0)].fg, buf[(10, 0)].fg, "no colour: that of the blank middle");
    }

    /// The workers get the whole width, not what the buttons leave: the long form is chosen whenever it fits the row.
    #[test]
    fn the_workers_indicator_is_given_the_full_width() {
        let workers = Workers { running: 0, limit: 4 };
        let right = |width| items(&CALM, &workers, "", width).1.last().map(|i| i.line.width());
        assert_eq!(right(20), Some("workers 0/4".len()));
        assert_eq!(right(11), Some("workers 0/4".len()));
        assert_eq!(right(10), Some("0/4".len()));
        assert_eq!(right(2), None);
    }

    /// The counter is dropped before the workers: only when the row holds the buttons, both and the spacing.
    #[test]
    fn the_inbox_counter_is_dropped_before_the_workers_indicator() {
        let workers = Workers { running: 0, limit: 4 };
        let n = |width| items(&CALM, &workers, "", width).1.len();
        // 3 (buttons) + 7 (`inbox 0`) + 11 (`workers 0/4`) + 5 spacing
        assert_eq!((n(26), n(25), n(11)), (2, 1, 1));
        assert_eq!(n(2), 0, "no workers, no counter");
    }
}
