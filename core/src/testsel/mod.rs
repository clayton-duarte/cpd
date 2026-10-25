//! `cpd test changed` and `cpd test run` (commands.md §26): the logic of `scripts/test-changed` and of
//! `scripts/tests.sh`, over [`Ports`]. The selection and the classification of a test name are pure over the text
//! of `git diff` and `cargo metadata`, which the [`Runner`](crate::host::runner::Runner) returns; the commands start
//! `cargo` through `scripts/cargo.sh`, and `node` for pi tests.
//!
//! Every function here is a stub (`todo!()`) until Job rs-tools2-code: the tests in `tests.rs` are red against it.

use std::io::Write;
use std::path::Path;

use clap::{Args, Subcommand};

use crate::error::CpdError;
use crate::ports::Ports;

/// `cpd test` (commands.md §26).
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the Rust tests the changed files can reach (§26.1)
    Changed(ChangedArgs),
    /// Run the named tests (§26.2)
    Run(RunArgs),
}

/// `cpd test changed [--dry-run] [<base>]` (commands.md §26.1).
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ChangedArgs {
    /// Print the one command it would run, and run nothing
    #[arg(long)]
    pub dry_run: bool,
    /// The git revision the changes are counted from (default `origin/main`)
    pub base: Option<String>,
}

/// `cpd test run <name>...` (commands.md §26.2).
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct RunArgs {
    /// A Rust name as nextest filters it, or a pi name `pi/test/<file>.test.ts::<title>`
    #[arg(required = true, value_name = "NAME")]
    pub names: Vec<String>,
}

/// `cpd test <command>` on the real standard streams, from the current directory; the exit status of the process.
pub fn exec(_ports: &Ports, _command: Command) -> Result<u8, CpdError> {
    todo!("cpd test: Job rs-tools2-code")
}

/// `cpd test changed` (§26.1) from `cwd`: the command goes to `out` (a dry run) or runs, its output forwarded to `out`
/// and `err`; `test-changed: nothing to run` goes to `err`. The exit status: 0 for a dry run and for nothing selected,
/// nextest's otherwise; a refusal is an `Err` whose exit code is the status.
pub fn changed(_ports: &Ports, _cwd: &Path, _args: &ChangedArgs, _out: &mut dyn Write, _err: &mut dyn Write) -> Result<u8, CpdError> {
    todo!("cpd test changed: Job rs-tools2-code")
}

/// `cpd test run <name>...` (§26.2) from `cwd`, run from the repo root: the output of the tests goes to `out` and
/// `err`. Non-zero when any test failed or did not run; no name is a `usage` error.
pub fn run(_ports: &Ports, _cwd: &Path, _names: &[String], _out: &mut dyn Write, _err: &mut dyn Write) -> Result<u8, CpdError> {
    todo!("cpd test run: Job rs-tools2-code")
}

/// `scripts/tests.sh build`: `nextest run --workspace --no-run`, then `npm run typecheck`; stops at the first failure.
pub fn build(_ports: &Ports, _cwd: &Path, _out: &mut dyn Write, _err: &mut dyn Write) -> Result<u8, CpdError> {
    todo!("cpd test build: Job rs-tools2-code")
}

/// `scripts/tests.sh list <name>`: 0 only when that one test exists.
pub fn list(_ports: &Ports, _cwd: &Path, _name: &str, _err: &mut dyn Write) -> Result<u8, CpdError> {
    todo!("cpd test list: Job rs-tools2-code")
}

#[cfg(test)]
mod tests;
