//! The Plan's Workflows (contract §3.8, §7.6; commands.md §9.4–§9.7): the object before and after it starts.
//!
//! - `add`, `edit`, `describe`, `cancel`, `start`: `workflow add|edit|describe|cancel|start`;
//! - `row`: the Plan as the Workflow commands read it.
//!
//! The pull request side (`open`, `poll`, `view`, `reply`, `rerun`, `rebase`, `push`, `merge`, `gh`, `status`) stays in [`crate::pr`].

use std::path::PathBuf;

use crate::args::{PlanArg, WorkflowTitle};
use crate::error::CpdError;
use crate::ids::WorkflowKey;
use crate::output::Output;

pub(crate) mod add;
pub(crate) mod cancel;
pub(crate) mod describe;
pub(crate) mod edit;
pub(crate) mod reopen;
pub(crate) mod row;
pub mod start;

#[cfg(test)]
pub(crate) mod reopen_tests;

/// The `workflow` group (commands.md §9): the Plan's Workflows, before and after they start.
#[derive(clap::Subcommand, Debug)]
pub enum WorkflowCommand {
    /// Add a Workflow to the Plan (§9.4)
    Add(AddArgs),
    /// Change a waiting Workflow before the lock (§9.5)
    Edit(EditArgs),
    /// Store the Workflow's description, which becomes the pull request body (§9.5a)
    Describe(DescribeArgs),
    /// Remove a Workflow from the Plan before the lock; after it, end a Workflow with nothing to open, or whose work another Workflow carried (`--superseded-by`) (§9.6)
    Cancel(CancelArgs),
    /// Start a waiting Workflow whose needs are merged, after a failed start (§9.7)
    Start(StartArgs),
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// The Workflow key, chosen by the Lead
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// The Workflow's repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: PathBuf,
    /// The Workflow's own title: names its change; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: WorkflowTitle,
    /// The Workflow's own Jira ticket key, e.g. ABC-123 (optional)
    #[arg(long, value_name = "KEY")]
    jira: Option<crate::context::Jira>,
    /// The branch the pull request targets [default: origin's HEAD branch, else main], or <key> (or <plan>/<key>) to stack on that Workflow
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// Workflows that must be merged before this one starts, comma-separated: a key of the Plan, or <plan>/<key> of another Plan of the Context
    #[arg(
        long,
        value_name = "r1,r2",
        value_delimiter = ',',
        conflicts_with = "no_needs"
    )]
    needs: Vec<add::WorkflowRef>,
    /// No needs, even on a reopened Plan (default there: every merged Workflow of the Plan)
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// The template of stub drafts the Workflow starts with, and the kinds its Jobs may have: tdd (docs, red tests, implementation), docs or config (§9.4)
    #[arg(long, value_name = "shape", value_parser = clap::builder::PossibleValuesParser::new(add::SHAPES), default_value = "tdd")]
    shape: String,
    /// After the lock: the template's Jobs as decided work, a JSON object keyed by the template suffix, each `{"tier", "spec"}` (§9.4)
    #[arg(long, value_name = "file.json")]
    jobs: Option<PathBuf>,
    /// Adopt the existing pull request <number> of the repo: its head branch is pushed to, its base is the pull request's (§9.4)
    #[arg(long, value_name = "number", conflicts_with = "base", value_parser = clap::value_parser!(i64).range(1..))]
    adopt: Option<i64>,
    /// With --adopt: the login of the pull request's author when the User is not the author (confirmed by the User first)
    #[arg(long = "confirm-author", value_name = "login", requires = "adopt")]
    confirm_author: Option<String>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct EditArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// The new repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: Option<PathBuf>,
    /// The new base branch, or <key> (or <plan>/<key>) to stack on that Workflow
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// The new needs, replacing the list, comma-separated: keys of the Plan or <plan>/<key>
    #[arg(
        long,
        value_name = "r1,r2",
        value_delimiter = ',',
        conflicts_with = "no_needs"
    )]
    needs: Option<Vec<add::WorkflowRef>>,
    /// Remove every need
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// The new title, in any phase while the Workflow is not merged or cancelled; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: Option<WorkflowTitle>,
    /// The Workflow's Jira ticket key, e.g. ABC-123: sets or replaces it, in any phase while the Workflow is not merged or cancelled
    #[arg(long, value_name = "KEY")]
    jira: Option<crate::context::Jira>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct DescribeArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// The description: a path to a UTF-8 file, or - for stdin
    #[arg(long, value_name = "path")]
    file: PathBuf,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// Cancel a dispatch Workflow whose merged work another Workflow carried: that Workflow, as <plan>/<key> (a bare <key> is of the Plan)
    #[arg(long = "superseded-by", value_name = "ref")]
    superseded_by: Option<add::WorkflowRef>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    #[command(flatten)]
    plan: PlanArg,
}

/// Runs `command` of the `workflow` group (commands.md §9.4–§9.7).
pub fn run_workflow(command: WorkflowCommand) -> Result<Output, CpdError> {
    match command {
        WorkflowCommand::Add(args) => add::add(args),
        WorkflowCommand::Edit(args) => edit::edit(args),
        WorkflowCommand::Describe(args) => describe::describe(args),
        WorkflowCommand::Cancel(args) => cancel::cancel(args),
        WorkflowCommand::Start(args) => start::run(args),
    }
}
