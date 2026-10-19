//! The `statusbar` view (commands.md §13.2, §17): one row across the full width at the bottom of the cockpit.
//! Left, the ⧉ reset-layout, ? legend and ▤ logs buttons, then the note of a failed click; right, the workers
//! indicator `workers N/M`. It has no keys and never takes keyboard focus (the root mouse binding passes a click on
//! with `send-keys -M`, §6.3).
//!
//! It looks at the store's highest Event id every [`TICK`] and re-reads the slots ([`read::slots`], the one thing it
//! draws) when it moved, at most once per [`COALESCE`]: a slot is taken and given back by Events, so nothing is read
//! on a timer. It draws when the workers, the note, the size or a click changed, not on every tick. No subprocess, no
//! `gh`.
//!
//! - [`bar`]: the row's drawing and its clickable items;
//! - [`workers`]: the indicator;
//! - [`layout`]: reset-layout, which the ⧉ button runs and `cpd tui` runs after building a new `main`.

mod bar;
pub(crate) mod layout;
mod workers;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::Line;

use self::bar::{Action, Item};
use self::workers::Workers;
use super::logs;
use super::palette::{self, Meaning};
use super::read;
use super::widgets::open_legend;
use super::runner::{ViewError, kept};
use super::{Capture, run_view};
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::store::Store;

/// How often the view looks at the Events and at the mouse.
const TICK: Duration = Duration::from_millis(200);
/// At most one read per this long, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// The buttons: one glyph, one cell each.
const RESET_BUTTON: &str = "⧉";
const LEGEND_BUTTON: &str = "?";
const LOGS_BUTTON: &str = "▤";

/// The bar's items: the three buttons, then the failure note if any (left), and the workers indicator (right), which
/// gets the full `width` as in the reference (`main.rs` `statusbar_items`).
fn items(workers: &Workers, note: &str, width: u16) -> (Vec<Item>, Vec<Item>) {
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
    let right = workers.line(width).map(|line| Item { line, action: None }).into_iter().collect();
    (left, right)
}

pub(super) fn run() -> Result<Output, CpdError> {
    run_view(
        "statusbar",
        Capture::Mouse,
        || {
            let home = CpdHome::from_env()?;
            let config = Config::load(&home)?;
            let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
            let (slots, cursor) = read::slots(&mut store, config.concurrency)?;
            let follow = read::Follow::new(&mut store, &[], cursor)?;
            Ok((store, config, Workers::of(&slots), follow))
        },
        |terminal, (mut store, config, workers, follow)| {
            let tmux = Tmux::new(config.tmux.socket.clone());
            show(terminal, &mut store, follow, &config, &tmux, workers)
        },
    )
}

/// Draws, re-reads and takes clicks until the terminal fails. A failed click becomes the note; a failed read keeps
/// the workers on screen and shows its error in the note's place until a read succeeds ([`kept`]).
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    mut follow: read::Follow,
    config: &Config,
    tmux: &Tmux,
    mut workers: Workers,
) -> Result<(), ViewError> {
    let mut clickable: Vec<(Rect, Action)> = Vec::new();
    let mut note = String::new();
    let mut error = None::<String>;
    let mut read_at = Instant::now();
    let mut redraw = true;
    loop {
        if redraw {
            let shown = error.as_deref().unwrap_or(&note);
            terminal.draw(|f| {
                let (left, right) = items(&workers, shown, f.area().width);
                clickable = bar::draw(f, f.area(), &left, &right);
            })?;
            redraw = false;
        }
        if event::poll(TICK)? {
            redraw = true; // a click or a resize
            if let Event::Mouse(m) = event::read()? {
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
            let (retry, shown) = (error.is_some(), error.clone());
            let read = (|| {
                let moved = !follow.poll(store)?.is_empty();
                if !(moved || retry) {
                    return Ok(None);
                }
                read::slots(store, config.concurrency).map(|(slots, _)| Some(Workers::of(&slots)))
            })();
            match kept(read, &mut error) {
                Some(None) => {} // nothing moved
                Some(Some(next)) => {
                    redraw |= next != workers;
                    workers = next;
                    read_at = Instant::now();
                }
                None => read_at = Instant::now(), // retried after the next COALESCE
            }
            redraw |= error != shown;
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

    /// Each button is one grey reversed cell, one space apart; the workers end at the right edge.
    #[test]
    fn the_statusbar_has_the_glyph_buttons_left_and_the_workers_right() {
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|f| {
                let (left, right) = items(&Workers { running: 1, limit: 4 }, "", f.area().width);
                hits = bar::draw(f, f.area(), &left, &right);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(text(buf, 0), format!("⧉ ? ▤{}workers 1/4", " ".repeat(40 - 5 - "workers 1/4".len())));
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
