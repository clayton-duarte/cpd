//! Pull requests (contract §7.6; commands.md §9). Filled in by Job `rs-pr`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Push the Lead Branch and open the PR (§9.1)
    Open(Pending),
    /// Poll the PR for feedback and the merge (§9.2)
    Poll(Pending),
    /// Rebase the Lead Branch onto the base and push (§9.3)
    Rebase(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match command {
        Command::Open(_) => "pr open",
        Command::Poll(_) => "pr poll",
        Command::Rebase(_) => "pr rebase",
    }))
}
