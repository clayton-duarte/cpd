//! The screen of a tui view on ratatui's `TestBackend`, and the keys a test types at it (docs/implementing.md, "The
//! in-process test kit"): they replace `capture-pane -p` / `-e` and `send-keys` for a view run in the test process.
//!
//! - [`Screen::from_buffer`] gives the rows as text and the style of every cell (`fg`, `bg`, `modifier`), which is what
//!   the assertions that read `capture-pane -e` read.
//! - [`Keys::script`] is the `cpd_tui::view_ports::Input` of a view: it hands out the scripted events in order.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use cpd_tui::view_ports::Input;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};

/// What a cell of the screen shows: its symbol and its style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub symbol: String,
    pub fg: Color,
    pub bg: Color,
    pub modifier: Modifier,
}

/// A copy of a `TestBackend` buffer, read as text and as styles.
#[derive(Clone, Debug)]
pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl Screen {
    /// The screen `buffer` holds (`terminal.backend().buffer()`).
    pub fn from_buffer(buffer: &Buffer) -> Screen {
        let cells = buffer
            .content()
            .iter()
            .map(|cell| Cell { symbol: cell.symbol().to_owned(), fg: cell.fg, bg: cell.bg, modifier: cell.modifier })
            .collect();
        Screen { width: buffer.area.width, height: buffer.area.height, cells }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The cell at column `x`, row `y`; panics outside the screen.
    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        assert!(x < self.width && y < self.height, "cell ({x}, {y}) is outside the {}x{} screen", self.width, self.height);
        &self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)]
    }

    /// Row `y` as text, without its trailing spaces (what `capture-pane -p` prints).
    pub fn row(&self, y: u16) -> String {
        let row: String = (0..self.width).map(|x| self.cell(x, y).symbol.as_str()).collect();
        row.trim_end().to_owned()
    }

    /// Every row as text, top to bottom.
    pub fn rows(&self) -> Vec<String> {
        (0..self.height).map(|y| self.row(y)).collect()
    }

    /// The whole screen as text: its rows, joined by newlines.
    pub fn text(&self) -> String {
        self.rows().join("\n")
    }

    /// The column and row of the first `text` on one row (rows from the top, columns from the left); `None` when absent.
    pub fn find(&self, text: &str) -> Option<(u16, u16)> {
        (0..self.height).find_map(|y| {
            let row: Vec<&str> = (0..self.width).map(|x| self.cell(x, y).symbol.as_str()).collect();
            (0..self.width).find(|&x| row[usize::from(x)..].concat().starts_with(text)).map(|x| (x, y))
        })
    }

    /// Whether `text` is on one row of the screen.
    pub fn contains(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// The cell of the first character of `text`; panics when the screen does not show it.
    pub fn cell_of(&self, text: &str) -> &Cell {
        let (x, y) = self.find(text).unwrap_or_else(|| panic!("{text:?} is not on the screen:\n{}", self.text()));
        self.cell(x, y)
    }
}

/// A key press of `code` with no modifier.
pub fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// A press of the character `c`.
pub fn ch(c: char) -> Event {
    key(KeyCode::Char(c))
}

/// A press of Ctrl and the character `c`.
pub fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

/// The presses of the characters of `text`, in order.
pub fn typed(text: &str) -> Vec<Event> {
    text.chars().map(ch).collect()
}

/// The scripted input of a view: [`Keys::script`]'s events, in order. A script that is over is a loop that did not end:
/// it panics, so the test fails loudly instead of spinning.
pub struct Keys {
    events: VecDeque<Event>,
}

impl Keys {
    /// An input that hands out `events` in order.
    pub fn script(events: impl IntoIterator<Item = Event>) -> Keys {
        Keys { events: events.into_iter().collect() }
    }

    /// How many events no `read` took yet.
    pub fn remaining(&self) -> usize {
        self.events.len()
    }
}

impl Input for Keys {
    fn poll(&mut self, _wait: Duration) -> io::Result<bool> {
        assert!(!self.events.is_empty(), "the script is over and the loop still polls");
        Ok(true)
    }

    fn read(&mut self) -> io::Result<Event> {
        Ok(self.events.pop_front().expect("the script is over and the loop still reads"))
    }
}
