//! The `plans` row as read (contract §2.2 Plan), with the per-repo columns of
//! its one PR (`prs`, R34 A6), and the phases of §3.1.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::{bad_row, plan_not_found};
use crate::error::CpdError;
use crate::ids::{ContextId, PlanId, PrKey, SessionId};

/// A Plan's phase (§3.1).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Grounding,
    Plan,
    Dispatch,
    Deploy,
    Done,
}

impl Phase {
    pub const ALL: [Phase; 5] = [Phase::Grounding, Phase::Plan, Phase::Dispatch, Phase::Deploy, Phase::Done];

    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Grounding => "grounding",
            Phase::Plan => "plan",
            Phase::Dispatch => "dispatch",
            Phase::Deploy => "deploy",
            Phase::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        Phase::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// One Plan, parsed at the boundary.
#[derive(Clone, Debug)]
pub(super) struct PlanRow {
    pub id: PlanId,
    pub context: ContextId,
    pub title: String,
    pub jira: Option<String>,
    /// The key of the Plan's one PR, whose columns follow.
    pub pr_key: PrKey,
    pub repo: String,
    pub base: String,
    pub lead_branch: String,
    pub lead_worktree: Option<String>,
    pub phase: Phase,
    pub lead_session: SessionId,
    pub pr: Option<Value>,
    pub all_merged_at: Option<String>,
    pub created_at: String,
    pub locked_at: Option<String>,
    pub done_at: Option<String>,
    /// When it was archived (contract §2.2 "Archived"), else none.
    pub archived_at: Option<String>,
}

const COLUMNS: &str = "p.id, p.context, p.title, p.jira, r.key, r.repo, r.base, r.lead_branch, r.lead_worktree, p.phase, p.lead_session,
    r.pr_number, r.pr_url, r.pr_opened_at, r.all_merged_at, p.created_at, p.locked_at, p.done_at, p.archived_at";

/// `plans p` with its one PR `r` (R34: until a Plan has several PRs, the
/// first PR it got stands for the Plan's repo, branch, worktree and PR).
const PLAN_WITH_PR: &str = "plans p JOIN prs r ON r.plan = p.id
    AND r.key = (SELECT key FROM prs WHERE plan = p.id ORDER BY created_txn, key LIMIT 1)";

/// The columns as SQLite returns them, before parsing.
type Raw = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    (Option<i64>, Option<String>, Option<String>),
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn raw(r: &rusqlite::Row) -> rusqlite::Result<Raw> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        (r.get(11)?, r.get(12)?, r.get(13)?),
        r.get(14)?,
        r.get(15)?,
        r.get(16)?,
        r.get(17)?,
        r.get(18)?,
    ))
}

impl PlanRow {
    fn parse(raw: Raw) -> Result<PlanRow, CpdError> {
        let (id, context, title, jira, pr_key, repo, base, lead_branch, lead_worktree, phase, lead_session, pr, all_merged_at, created_at, locked_at, done_at, archived_at) = raw;
        let id = PlanId::parse(&id).map_err(|_| bad_row(format!("Plan id {id:?}")))?;
        let field = |name: &str, value: &str| bad_row(format!("Plan {id} has {name} {value:?}"));
        let pr = match pr {
            (Some(number), Some(url), Some(opened_at)) => Some(json!({"number": number, "url": url, "opened_at": opened_at})),
            _ => None,
        };
        Ok(PlanRow {
            context: ContextId::parse(&context).map_err(|_| field("context", &context))?,
            pr_key: PrKey::parse(&pr_key).map_err(|_| field("PR key", &pr_key))?,
            phase: Phase::parse(&phase).ok_or_else(|| field("phase", &phase))?,
            lead_session: SessionId::parse(&lead_session).map_err(|_| field("lead_session", &lead_session))?,
            title,
            jira,
            repo,
            base,
            lead_branch,
            lead_worktree,
            pr,
            all_merged_at,
            created_at,
            locked_at,
            done_at,
            archived_at,
            id,
        })
    }

    /// Every field of §2.2 Plan, as `plan show` prints it.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "context": self.context, "title": self.title, "jira": self.jira,
            "repo": self.repo, "base": self.base, "lead_branch": self.lead_branch,
            "lead_worktree": self.lead_worktree, "phase": self.phase.as_str(), "lead_session": self.lead_session,
            "pr": self.pr, "all_merged_at": self.all_merged_at,
            "created_at": self.created_at, "locked_at": self.locked_at, "done_at": self.done_at,
            "archived_at": self.archived_at,
        })
    }
}

/// Every Plan matching `filter` (SQL after `WHERE`, on `p` and `r`), oldest first.
pub(super) fn query(conn: &Connection, filter: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<PlanRow>, CpdError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM {PLAN_WITH_PR} WHERE {filter} ORDER BY p.created_at, p.id"))?;
    let rows = stmt.query_map(params, raw)?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(PlanRow::parse).collect()
}

/// The Plan `id`, if it exists.
pub(super) fn find(conn: &Connection, id: &PlanId) -> Result<Option<PlanRow>, CpdError> {
    let raw = conn
        .query_row(&format!("SELECT {COLUMNS} FROM {PLAN_WITH_PR} WHERE p.id = ?1"), [id.as_str()], raw)
        .optional()?;
    raw.map(PlanRow::parse).transpose()
}

/// The Plan `id`, else `plan_not_found`.
pub(super) fn load(conn: &Connection, id: &PlanId) -> Result<PlanRow, CpdError> {
    find(conn, id)?.ok_or_else(|| plan_not_found(id))
}
