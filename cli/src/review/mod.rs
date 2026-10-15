//! Review (commands.md §7, §16.2, §16.3): the review files and brief, the
//! Reviewer, the verdict, and the Lead's `review decide`. Filled in by Job `rs-review`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Approve or reject an Attempt awaiting the Lead (§7.1)
    Decide(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match command {
        Command::Decide(_) => "review decide",
    }))
}
