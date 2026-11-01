//! The Plan with its Workflows as `pr open`, `pr poll` and `pr rebase` read them, and
//! what the commands do with a Workflow row (`plan::WorkflowRow`).

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::args::WorkflowArg;
use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;
use crate::plan::fingerprint::load_workflows;
use crate::plan::{self, PlanRow, WorkflowRow, Pull};

impl WorkflowRow {
    /// The Workflow's Lead worktree: set from the status `dispatch` on (`workflows` CHECK).
    pub fn lead(&self, plan: &PlanId) -> Result<&PathBuf, CpdError> {
        self.lead_worktree.as_ref().ok_or_else(|| CpdError::bad_row(format!("Workflow {} of Plan {plan} has no Lead worktree", self.key)))
    }

    /// `{key, status, pr: {number, url, opened_at}}`, as the commands print the Workflow.
    pub fn to_json(&self) -> Value {
        json!({"key": self.key, "status": self.status, "pr": self.pull.as_ref().map(pull_json)})
    }

    /// `illegal_transition` (exit 3): `command` needs the Workflow in status `needs`.
    pub fn refuse(&self, plan: &PlanId, command: &str, needs: &str) -> CpdError {
        CpdError::new(
            ErrorCode::IllegalTransition,
            format!("Workflow {} of Plan {plan} is {}: {command} needs status {needs}.", self.key, self.status),
        )
        .with("plan", plan.as_str())
        .with("pr", self.key.as_str())
        .with("from", self.status.as_str())
        .with("needs", needs)
    }
}

/// `{number, url, opened_at}`, as the commands print the pull request.
pub(super) fn pull_json(pull: &Pull) -> Value {
    json!({"number": pull.number, "url": pull.url, "opened_at": pull.opened_at})
}

/// A Plan with its Workflows.
#[derive(Clone, Debug)]
pub(crate) struct PlanWorkflows {
    pub plan: PlanRow,
    /// Every Workflow in key order, `cancelled` ones included.
    pub workflows: Vec<WorkflowRow>,
}

impl PlanWorkflows {
    /// The Workflow `--workflow` names, else the Plan's only Workflow that is not `cancelled` (§1.3).
    pub fn select(&self, arg: &WorkflowArg) -> Result<&WorkflowRow, CpdError> {
        arg.select(&&self.plan.id, &self.workflows)
    }

    /// The Workflows that are not `cancelled`.
    pub fn live(&self) -> impl Iterator<Item = &WorkflowRow> {
        self.workflows.iter().filter(|p| p.status != "cancelled")
    }
}

/// The Plan `id` with its Workflows, else `plan_not_found`.
pub(crate) fn load(conn: &Connection, id: &PlanId) -> Result<PlanWorkflows, CpdError> {
    Ok(PlanWorkflows { plan: plan::row::load(conn, id)?, workflows: load_workflows(conn, id)? })
}
