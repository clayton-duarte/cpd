//! The screen of a tui view on ratatui's `TestBackend`, and the keys a test types at it (docs/implementing.md, "The
//! in-process test kit"): they replace `capture-pane -p` / `-e` and `send-keys` for a view run in the test process.
//!
//! - [`Screen::from_buffer`] gives the rows as text and the style of every cell (`fg`, `bg`, `modifier`), which is what
//!   the assertions that read `capture-pane -e` read.
//! - [`Keys::script`] is the `cpd_tui::view_ports::Input` of a view: it hands out the scripted events in order.
//! - [`Script`] is an `Input` too, with time and a changing world: a step is an event, an idle poll (it naps the poll's
//!   wait on the clock) or a closure that runs between two polls (a commit by another process, a snapshot of the screen).
//! - [`Display`] is a `TestBackend` the test keeps a handle of while the view owns the terminal made of it.
//! - [`FakeTty`] is the `Tty` of a view: whether there is a terminal, and the log of its raw mode.
//! - [`Bytes`] is a writer the test reads while the code under test holds it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cpd_core::clock::Clock;
use cpd_tui::view_ports::{Input, Tty};
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::{Buffer, Cell as BufferCell};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Size};
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

/// One thing a [`Script`] does at a poll or a read.
pub enum Step {
    /// An event is there.
    Event(Event),
    /// Nothing comes before the poll's wait is over: the poll naps its wait on the clock and says no.
    Idle,
    /// The world changes before the next step, and takes no time.
    Do(Box<dyn FnMut()>),
}

/// `n` idle polls.
pub fn idle(n: usize) -> Vec<Step> {
    (0..n).map(|_| Step::Idle).collect()
}

/// A step that runs `change`.
pub fn then(change: impl FnMut() + 'static) -> Step {
    Step::Do(Box::new(change))
}

/// The scripted input of a view that waits on a clock. A script that is over is a loop that did not end: it panics, so
/// the test fails loudly instead of spinning.
pub struct Script {
    clock: Arc<dyn Clock>,
    steps: VecDeque<Step>,
}

impl Script {
    /// An input that does `steps` in order; its idle polls nap on `clock` (the view's own).
    pub fn new(clock: Arc<dyn Clock>, steps: impl IntoIterator<Item = Step>) -> Script {
        Script { clock, steps: steps.into_iter().collect() }
    }

    /// How many steps are left.
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }
}

impl Input for Script {
    fn poll(&mut self, wait: Duration) -> io::Result<bool> {
        loop {
            match self.steps.front_mut() {
                None => panic!("the script is over and the loop still polls"),
                Some(Step::Do(change)) => {
                    change();
                    self.steps.pop_front();
                }
                Some(Step::Idle) => {
                    self.steps.pop_front();
                    self.clock.sleep(wait);
                    return Ok(false);
                }
                Some(Step::Event(_)) => return Ok(true),
            }
        }
    }

    fn read(&mut self) -> io::Result<Event> {
        loop {
            match self.steps.pop_front() {
                None => panic!("the script is over and the loop still reads"),
                Some(Step::Do(mut change)) => change(),
                Some(Step::Event(event)) => return Ok(event),
                Some(Step::Idle) => panic!("a read cannot be idle: the loop reads only what a poll said is there"),
            }
        }
    }
}

/// A `TestBackend` shared with the test: `Display::terminal()` is the terminal a view draws on, and `screen()` reads what
/// it drew last, also while the view still runs (from a [`Step::Do`], or from the test while a view on another thread waits
/// for its input).
#[derive(Clone)]
pub struct Display {
    backend: Arc<Mutex<TestBackend>>,
    flushes: Arc<AtomicUsize>,
}

impl Display {
    /// A `width` x `height` display.
    pub fn new(width: u16, height: u16) -> Display {
        Display { backend: Arc::new(Mutex::new(TestBackend::new(width, height))), flushes: Arc::default() }
    }

    fn backend(&self) -> std::sync::MutexGuard<'_, TestBackend> {
        self.backend.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many times the view flushed its terminal: one per `terminal.draw`, even with an empty diff (what `syscw` of
    /// the process counted when the views ran in a pane).
    pub fn flushes(&self) -> usize {
        self.flushes.load(Ordering::SeqCst)
    }

    /// A terminal that draws on this display.
    pub fn terminal(&self) -> ratatui::Terminal<Display> {
        ratatui::Terminal::new(self.clone()).expect("a terminal on a test display")
    }

    /// The screen as it is drawn now.
    pub fn screen(&self) -> Screen {
        Screen::from_buffer(self.backend().buffer())
    }
}

impl Backend for Display {
    type Error = std::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a BufferCell)>,
    {
        self.backend().draw(content)
    }
    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.backend().hide_cursor()
    }
    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.backend().show_cursor()
    }
    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.backend().get_cursor_position()
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.backend().set_cursor_position(position)
    }
    fn clear(&mut self) -> Result<(), Self::Error> {
        self.backend().clear()
    }
    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.backend().clear_region(clear_type)
    }
    fn size(&self) -> Result<Size, Self::Error> {
        self.backend().size()
    }
    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.backend().window_size()
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.flushes.fetch_add(1, Ordering::SeqCst);
        self.backend().flush()
    }
}

/// The facts of a terminal, scripted: whether there is one; each raw-mode call is logged, shared with the test.
pub struct FakeTty {
    terminal: bool,
    /// `"raw_on"` and `"raw_off"`, in the order the view called them.
    pub log: Rc<RefCell<Vec<&'static str>>>,
}

impl FakeTty {
    /// A terminal that is there (`terminal`) or not.
    pub fn new(terminal: bool) -> FakeTty {
        FakeTty { terminal, log: Rc::default() }
    }
}

impl Tty for FakeTty {
    fn is_terminal(&self) -> bool {
        self.terminal
    }
    fn raw_on(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("raw_on");
        Ok(())
    }
    fn raw_off(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("raw_off");
        Ok(())
    }
}

/// A writer whose bytes the test reads while the code under test holds a clone of it.
#[derive(Clone, Default)]
pub struct Bytes(Arc<Mutex<Vec<u8>>>);

impl Write for Bytes {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Bytes {
    /// Everything written so far.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}
