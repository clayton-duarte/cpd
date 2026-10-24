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
