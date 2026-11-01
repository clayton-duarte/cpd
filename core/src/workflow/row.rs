//! The Plan with its PRs as `pr open`, `pr poll` and `pr rebase` read them, and
//! what the commands do with a PR row (`plan::WorkflowRow`).

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::args::WorkflowArg;
use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;
use crate::plan::fingerprint::load_workflows;
use crate::plan::{self, PlanRow, WorkflowRow, Pull};

impl WorkflowRow {
    /// The PR's Lead worktree: set from the status `dispatch` on (`prs` CHECK).
    pub fn lead(&self, plan: &PlanId) -> Result<&PathBuf, CpdError> {
        self.lead_worktree.as_ref().ok_or_else(|| CpdError::bad_row(format!("PR {} of Plan {plan} has no Lead worktree", self.key)))
    }

    /// `{key, status, pr: {number, url, opened_at}}`, as the commands print the PR.
    pub fn to_json(&self) -> Value {
        json!({"key": self.key, "status": self.status, "pr": self.pull.as_ref().map(pull_json)})
    }

    /// `illegal_transition` (exit 3): `command` needs the PR in status `needs`.
    pub fn refuse(&self, plan: &PlanId, command: &str, needs: &str) -> CpdError {
        CpdError::new(
            ErrorCode::IllegalTransition,
            format!("PR {} of Plan {plan} is {}: {command} needs status {needs}.", self.key, self.status),
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

/// A Plan with its PRs.
#[derive(Clone, Debug)]
pub(crate) struct PlanWorkflows {
    pub plan: PlanRow,
    /// Every PR in key order, `cancelled` ones included.
    pub workflows: Vec<WorkflowRow>,
}

impl PlanWorkflows {
    /// The PR `--pr` names, else the Plan's only PR that is not `cancelled` (§1.3).
    pub fn select(&self, arg: &WorkflowArg) -> Result<&WorkflowRow, CpdError> {
        arg.select(&&self.plan.id, &self.workflows)
    }

    /// The PRs that are not `cancelled`.
    pub fn live(&self) -> impl Iterator<Item = &WorkflowRow> {
        self.workflows.iter().filter(|p| p.status != "cancelled")
    }
}

/// The Plan `id` with its PRs, else `plan_not_found`.
pub(crate) fn load(conn: &Connection, id: &PlanId) -> Result<PlanWorkflows, CpdError> {
    Ok(PlanWorkflows { plan: plan::row::load(conn, id)?, workflows: load_workflows(conn, id)? })
}
