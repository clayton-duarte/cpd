//! `cpd prune` (commands.md §26): the clean-up of leftover worktrees whose work reached `origin/main`.
//!
//! STUB: the signatures the tests of `tests.rs` call; the logic is the next Job's.

use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::output::Output;
use crate::ports::Ports;

#[cfg(test)]
mod tests;

/// `cpd prune [--apply] [--drop <lead-branch>]... <repo>...`
#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Do the removals; without it nothing changes and the command prints what it would do
    #[arg(long)]
    pub apply: bool,
    /// Remove the worktrees of this Lead Branch whether or not they are merged (repeatable)
    #[arg(long = "drop", value_name = "lead-branch")]
    pub drop: Vec<String>,
    /// The repos to clean
    #[arg(required = true, value_name = "repo")]
    pub repos: Vec<PathBuf>,
}

/// What a run printed and the exit code it ends with (commands.md §26: 0, 1 a command failed, 2 usage or the store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub exit: u8,
    pub stdout: String,
    pub stderr: String,
}

/// Prunes `args.repos` on `ports`, reading the protected set from the store of `home`.
pub fn run(_ports: &Ports, _home: &CpdHome, _args: &Args) -> Outcome {
    todo!("cpd prune: rs-prune2-code")
}

/// The command of the binary: [`run`] on the real ports and `$CPD_HOME`.
pub fn command(_args: Args) -> Result<Output, CpdError> {
    todo!("cpd prune: rs-prune2-code")
}
