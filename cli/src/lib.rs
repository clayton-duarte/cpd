//! cpd (binary `cpd`): the root command tree.
//! The shared core is the `cpd-core` crate (`core/`), the cockpit the `cpd-tui` crate (`tui/`).

pub mod cli;
pub mod install;
pub mod interface;

#[cfg(feature = "fakes")]
pub mod fakes;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// Parses `argv`, runs the command and prints its result (contract §1.2).
/// Returns the process exit code.
pub fn main_with(argv: impl IntoIterator<Item = OsString>, stdout: &mut impl Write) -> ExitCode {
    run_with(&cpd_core::ports::Ports::real(), argv, stdout)
}

/// Runs `argv` on `ports` and writes its result to `out`; returns the exit code. The thin mains call it.
pub fn run_with(
    ports: &cpd_core::ports::Ports,
    argv: impl IntoIterator<Item = OsString>,
    mut out: &mut dyn Write,
) -> ExitCode {
    cpd_core::output::emit(cli::run_with(ports, argv), &mut out)
}
