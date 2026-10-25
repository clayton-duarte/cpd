//! `cpd host approve` and `cpd host repair` (commands.md §9.11; hs-d618): the User-approved
//! repair of the cpd checkout `$HOME/projects/cpd` when dotfiles' install skipped its
//! fast-forward.
//!
//! - `cpd host approve` (internal; the adapter's `cpd_host_approve` tool after the User's
//!   Allow once): Manager only; writes one private approval file for the calling Session,
//!   valid 10 minutes, one repair.
//! - `cpd host repair` (Manager only; contract §3.7 E6): saves the diff and the old HEAD as
//!   a ref, then resets `main` to `origin/main`. Needs a live approval of the calling
//!   Session; the User running it from a terminal needs none.
//!
//! STUB: the test Job pins the contract; the implementation Job fills these bodies.

use clap::Subcommand;

use crate::error::CpdError;
use crate::output::Output;

/// The `cpd host` commands (commands.md §9.11).
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Record the User's approval for one checkout repair (adapter) (§9.11)
    Approve,
    /// Repair the skipped cpd install (Manager) (§9.11)
    Repair,
}

/// Runs a `host` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Approve => todo!("cpd host approve"),
        Command::Repair => todo!("cpd host repair"),
    }
}
