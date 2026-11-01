//! The ports of the views: what a view's loop reads besides the store, so a test runs it on `ManualClock` with scripted
//! input and a `TestBackend` and no terminal (docs/implementing.md, "Ports, Spawn and run_with"); the red tests are
//! `view_ports_tests`.
//!
//! The old names stay and call the `_with` forms with the real ports: [`Ports::real`], and the real [`Input`] (crossterm's
//! `poll` and `read`: [`CrosstermInput`]) and [`Tty`] (stdin and stdout, `enable_raw_mode`: [`StdTty`]).

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, SystemTime};

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};

use crate::runner::{Capture, ViewError};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;
use cpd_core::ports::Ports;

/// `cpd <argv>` run in this process: what `cli` passes as [`crate::RunCommand`], as a closure a test can capture into.
pub type RunFn<'a> = &'a dyn Fn(Vec<std::ffi::OsString>) -> Result<Output, CpdError>;

/// The events a view waits for: `poll` waits at most `wait` for one, `read` takes it. A test's input is scripted: its
/// `poll` with nothing scripted naps `wait` on the `ManualClock` and says no.
pub trait Input {
    fn poll(&mut self, wait: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;

    /// For a driven loop with work under way (a command that runs): whether an event is there now. The loop that is
    /// driven waits for its work to end, in no time of the clock's, so a scripted input says no, the default. An input that
    /// says it can, takes the loop on reading events while the work runs.
    fn ready_while_busy(&mut self) -> io::Result<bool> {
        Ok(false)
    }

    /// Told by a loop each time it takes a snapshot of the store (the model it draws, read again): what a test that
    /// drives a view from outside counts, to assert that an idle view reads nothing. The real input ignores it.
    fn snapshot_read(&mut self) {}
}

/// The facts of the terminal a view must not read from the process: whether there is one, and its raw mode.
pub trait Tty {
    /// Whether stdin and stdout are both a terminal.
    fn is_terminal(&self) -> bool;
    fn raw_on(&mut self) -> io::Result<()>;
    fn raw_off(&mut self) -> io::Result<()>;
}

/// The real input: crossterm's `poll` and `read` on this process's terminal.
pub struct CrosstermInput;

impl Input for CrosstermInput {
    fn poll(&mut self, wait: Duration) -> io::Result<bool> {
        event::poll(wait)
    }
    fn read(&mut self) -> io::Result<Event> {
        event::read()
    }
}

/// The real terminal facts: stdin and stdout, and crossterm's raw mode.
pub struct StdTty;

impl Tty for StdTty {
    fn is_terminal(&self) -> bool {
        io::stdin().is_terminal() && io::stdout().is_terminal()
    }
    fn raw_on(&mut self) -> io::Result<()> {
        enable_raw_mode()
    }
    fn raw_off(&mut self) -> io::Result<()> {
        disable_raw_mode()
    }
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
        self.terminal.draw(render).map_err(|e| io::Error::other(e.to_string()))?;
        self.draws += 1;
        Ok(())
    }

    /// The loop took a snapshot of the store: its input is told ([`Input::snapshot_read`]). Every loop calls this where it
    /// reads its model again, and nowhere else (the polls of the Events are no snapshot).
    pub fn snapshot_read(&mut self) {
        self.input.snapshot_read();
    }

    /// How many frames were drawn.
    pub fn draws(&self) -> usize {
        self.draws
    }
}

/// Puts the terminal back as it was, on `Drop`: the reporting `capture` turned on is written off to `out`, then the
/// alternate screen is left (written to `out`) and raw mode is turned off (`tty`). A panic and an `Err` pass through it too.
/// ratatui's own panic hook restores only raw mode and the alternate screen, never the mouse and focus reporting.
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
        // Nothing can be done about a failure here, and `drop` may run while a panic unwinds.
        match self.capture {
            Capture::Keys => {}
            Capture::Mouse => drop(execute!(self.out, DisableMouseCapture)),
            Capture::MouseAndFocus => drop(execute!(self.out, DisableFocusChange, DisableMouseCapture)),
        }
        drop(execute!(self.out, LeaveAlternateScreen));
        drop(self.tty.raw_off());
    }
}

/// [`crate::run_view`] on injected ports: refuses `not_a_terminal` when `tty` says there is none (nothing written, `prepare`
/// not run); else `prepare`, then raw mode on and the alternate screen and `capture` written to `out`, then `body` on a
/// [`Ctx`]; whatever way `body` ends the [`Restore`] runs.
#[allow(clippy::too_many_arguments)]
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
    if !tty.is_terminal() {
        return Err(crate::not_a_terminal(&format!("tui view {name}")));
    }
    let prepared = prepare()?;
    let terminal_failed = |e: io::Error| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view {name} failed: {e}."))
            .with("command", format!("tui view {name}"))
    };
    tty.raw_on().map_err(terminal_failed)?;
    let restore = Restore::new(capture, tty, out);
    let on = |out: &mut W| -> io::Result<()> {
        execute!(out, EnterAlternateScreen)?;
        match capture {
            Capture::Keys => Ok(()),
            Capture::Mouse => execute!(out, EnableMouseCapture),
            Capture::MouseAndFocus => execute!(out, EnableMouseCapture, EnableFocusChange),
        }
    };
    on(restore.out).map_err(terminal_failed)?;
    let mut ctx = Ctx::new(ports, terminal, input);
    let result = body(&mut ctx, prepared);
    drop(ctx);
    drop(restore);
    match result {
        Ok(()) => Ok(Output::Owned),
        Err(ViewError::Terminal(e)) => Err(terminal_failed(e)),
        Err(ViewError::Cpd(e)) => Err(e),
    }
}

/// How long a view in a pane that failed waits before it exits.
pub(crate) const BACKOFF: Duration = Duration::from_secs(3);

/// [`crate::backed_off`] on injected ports: a failed view in a pane (`on_terminal`) says its error and "Starting again in 3 s."
/// on `err` and naps the back-off on the clock before it returns the result; without a terminal the result is returned at once.
pub fn backed_off_with(
    ports: &Ports,
    on_terminal: bool,
    err: &mut dyn Write,
    result: Result<Output, CpdError>,
) -> Result<Output, CpdError> {
    if let Err(e) = &result
        && on_terminal {
            drop(writeln!(err, "{}\nStarting again in {} s.", e.message(), BACKOFF.as_secs()));
            ports.clock.sleep(BACKOFF);
        }
    result
}

/// The spinner frame now, on the clock of `ports` (`palette::frame` of `ports.clock.utc()`).
pub fn frame_now(ports: &Ports) -> usize {
    crate::palette::frame(SystemTime::from(ports.clock.utc()))
}

/// The wall time now in Unix milliseconds, on the clock of `ports`.
pub fn wall_ms(ports: &Ports) -> u64 {
    u64::try_from(ports.clock.utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
}

/// Whether the snooze that ends at `end` is over, on the clock of `ports`; no snooze is never over.
pub fn snooze_over(ports: &Ports, end: Option<time::OffsetDateTime>) -> bool {
    end.is_some_and(|end| ports.clock.utc() >= end)
}
