//! The Plan with its PRs as `pr open`, `pr poll` and `pr rebase` read them, and
//! what the commands do with a PR row (`plan::PrRow`).

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::args::PrArg;
use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;
use crate::plan::fingerprint::load_prs;
use crate::plan::{Phase, PrRow, Pull};

impl PrRow {
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

#[derive(Clone, Debug)]
pub(super) struct PlanRow {
    pub id: PlanId,
    pub title: String,
    pub phase: Phase,
    /// Every PR in key order, `cancelled` ones included.
    pub prs: Vec<PrRow>,
}

impl PlanRow {
    /// The PR `--pr` names, else the Plan's only PR that is not `cancelled` (§1.3).
    pub fn select(&self, arg: &PrArg) -> Result<&PrRow, CpdError> {
        arg.select(&self.id, &self.prs)
    }

    /// The PRs that are not `cancelled`.
    pub fn live(&self) -> impl Iterator<Item = &PrRow> {
        self.prs.iter().filter(|p| p.status != "cancelled")
    }
}

/// The Plan `id` with its PRs, else `plan_not_found`.
pub(super) fn load(conn: &Connection, id: &PlanId) -> Result<PlanRow, CpdError> {
    let (title, phase): (String, String) = conn
        .query_row("SELECT title, phase FROM plans WHERE id = ?1", [id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CpdError::plan_not_found(id),
            e => e.into(),
        })?;
    let phase = Phase::parse(&phase).ok_or_else(|| CpdError::bad_row(format!("Plan {id} has phase {phase:?}")))?;
    Ok(PlanRow { id: id.clone(), title, phase, prs: load_prs(conn, id)? })
}
