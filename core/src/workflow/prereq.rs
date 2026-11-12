//! Workflow external prerequisites (commands.md §9.8).
//!
//! Stubs for external prerequisites gate merge, post, deploy.

use clap::Subcommand;

use crate::args::PlanArg;
use crate::error::CpdError;
use crate::ids::WorkflowKey;
use crate::output::Output;

#[derive(Subcommand, Debug)]
pub enum PrereqCommand {
    /// Add an external prerequisite to a workflow
    Add(AddPrereqArgs),
    /// Confirm a manual prerequisite
    Confirm(ConfirmPrereqArgs),
}

#[derive(clap::Args, Debug)]
pub struct AddPrereqArgs {
    /// The Workflow key
    #[arg(long, value_name = "key")]
    pub workflow: WorkflowKey,
    /// Kind of prerequisite: ld-flag or manual
    #[arg(long, value_name = "kind")]
    pub kind: String,
    /// LaunchDarkly project key (for kind ld-flag)
    #[arg(long, value_name = "project")]
    pub project: Option<String>,
    /// LaunchDarkly flag key (for kind ld-flag)
    #[arg(long, value_name = "key")]
    pub key: Option<String>,
    /// LaunchDarkly environment (for kind ld-flag, optional)
    #[arg(long, value_name = "env")]
    pub env: Option<String>,
    /// Manual prerequisite description text (for kind manual)
    #[arg(long, value_name = "text")]
    pub text: Option<String>,
    #[command(flatten)]
    pub plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ConfirmPrereqArgs {
    /// The Workflow key
    #[arg(long, value_name = "key")]
    pub workflow: WorkflowKey,
    /// The prerequisite id
    #[arg(long, value_name = "id")]
    pub id: String,
    #[command(flatten)]
    pub plan: PlanArg,
}

/// Run a prerequisite command. Stubbed out.
pub fn run(command: PrereqCommand) -> Result<Output, CpdError> {
    match command {
        PrereqCommand::Add(_) => Err(CpdError::bad_row("workflow prereq add not yet implemented")),
        PrereqCommand::Confirm(_) => {
            Err(CpdError::bad_row("workflow prereq confirm not yet implemented"))
        }
    }
}
