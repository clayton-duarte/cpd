//! Plans and their phases (contract §2.2, §3.1; commands.md §4).
//! Filled in by Job `rs-plan`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Create a Plan and start its Lead (§4.1)
    New(Pending),
    /// List the Plans (§4.2)
    List(Pending),
    /// Show a Plan: the TUI's snapshot (§4.3)
    Show(Pending),
    /// Move the Plan to phase `plan` (§4.4)
    Phase(Pending),
    /// Render the Plan to run/<plan>/plan.md (§4.5)
    Render(Pending),
    /// Request the User's approval of the draft Jobs (§4.6)
    Review(Pending),
    /// Record the User's decision on a Plan review (§4.7)
    Decide(Pending),
    /// Lock the Plan and start dispatching (§4.8)
    Lock(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match command {
        Command::New(_) => "plan new",
        Command::List(_) => "plan list",
        Command::Show(_) => "plan show",
        Command::Phase(_) => "plan phase",
        Command::Render(_) => "plan render",
        Command::Review(_) => "plan review",
        Command::Decide(_) => "plan decide",
        Command::Lock(_) => "plan lock",
    }))
}
