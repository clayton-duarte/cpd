//! The screen of a tui view on ratatui's `TestBackend`, and the keys a test types at it (docs/implementing.md, "The
//! in-process test kit"): they replace `capture-pane -p` / `-e` and `send-keys` for a view run in the test process.
//!
//! - [`Screen::from_buffer`] gives the rows as text and the style of every cell (`fg`, `bg`, `modifier`), which is what
//!   the assertions that read `capture-pane -e` read.
//! - [`Keys::script`] is the `cpd_tui::view_ports::Input` of a view: it hands out the scripted events in order.
//! - [`Script`] is the same for a loop that follows the store: its [`Step`]s are the keys, the polls that find nothing (a
//!   nap on the `ManualClock`), what the world does between two polls (`Step::then`) and a look at the screen the loop
//!   drew so far (`Step::look`, on the [`SharedBackend`] the view draws on). No sleep, no wait: the loop ends when the
//!   script has typed its last key.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::io;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cpd_core::clock::{Clock, ManualClock};
use cpd_tui::view_ports::Input;
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::{Buffer, Cell as BufferCell};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
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

/// One notch of the wheel (`ScrollUp` or `ScrollDown`) at 0-based `column`, `row` of the screen.
pub fn wheel(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE })
}

/// A [`TestBackend`] the test keeps a handle to while the view owns the terminal: both read and write the same buffer.
#[derive(Clone)]
pub struct SharedBackend(Rc<RefCell<TestBackend>>);

impl SharedBackend {
    /// A blank `width` x `height` screen.
    pub fn new(width: u16, height: u16) -> SharedBackend {
        SharedBackend(Rc::new(RefCell::new(TestBackend::new(width, height))))
    }

    /// The screen as the view drew it last.
    pub fn screen(&self) -> Screen {
        Screen::from_buffer(self.0.borrow().buffer())
    }

    /// The terminal a view draws on (`Ctx::new(&ports, backend.terminal(), &mut input)`).
    pub fn terminal(&self) -> ratatui::Terminal<SharedBackend> {
        ratatui::Terminal::new(self.clone()).expect("a test terminal")
    }
}

impl Backend for SharedBackend {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Infallible>
    where
        I: Iterator<Item = (u16, u16, &'a BufferCell)>,
    {
        self.0.borrow_mut().draw(content)
    }
    fn hide_cursor(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().hide_cursor()
    }
    fn show_cursor(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().show_cursor()
    }
    fn get_cursor_position(&mut self) -> Result<Position, Infallible> {
        self.0.borrow_mut().get_cursor_position()
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Infallible> {
        self.0.borrow_mut().set_cursor_position(position)
    }
    fn clear(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().clear()
    }
    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Infallible> {
        self.0.borrow_mut().clear_region(clear_type)
    }
    fn size(&self) -> Result<Size, Infallible> {
        self.0.borrow().size()
    }
    fn window_size(&mut self) -> Result<WindowSize, Infallible> {
        self.0.borrow_mut().window_size()
    }
    fn flush(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().flush()
    }
}

/// One thing a scripted terminal does at a poll or a read of the loop.
pub enum Step {
    /// An event is there: the poll says yes and the read takes it.
    Event(Event),
    /// Nothing comes before the poll's wait is over: the poll naps its wait on the `ManualClock` and says no.
    Idle,
    /// The world changes (another process commits) before the next poll or read; takes no time.
    Do(Box<dyn FnMut()>),
    /// The test looks at the screen the loop drew so far (its last frame); takes no time.
    Look(Box<dyn FnMut(&Screen)>),
}

impl Step {
    /// A press of `code`.
    pub fn key(code: KeyCode) -> Step {
        Step::Event(key(code))
    }

    /// A press of the character `c`.
    pub fn ch(c: char) -> Step {
        Step::Event(ch(c))
    }

    /// `f` runs when the loop reaches this step: what the world does between two polls.
    pub fn then(f: impl FnMut() + 'static) -> Step {
        Step::Do(Box::new(f))
    }

    /// `f` is given the screen the loop drew so far when the loop reaches this step; its assertions are the test's.
    pub fn look(f: impl FnMut(&Screen) + 'static) -> Step {
        Step::Look(Box::new(f))
    }
}

/// The scripted input of a view that follows the store: its [`Step`]s in order. A script that is over is a loop that did
/// not end: it panics, so the test fails loudly instead of spinning.
pub struct Script {
    clock: Arc<ManualClock>,
    screen: SharedBackend,
    steps: VecDeque<Step>,
}

impl Script {
    /// A script of `steps` for a view drawing on `screen`; an idle poll naps on `clock` (`fakes.clock`).
    pub fn new(clock: &Arc<ManualClock>, screen: &SharedBackend, steps: impl IntoIterator<Item = Step>) -> Script {
        Script { clock: Arc::clone(clock), screen: screen.clone(), steps: steps.into_iter().collect() }
    }

    /// How many steps the loop did not reach.
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }

    /// Runs the steps that take no time, up to the next one that does.
    fn settle(&mut self) {
        loop {
            match self.steps.front_mut() {
                Some(Step::Do(change)) => change(),
                Some(Step::Look(look)) => look(&self.screen.screen()),
                _ => return,
            }
            self.steps.pop_front();
        }
    }
}

impl Input for Script {
    fn poll(&mut self, wait: Duration) -> io::Result<bool> {
        self.settle();
        match self.steps.front() {
            None => panic!("the script is over and the loop still polls"),
            Some(Step::Idle) => {
                self.steps.pop_front();
                self.clock.sleep(wait);
                Ok(false)
            }
            Some(_) => Ok(true),
        }
    }

    fn read(&mut self) -> io::Result<Event> {
        self.settle();
        match self.steps.pop_front() {
            Some(Step::Event(event)) => Ok(event),
            Some(Step::Idle) => panic!("a read cannot be idle: the loop reads only what a poll said is there, or blocks"),
            Some(Step::Do(_) | Step::Look(_)) => unreachable!("settled above"),
            None => panic!("the script is over and the loop still reads"),
        }
    }
}
