//! The screen of a tui view on ratatui's `TestBackend`, and the keys a test types at it (docs/implementing.md, "The
//! in-process test kit"): they replace `capture-pane -p` / `-e` and `send-keys` for a view run in the test process.
//!
//! - [`Screen::from_buffer`] gives the rows as text and the style of every cell (`fg`, `bg`, `modifier`), which is what
//!   the assertions that read `capture-pane -e` read.
//! - [`Keys::script`] is the `cpd_tui::view_ports::Input` of a view: it hands out the scripted events in order.
//! - [`Script`] is the same for a loop that waits on a clock: its [`Step`]s are the keys, the polls that find nothing (a
//!   nap on the clock), what the world does between two polls (`then`, `Step::then`) and a look at the screen the loop
//!   drew so far (`Step::look`, with a [`Script::on_screen`] on the [`SharedBackend`] the view draws on). No sleep, no
//!   wait: the loop ends when the script has typed its last key.
//! - [`Display`] is a `TestBackend` the test keeps a handle of while the view owns the terminal made of it.
//! - [`SharedBackend`] is the same on one thread, with a hook that runs after every frame ([`Steps`]).
//! - [`FakeTty`] is the `Tty` of a view: whether there is a terminal, and the log of its raw mode.
//! - [`Bytes`] is a writer the test reads while the code under test holds it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::io::{self, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cpd_core::clock::Clock;
use cpd_tui::view_ports::{Input, Tty};
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::{Buffer, Cell as BufferCell, CellDiffOption};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};

/// What a cell of the screen shows: its symbol and its style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub symbol: String,
    pub fg: Color,
    pub bg: Color,
    pub modifier: Modifier,
    /// How many columns the symbol covers: 1, or the forced width of a linked piece (`cpd_tui::link`).
    pub covers: u16,
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
            .map(|cell| Cell {
                symbol: cell.symbol().to_owned(),
                fg: cell.fg,
                bg: cell.bg,
                modifier: cell.modifier,
                covers: match cell.diff_option {
                    CellDiffOption::ForcedWidth(width) => width.get(),
                    _ => 1,
                },
            })
            .collect();
        Screen {
            width: buffer.area.width,
            height: buffer.area.height,
            cells,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The cell at column `x`, row `y`; panics outside the screen.
    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        assert!(
            x < self.width && y < self.height,
            "cell ({x}, {y}) is outside the {}x{} screen",
            self.width,
            self.height
        );
        &self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)]
    }

    /// What row `y` shows, piece by piece: the column and the text of each. A link's piece (`cpd_tui::link`) is one cell
    /// whose symbol carries its OSC 8 escapes around its text; the backend never writes the cell(s) after it, so those hold
    /// nothing to show and a terminal shows the piece's text where its first cell is.
    fn pieces(&self, y: u16) -> Vec<(u16, &str)> {
        let (mut pieces, mut x) = (Vec::new(), 0);
        while x < self.width {
            let symbol = self.cell(x, y).symbol.as_str();
            let link = symbol
                .strip_prefix("\x1b]8;;")
                .and_then(|s| s.split_once('\x07'))
                .map(|(_, text)| text.trim_end_matches("\x1b]8;;\x07"));
            pieces.push((x, link.unwrap_or(symbol)));
            x += link.map_or(1, |text| text.chars().count().max(1) as u16);
        }
        pieces
    }

    /// Row `y` as text, without its trailing spaces (what `capture-pane -p` prints).
    pub fn row(&self, y: u16) -> String {
        let row: String = self.pieces(y).into_iter().map(|(_, text)| text).collect();
        row.trim_end().to_owned()
    }

    /// Row `y` as the terminal shows it: a linked piece is one cell whose symbol carries its OSC 8 escapes (`ESC ] 8 ; ; <url> BEL`,
    /// which take no column) and holds the text of the cells it covers, so the cells it covers are skipped; no trailing spaces.
    pub fn shown_row(&self, y: u16) -> String {
        let (mut row, mut x) = (String::new(), 0);
        while x < self.width {
            let cell = self.cell(x, y);
            row.push_str(&without_escapes(&cell.symbol));
            x += cell.covers;
        }
        row.trim_end().to_owned()
    }

    /// Every row as [`Screen::shown_row`] shows it, top to bottom.
    pub fn shown_rows(&self) -> Vec<String> {
        (0..self.height).map(|y| self.shown_row(y)).collect()
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
            let pieces = self.pieces(y);
            (0..pieces.len())
                .find(|&i| {
                    pieces[i..]
                        .iter()
                        .map(|(_, piece)| *piece)
                        .collect::<String>()
                        .starts_with(text)
                })
                .map(|i| (pieces[i].0, y))
        })
    }

    /// Whether `text` is on one row of the screen.
    pub fn contains(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// The cell of the first character of `text`; panics when the screen does not show it.
    pub fn cell_of(&self, text: &str) -> &Cell {
        let (x, y) = self
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} is not on the screen:\n{}", self.text()));
        self.cell(x, y)
    }
}

/// `symbol` without its OSC 8 escapes (`ESC ] 8 ; ; <url> BEL`).
fn without_escapes(symbol: &str) -> String {
    let mut out = String::new();
    let mut chars = symbol.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|c| *c == '\x07');
        } else {
            out.push(c);
        }
    }
    out
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
        Keys {
            events: events.into_iter().collect(),
        }
    }

    /// How many events no `read` took yet.
    pub fn remaining(&self) -> usize {
        self.events.len()
    }
}

impl Input for Keys {
    fn poll(&mut self, _wait: Duration) -> io::Result<bool> {
        assert!(
            !self.events.is_empty(),
            "the script is over and the loop still polls"
        );
        Ok(true)
    }

    fn read(&mut self) -> io::Result<Event> {
        Ok(self
            .events
            .pop_front()
            .expect("the script is over and the loop still reads"))
    }
}

/// The input of a loop that does not read it (a scripted pump carries the events): any read is a bug of the test.
pub struct NoInput;

impl Input for NoInput {
    fn poll(&mut self, _: Duration) -> io::Result<bool> {
        panic!("the loop polls ctx.input: its events come from the pump");
    }
    fn read(&mut self) -> io::Result<Event> {
        panic!("the loop reads ctx.input: its events come from the pump");
    }
}

/// One notch of the wheel (`ScrollUp` or `ScrollDown`) at 0-based `column`, `row` of the screen.
pub fn wheel(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A [`TestBackend`] the test keeps a handle to while the view owns the terminal: both read and write the same buffer.
#[derive(Clone)]
pub struct SharedBackend(Rc<RefCell<TestBackend>>, Rc<RefCell<Option<FrameHook>>>);

/// What runs after each frame the view wrote ([`SharedBackend::on_frame`]).
type FrameHook = Box<dyn FnMut(&Screen)>;

impl SharedBackend {
    /// A blank `width` x `height` screen.
    pub fn new(width: u16, height: u16) -> SharedBackend {
        SharedBackend(
            Rc::new(RefCell::new(TestBackend::new(width, height))),
            Rc::default(),
        )
    }

    /// The screen as the view drew it last.
    pub fn screen(&self) -> Screen {
        Screen::from_buffer(self.0.borrow().buffer())
    }

    /// `hook` runs after every frame the view writes (the terminal's `flush`), with the screen of that frame, on the view's
    /// thread and before the view goes on: what the world does while the view waits (a commit, the time), and a look at
    /// the screen. Replaces the one before. [`Steps`] is the usual hook.
    pub fn on_frame(&self, hook: impl FnMut(&Screen) + 'static) {
        *self.1.borrow_mut() = Some(Box::new(hook));
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
        self.0.borrow_mut().flush()?;
        // the hook is out of its cell while it runs: it may call `on_frame` itself
        let hook = self.1.borrow_mut().take();
        if let Some(mut hook) = hook {
            hook(&self.screen());
            self.1.borrow_mut().get_or_insert(hook);
        }
        Ok(())
    }
}

/// What a test does, in order, as a view draws: each step waits for a frame whose screen satisfies its condition, runs its
/// action (a change of the world, a look at the screen, the time passing on the `ManualClock`) and hands over to the next
/// step; one step at most per frame, so a step's action is always seen by the frames after it. No sleep and no poll: a
/// frame is what the view draws, and the signals of the scripted pump decide when. A script of signals that is over before
/// the last step ran is a test that did not do what it says: [`Steps::finish`] fails and names the step.
/// A scripted step: (name, is the screen ready, what to do then).
type ScreenStep = (
    String,
    Box<dyn Fn(&Screen) -> bool>,
    Box<dyn FnMut(&Screen)>,
);

#[derive(Clone, Default)]
pub struct Steps {
    todo: Rc<RefCell<VecDeque<ScreenStep>>>,
}

impl Steps {
    pub fn new() -> Steps {
        Steps::default()
    }

    /// At the first frame whose screen satisfies `when`: `then` runs.
    pub fn at(
        self,
        what: &str,
        when: impl Fn(&Screen) -> bool + 'static,
        then: impl FnMut(&Screen) + 'static,
    ) -> Steps {
        self.todo
            .borrow_mut()
            .push_back((what.to_owned(), Box::new(when), Box::new(then)));
        self
    }

    /// At the next frame: `then` runs.
    pub fn next(self, what: &str, then: impl FnMut(&Screen) + 'static) -> Steps {
        self.at(what, |_| true, then)
    }

    /// Makes these steps the frame hook of `backend`.
    pub fn on(&self, backend: &SharedBackend) {
        let todo = Rc::clone(&self.todo);
        backend.on_frame(move |screen| {
            // out of the queue while it runs, so an action that looks at the steps finds them as they are
            let step = {
                let mut todo = todo.borrow_mut();
                match todo.front() {
                    Some((_, when, _)) if when(screen) => todo.pop_front(),
                    _ => None,
                }
            };
            if let Some((_, _, mut then)) = step {
                then(screen);
            }
        });
    }

    /// The steps no frame reached yet, by what they wait for.
    pub fn pending(&self) -> Vec<String> {
        self.todo
            .borrow()
            .iter()
            .map(|(what, ..)| what.clone())
            .collect()
    }

    /// Fails when a step was not reached: the view ended first.
    pub fn finish(&self) {
        let pending = self.pending();
        assert!(
            pending.is_empty(),
            "the view ended before the steps {pending:?}"
        );
    }
}

/// A left click at 0-based `column`, `row` of the screen (what `\x1b[<0;..M` is).
pub fn left_click(column: u16, row: u16) -> Event {
    mouse(MouseEventKind::Down(MouseButton::Left), column, row)
}

/// A right click at 0-based `column`, `row` of the screen.
pub fn right_click(column: u16, row: u16) -> Event {
    mouse(MouseEventKind::Down(MouseButton::Right), column, row)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// One thing a [`Script`] does at a poll or a read.
pub enum Step {
    /// An event is there.
    Event(Event),
    /// Nothing comes before the poll's wait is over: the poll naps its wait on the clock and says no.
    Idle,
    /// The world changes before the next step, and takes no time.
    Do(Box<dyn FnMut()>),
    /// The test looks at the screen the loop drew so far (its last frame); takes no time. Needs [`Script::on_screen`].
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

    /// `f` runs when the loop reaches this step: what the world does between two polls (as the free fn [`then`]).
    pub fn then(f: impl FnMut() + 'static) -> Step {
        Step::Do(Box::new(f))
    }

    /// `f` is given the screen the loop drew so far when the loop reaches this step; its assertions are the test's.
    pub fn look(f: impl FnMut(&Screen) + 'static) -> Step {
        Step::Look(Box::new(f))
    }
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
    screen: Option<SharedBackend>,
    steps: VecDeque<Step>,
}

impl Script {
    /// An input that does `steps` in order; its idle polls nap on `clock` (the view's own).
    pub fn new(clock: Arc<dyn Clock>, steps: impl IntoIterator<Item = Step>) -> Script {
        Script {
            clock,
            screen: None,
            steps: steps.into_iter().collect(),
        }
    }

    /// As [`Script::new`], for a view drawing on `screen`: a [`Step::look`] is given what it drew so far.
    pub fn on_screen(
        clock: Arc<dyn Clock>,
        screen: &SharedBackend,
        steps: impl IntoIterator<Item = Step>,
    ) -> Script {
        Script {
            clock,
            screen: Some(screen.clone()),
            steps: steps.into_iter().collect(),
        }
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
                Some(Step::Look(look)) => {
                    look(
                        &self
                            .screen
                            .as_ref()
                            .expect("a Step::look needs Script::on_screen")
                            .screen(),
                    );
                }
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
            Some(Step::Idle) => {
                panic!("a read cannot be idle: the loop reads only what a poll said is there")
            }
            Some(Step::Do(_) | Step::Look(_)) => unreachable!("settled above"),
            None => panic!("the script is over and the loop still reads"),
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
        Display {
            backend: Arc::new(Mutex::new(TestBackend::new(width, height))),
            flushes: Arc::default(),
        }
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
        FakeTty {
            terminal,
            log: Rc::default(),
        }
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
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Bytes {
    /// Everything written so far.
    pub fn bytes(&self) -> Vec<u8> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
