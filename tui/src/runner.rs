//! [`run_view`]: what every view of `cpd tui view` runs through (commands.md §13.2). One place owns the terminal
//! set-up (raw mode, the alternate screen, mouse and focus reporting), its restoring on every way out (the end of
//! the loop, an error, a panic), the mapping of a terminal failure to an error, and the policy for a failed read
//! ([`kept`]).

use std::io::{Stdout, stdout};

use ratatui::DefaultTerminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::view_ports::{CrosstermInput, Ctx, StdTty, run_view_with};
use super::{not_a_terminal, on_a_terminal};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;
use cpd_core::ports::Ports;

/// What a view asks the terminal to report to it, besides keys. Focus reporting without the mouse is not a choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    /// Keys only.
    Keys,
    /// Keys and the mouse.
    Mouse,
    /// Keys, the mouse and focus changes.
    MouseAndFocus,
}

/// Why a view's loop ended without the User leaving it.
#[derive(Debug)]
pub enum ViewError {
    /// The terminal failed (a draw, a poll, a read of input).
    Terminal(std::io::Error),
    /// The view's own work failed; its error is reported as it is.
    Cpd(CpdError),
}

impl From<std::io::Error> for ViewError {
    fn from(e: std::io::Error) -> ViewError {
        ViewError::Terminal(e)
    }
}

impl From<CpdError> for ViewError {
    fn from(e: CpdError) -> ViewError {
        ViewError::Cpd(e)
    }
}

/// Runs one view of `cpd tui view <name>`.
///
/// Refuses with `not_a_terminal` when stdin or stdout is no terminal. Then `prepare` takes what the view needs
/// before anything is drawn (its first read: a failure is the command's error, with the screen untouched). From
/// there this process owns the terminal (commands.md §1.2): the alternate screen and `capture` are set up, `body`
/// runs with the terminal and what `prepare` gave, and whatever way `body` ends, by `Ok`, by `Err` or by a panic, the
/// terminal is restored and the reporting `capture` turned on is turned off.
pub fn run_view<P>(
    name: &str,
    capture: Capture,
    prepare: impl FnOnce() -> Result<P, CpdError>,
    body: impl FnOnce(&mut DefaultTerminal, P) -> Result<(), ViewError>,
) -> Result<Output, CpdError> {
    run_view_on(
        &Ports::current(),
        name,
        capture,
        prepare,
        |ctx, prepared| body(&mut ctx.terminal, prepared),
    )
}

/// [`run_view`] on this process's terminal and input, with `body` on a [`Ctx`] over `ports`: what the views whose loops
/// wait through the ports run through; a test runs the loop itself through [`run_view_with`].
pub fn run_view_on<P>(
    ports: &Ports,
    name: &str,
    capture: Capture,
    prepare: impl FnOnce() -> Result<P, CpdError>,
    body: impl FnOnce(&mut Ctx<CrosstermBackend<Stdout>>, P) -> Result<(), ViewError>,
) -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal(&format!("tui view {name}")));
    }
    let terminal = Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| {
        CpdError::new(
            ErrorCode::NotATerminal,
            format!("The terminal of cpd tui view {name} failed: {e}."),
        )
        .with("command", format!("tui view {name}"))
    })?;
    // ratatui's `init` does this too: the panic message is printed on the normal screen, not lost with the alternate one.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        previous(info);
    }));
    run_view_with(
        ports,
        &mut StdTty,
        &mut stdout(),
        terminal,
        &mut CrosstermInput,
        name,
        capture,
        prepare,
        body,
    )
}

/// The policy of every view for a failed read: the last good snapshot stays on screen and `error` holds the
/// failure, one row of text for the view to draw, until a read succeeds and clears it. The view reads again at its
/// next refresh while `error` is set, whatever the Events say. Returns what a good read gave, for the view to take.
pub(crate) fn kept<T>(read: Result<T, CpdError>, error: &mut Option<String>) -> Option<T> {
    match read {
        Ok(next) => {
            *error = None;
            Some(next)
        }
        Err(e) => {
            *error = Some(one_row(e.message()));
            None
        }
    }
}

/// `text` on one line: its lines joined by a space.
fn one_row(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(text: &str) -> Result<u32, CpdError> {
        Err(CpdError::new(ErrorCode::StoreBusy, text))
    }

    #[test]
    fn a_failed_read_keeps_the_snapshot_and_a_good_one_clears_the_error() {
        let mut error = None;
        assert_eq!(kept(failure("the store\nis busy"), &mut error), None);
        assert_eq!(error.as_deref(), Some("the store is busy"));
        assert_eq!(kept(failure("again"), &mut error), None);
        assert_eq!(error.as_deref(), Some("again"));
        assert_eq!(kept(Ok(2), &mut error), Some(2));
        assert_eq!(error, None);
    }
}
