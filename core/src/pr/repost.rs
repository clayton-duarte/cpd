//! `pr repost` (commands.md §9.8b2): one reminder in the Slack thread of the Workflow's post. Stub: not implemented yet.

use crate::args::{PlanArg, WorkflowArg};
use crate::error::CpdError;
use crate::output::Output;
use crate::ports::Ports;

#[derive(clap::Args, Debug)]
pub struct RepostArgs {
    #[command(flatten)]
    workflow: WorkflowArg,
    #[command(flatten)]
    plan: PlanArg,
}

pub(super) fn repost(_ports: &Ports, _args: RepostArgs) -> Result<Output, CpdError> {
    todo!("pr repost (commands.md §9.8b2)")
}
