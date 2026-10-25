//! cpd (binary `cpd`): the root command tree.
//! The shared core is the `cpd-core` crate (`core/`), the cockpit the `cpd-tui` crate (`tui/`).

pub mod cli;
pub mod interface;

#[cfg(feature = "fakes")]
pub mod fakes;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// Parses `argv`, runs the command and prints its result (contract §1.2).
/// Returns the process exit code.
pub fn main_with(argv: impl IntoIterator<Item = OsString>, stdout: &mut impl Write) -> ExitCode {
    cpd_core::output::emit(cli::run(argv), stdout)
}

/// Runs `argv` on `ports` and writes its result to `out`; returns the exit code. The thin mains call it.
///
/// STUB of PR ports2's red Job: Job ports-3 implements it (`main_with` then calls it with `Ports::real()`).
pub fn run_with(_ports: &cpd_core::ports::Ports, _argv: impl IntoIterator<Item = OsString>, _out: &mut dyn Write) -> ExitCode {
    todo!("run_with (Job ports-3)")
}
