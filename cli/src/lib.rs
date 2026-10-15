//! cpd 2: the Rust rewrite of cpd (binary `cpd2`).
//!
//! The normative documents are `docs/v2/contract.md`, `docs/v2/commands.md`
//! and `docs/v2/CONTEXT.md`; `docs/v2/implementing.md` explains this crate's
//! layout. Every module is declared here once: a Job fills in its own area
//! module and never edits this file.

// Shared foundations (Job rs-workspace).
pub mod args;
pub mod cli;
pub mod error;
pub mod home;
pub mod ids;
pub mod output;

// One module per area, each filled in by its own Job.
pub mod ask;
pub mod attempt;
pub mod config;
pub mod context;
pub mod doctor;
pub mod driver;
pub mod events;
pub mod git;
pub mod host;
pub mod job;
pub mod merge;
pub mod metrics;
pub mod plan;
pub mod pr;
pub mod review;
pub mod session;
pub mod spec;
pub mod store;
pub mod tui;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// Parses `argv`, runs the command and prints its result (contract §1.2).
/// Returns the process exit code.
pub fn main_with(argv: impl IntoIterator<Item = OsString>, stdout: &mut impl Write) -> ExitCode {
    output::emit(cli::run(argv), stdout)
}
