//! The `plans` row as `pr` reads it: parsed once, so what follows holds valid values.

use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;
use crate::plan::Phase;

/// The PR of a Plan in phase `pr` or `done` (contract §2.2 Plan).
#[derive(Clone, Debug)]
pub(super) struct Pr {
    pub number: i64,
    pub url: String,
    pub opened_at: String,
}

impl Pr {
    /// `{number, url, opened_at}`, as `pr open` prints it.
    pub fn to_json(&self) -> Value {
        json!({"number": self.number, "url": self.url, "opened_at": self.opened_at})
    }

    /// `{number, url}`, as the Events carry it.
    pub fn short(&self) -> Value {
        json!({"number": self.number, "url": self.url})
    }
}

#[derive(Clone, Debug)]
pub(super) struct PlanRow {
    pub id: PlanId,
    pub title: String,
    pub repo: PathBuf,
    pub base: String,
    pub lead_branch: String,
    lead_worktree: Option<PathBuf>,
    pub phase: Phase,
    pub pr: Option<Pr>,
    /// `pr_seen` exactly as stored, for the compare-and-swap of `pr poll`.
    pub pr_seen: String,
}

fn bad_row(what: String) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: {what}."))
}

impl PlanRow {
    /// The Lead worktree: set in every phase from `dispatch` on (`plans` CHECK).
    pub fn lead(&self) -> Result<&PathBuf, CpdError> {
        self.lead_worktree.as_ref().ok_or_else(|| bad_row(format!("Plan {} has no Lead worktree", self.id)))
    }
}

/// The Plan `id`, else `plan_not_found`.
pub(super) fn load(conn: &Connection, id: &PlanId) -> Result<PlanRow, CpdError> {
    let raw = conn
        .query_row(
            "SELECT title, repo, base, lead_branch, lead_worktree, phase, pr_number, pr_url, pr_opened_at, pr_seen
               FROM plans WHERE id = ?1",
            [id.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    (r.get::<_, Option<i64>>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?),
                    r.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {id}.")).with("plan", id.as_str()))?;
    let (title, repo, base, lead_branch, lead_worktree, phase, pr, pr_seen) = raw;
    let phase = Phase::parse(&phase).ok_or_else(|| bad_row(format!("Plan {id} has phase {phase:?}")))?;
    let pr = match pr {
        (Some(number), Some(url), Some(opened_at)) => Some(Pr { number, url, opened_at }),
        _ => None,
    };
    Ok(PlanRow {
        id: id.clone(),
        title,
        repo: PathBuf::from(repo),
        base,
        lead_branch,
        lead_worktree: lead_worktree.map(PathBuf::from),
        phase,
        pr,
        pr_seen,
    })
}
