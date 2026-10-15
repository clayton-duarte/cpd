//! Contexts and their records (contract §2.2, §4.6; commands.md §3).
//! Filled in by Job `rs-context`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Create a Context (§3.1)
    New(Pending),
    /// List the Contexts (§3.2)
    List(Pending),
    /// Show a Context and its records (§3.3)
    Show(Pending),
    /// Record a decision (§3.4)
    Decide(Pending),
    /// Record a note (§3.4)
    Note(Pending),
    /// Record a question (§3.4)
    Ask(Pending),
    /// Record the answer to a question (§3.5)
    Answer(Pending),
    /// Sync the Context stores with their remotes (§3.6)
    Sync(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match command {
        Command::New(_) => "context new",
        Command::List(_) => "context list",
        Command::Show(_) => "context show",
        Command::Decide(_) => "context decide",
        Command::Note(_) => "context note",
        Command::Ask(_) => "context ask",
        Command::Answer(_) => "context answer",
        Command::Sync(_) => "context sync",
    }))
}
