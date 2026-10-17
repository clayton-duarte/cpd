//! The Plan and its PRs (`prs`, R34 A6) as `pr open`, `pr poll` and `pr rebase`
//! read them: parsed once, so what follows holds valid values.

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::args::PrArg;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{PlanId, PrKey};
use crate::plan::Phase;
use crate::plan::fingerprint::PrSpec;

/// The pull request of a PR that is `open` or `merged` (contract §2.2 PR `pr`).
#[derive(Clone, Debug)]
pub(super) struct Pull {
    pub number: i64,
    pub url: String,
    pub opened_at: String,
}

impl Pull {
    /// `{number, url, opened_at}`, as the commands print it.
    pub fn to_json(&self) -> Value {
        json!({"number": self.number, "url": self.url, "opened_at": self.opened_at})
    }
}

/// One PR of the Plan.
#[derive(Clone, Debug)]
pub(super) struct PrRow {
    pub key: PrKey,
    pub repo: PathBuf,
    pub base: String,
    pub lead_branch: String,
    lead_worktree: Option<PathBuf>,
    /// `waiting`, `dispatch`, `open`, `merged` or `cancelled` (§3.8).
    pub status: String,
    pub pull: Option<Pull>,
    /// `pr_seen` exactly as stored, for the compare-and-swap of `pr poll`.
    pub pr_seen: String,
}

impl PrRow {
    /// The PR's Lead worktree: set from the status `dispatch` on (`prs` CHECK).
    pub fn lead(&self, plan: &PlanId) -> Result<&PathBuf, CpdError> {
        self.lead_worktree.as_ref().ok_or_else(|| bad_row(format!("PR {} of Plan {plan} has no Lead worktree", self.key)))
    }

    /// `{key, status, pr: {number, url, opened_at}}`, as the commands print the PR.
    pub fn to_json(&self) -> Value {
        json!({"key": self.key, "status": self.status, "pr": self.pull.as_ref().map(Pull::to_json)})
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

#[derive(Clone, Debug)]
pub(super) struct PlanRow {
    pub id: PlanId,
    pub title: String,
    pub phase: Phase,
    /// Every PR in key order, `cancelled` ones included.
    pub prs: Vec<PrRow>,
}

fn bad_row(what: String) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: {what}."))
}

impl PlanRow {
    /// The PR `--pr` names, else the Plan's only PR that is not `cancelled` (§1.3).
    pub fn select(&self, arg: &PrArg) -> Result<&PrRow, CpdError> {
        let specs: Vec<PrSpec> = self
            .prs
            .iter()
            .map(|p| PrSpec { key: p.key.to_string(), repo: String::new(), base: p.base.clone(), needs: Vec::new(), status: p.status.clone() })
            .collect();
        let key = arg.select(&self.id, &specs)?.key.clone();
        Ok(self.get(&key))
    }

    /// The PR `key`, which `self` has.
    fn get(&self, key: &str) -> &PrRow {
        self.prs.iter().find(|p| p.key.as_str() == key).expect("a key taken from the Plan's own PRs")
    }

    /// The PRs that are not `cancelled`.
    pub fn live(&self) -> impl Iterator<Item = &PrRow> {
        self.prs.iter().filter(|p| p.status != "cancelled")
    }
}

type Raw = (String, String, String, String, Option<String>, String, (Option<i64>, Option<String>, Option<String>), String);

/// The Plan `id` with its PRs, else `plan_not_found`.
pub(super) fn load(conn: &Connection, id: &PlanId) -> Result<PlanRow, CpdError> {
    let (title, phase): (String, String) = conn
        .query_row("SELECT title, phase FROM plans WHERE id = ?1", [id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {id}.")).with("plan", id.as_str()),
            e => e.into(),
        })?;
    let phase = Phase::parse(&phase).ok_or_else(|| bad_row(format!("Plan {id} has phase {phase:?}")))?;
    let mut stmt = conn.prepare(
        "SELECT key, repo, base, lead_branch, lead_worktree, status, pr_number, pr_url, pr_opened_at, pr_seen
           FROM prs WHERE plan = ?1 ORDER BY key",
    )?;
    let raw: Vec<Raw> = stmt
        .query_map([id.as_str()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                (r.get(6)?, r.get(7)?, r.get(8)?),
                r.get(9)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let prs = raw
        .into_iter()
        .map(|(key, repo, base, lead_branch, lead_worktree, status, pull, pr_seen)| {
            let key = PrKey::parse(&key).map_err(|_| bad_row(format!("Plan {id} has a PR key {key:?}")))?;
            let pull = match pull {
                (Some(number), Some(url), Some(opened_at)) => Some(Pull { number, url, opened_at }),
                _ => None,
            };
            Ok(PrRow { key, repo: PathBuf::from(repo), base, lead_branch, lead_worktree: lead_worktree.map(PathBuf::from), status, pull, pr_seen })
        })
        .collect::<Result<Vec<_>, CpdError>>()?;
    Ok(PlanRow { id: id.clone(), title, phase, prs })
}
