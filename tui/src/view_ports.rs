//! The ports of the views: what a view's loop reads besides the store, so a test runs it on `ManualClock` with scripted
//! input and a `TestBackend` and no terminal (docs/implementing.md, "Ports, Spawn and run_with"). STUBS: every body is
//! `todo!()` or does nothing, until the implementation Job writes it; the red tests are `view_ports_tests`.
//!
//! The old names stay and call the `_with` forms with the real ports: [`Ports::real`], and the real [`Input`] (crossterm's
//! `poll` and `read`) and [`Tty`] (stdin and stdout, `enable_raw_mode`) the implementation Job adds.

use std::io::{self, Write};
use std::time::Duration;

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::Event;

use crate::runner::{Capture, ViewError};
use cpd_core::error::CpdError;
use cpd_core::output::Output;
use cpd_core::ports::Ports;

/// `cpd <argv>` run in this process: what `cli` passes as [`crate::RunCommand`], as a closure a test can capture into.
pub type RunFn<'a> = &'a dyn Fn(Vec<std::ffi::OsString>) -> Result<Output, CpdError>;

/// The events a view waits for: `poll` waits at most `wait` for one, `read` takes it. A test's input is scripted: its
/// `poll` with nothing scripted naps `wait` on the `ManualClock` and says no.
pub trait Input {
    fn poll(&mut self, wait: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;
}

/// The facts of the terminal a view must not read from the process: whether there is one, and its raw mode.
pub trait Tty {
    /// Whether stdin and stdout are both a terminal.
    fn is_terminal(&self) -> bool;
    fn raw_on(&mut self) -> io::Result<()>;
    fn raw_off(&mut self) -> io::Result<()>;
}

/// What a loop of a view runs on: the ports, the terminal it draws on and the input it waits for. It counts its draws.
pub struct Ctx<'a, B: Backend> {
    pub ports: &'a Ports,
    pub terminal: Terminal<B>,
    pub input: &'a mut dyn Input,
    draws: usize,
}

impl<'a, B: Backend> Ctx<'a, B> {
    pub fn new(ports: &'a Ports, terminal: Terminal<B>, input: &'a mut dyn Input) -> Ctx<'a, B> {
        Ctx { ports, terminal, input, draws: 0 }
    }

    /// Draws one frame; the loops draw only through this, so `draws` counts the writes to the pane.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        let _ = render;
        todo!("Ctx::draw")
    }

    /// How many frames were drawn.
    pub fn draws(&self) -> usize {
        self.draws
    }
}

/// Puts the terminal back as it was, on `Drop`: the reporting `capture` turned on is written off to `out`, then the
/// alternate screen is left (written to `out`) and raw mode is turned off (`tty`). A panic and an `Err` pass through it too.
pub struct Restore<'a, W: Write> {
    capture: Capture,
    tty: &'a mut dyn Tty,
    out: &'a mut W,
}

impl<'a, W: Write> Restore<'a, W> {
    pub fn new(capture: Capture, tty: &'a mut dyn Tty, out: &'a mut W) -> Restore<'a, W> {
        Restore { capture, tty, out }
    }
}

impl<W: Write> Drop for Restore<'_, W> {
    fn drop(&mut self) {
        // STUB: restores nothing.
        let _ = (&self.capture, &mut self.tty, &mut self.out);
    }
}

/// [`crate::run_view`] on injected ports: refuses `not_a_terminal` when `tty` says there is none (nothing written, `prepare`
/// not run); else `prepare`, then raw mode on and the alternate screen and `capture` written to `out`, then `body` on a
/// [`Ctx`]; whatever way `body` ends the [`Restore`] runs.
pub fn run_view_with<B, W, P>(
    ports: &Ports,
    tty: &mut dyn Tty,
    out: &mut W,
    terminal: Terminal<B>,
    input: &mut dyn Input,
    name: &str,
    capture: Capture,
    prepare: impl FnOnce() -> Result<P, CpdError>,
    body: impl FnOnce(&mut Ctx<B>, P) -> Result<(), ViewError>,
) -> Result<Output, CpdError>
where
    B: Backend,
    W: Write,
{
    let _ = (ports, tty, out, terminal, input, name, capture, prepare, body);
    todo!("run_view_with")
}

/// [`crate::backed_off`] on injected ports: a failed view in a pane (`on_terminal`) says its error and "Starting again in 3 s."
/// on `err` and naps the back-off on the clock before it returns the result; without a terminal the result is returned at once.
pub fn backed_off_with(
    ports: &Ports,
    on_terminal: bool,
    err: &mut dyn Write,
    result: Result<Output, CpdError>,
) -> Result<Output, CpdError> {
    let _ = (ports, on_terminal, err);
    let _ = result;
    todo!("backed_off_with")
}

/// The spinner frame now, on the clock of `ports` (`palette::frame` of `ports.clock.utc()`).
pub fn frame_now(ports: &Ports) -> usize {
    let _ = ports;
    todo!("frame_now")
}

/// The wall time now in Unix milliseconds, on the clock of `ports`.
pub fn wall_ms(ports: &Ports) -> u64 {
    let _ = ports;
    todo!("wall_ms")
}

/// Whether the snooze that ends at `end` is over, on the clock of `ports`; no snooze is never over.
pub fn snooze_over(ports: &Ports, end: Option<time::OffsetDateTime>) -> bool {
    let _ = (ports, end);
    todo!("snooze_over")
}
