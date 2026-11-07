//! The `plans` row as read (contract §2.2 Plan), and the phases of §3.1.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::outline::Outline;
use crate::error::CpdError;
use crate::ids::{ContextId, PlanId, SessionId};

/// A Plan's phase (§3.1).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    Shaping,
    Grounding,
    Plan,
    Dispatch,
    Deploy,
    Done,
}

impl Phase {
    pub const ALL: [Phase; 6] = [
        Phase::Shaping,
        Phase::Grounding,
        Phase::Plan,
        Phase::Dispatch,
        Phase::Deploy,
        Phase::Done,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Shaping => "shaping",
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

/// One Plan, parsed at the boundary: the `plans` row alone, so a Plan with no
/// Workflow (just created) is found too.
#[derive(Clone, Debug)]
pub struct PlanRow {
    pub id: PlanId,
    pub context: ContextId,
    pub title: String,
    pub jira: Option<String>,
    pub phase: Phase,
    /// The Plan's Lead Session; none while the Plan is Lead-less (contract §2.2, air-d19, air-d22).
    pub lead_session: Option<SessionId>,
    pub created_at: String,
    pub locked_at: Option<String>,
    pub done_at: Option<String>,
    /// When it was archived (contract §2.2 "Archived"), else none.
    pub archived_at: Option<String>,
    /// The Plan's outline (§2.2, commands.md §4.13), parsed from `plans.outline`; none until `plan outline`.
    pub outline: Option<Outline>,
}

const COLUMNS: &str = "id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at, outline";

/// The columns as SQLite returns them, before parsing.
type Raw = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<String>,
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
    ))
}

impl PlanRow {
    fn parse(raw: Raw) -> Result<PlanRow, CpdError> {
        let (
            id,
            context,
            title,
            jira,
            phase,
            lead_session,
            created_at,
            locked_at,
            done_at,
            archived_at,
            outline,
        ) = raw;
        let id = PlanId::parse(&id).map_err(|_| CpdError::bad_row(format!("Plan id {id:?}")))?;
        let field =
            |name: &str, value: &str| CpdError::bad_row(format!("Plan {id} has {name} {value:?}"));
        Ok(PlanRow {
            context: ContextId::parse(&context).map_err(|_| field("context", &context))?,
            phase: Phase::parse(&phase).ok_or_else(|| field("phase", &phase))?,
            lead_session: lead_session
                .as_deref()
                .map(SessionId::parse)
                .transpose()
                .map_err(|_| field("lead_session", lead_session.as_deref().unwrap_or("")))?,
            outline: outline
                .map(|text| Outline::stored(id.as_str(), &text))
                .transpose()?,
            title,
            jira,
            created_at,
            locked_at,
            done_at,
            archived_at,
            id,
        })
    }

    /// Every field of §2.2 Plan except `prs`, as `plan show` prints them. `reviewed_pr` is null here: it needs a
    /// read of the store, so `plan show` sets it with [`reviewed_pr`].
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "context": self.context, "title": self.title, "jira": self.jira, "phase": self.phase.as_str(),
            "lead_session": self.lead_session, "created_at": self.created_at, "locked_at": self.locked_at, "done_at": self.done_at,
            "archived_at": self.archived_at, "outline": self.outline.as_ref().map(Outline::to_value), "reviewed_pr": Value::Null,
        })
    }
}

/// The pull request a review Plan reviews (contract §2.2 Plan `reviewed_pr`): null, or `{repo, number, url, title, state}`,
/// the `state` read from the `pull_requests` row of `(repo, number)` now, null when there is none.
pub fn reviewed_pr(conn: &Connection, plan: &PlanId) -> Result<Value, CpdError> {
    type Stored = (Option<String>, Option<i64>, Option<String>);
    let (repo, number, title): Stored = conn.query_row(
        "SELECT reviewed_repo, reviewed_number, reviewed_title FROM plans WHERE id = ?1",
        [plan.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let (repo, number, title) = match (repo, number, title) {
        (None, None, None) => return Ok(Value::Null),
        (Some(repo), Some(number), Some(title)) => (repo, number, title),
        _ => {
            return Err(CpdError::bad_row(format!(
                "Plan {plan} has a partial reviewed pull request"
            )));
        }
    };
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM pull_requests WHERE repo = ?1 AND number = ?2",
            rusqlite::params![repo, number],
            |r| r.get(0),
        )
        .optional()?;
    let url = crate::inbox::review_plan::pr_url(&repo, number.unsigned_abs());
    Ok(json!({"repo": repo, "number": number, "url": url, "title": title, "state": state}))
}

/// Every Plan matching `filter` (SQL after `WHERE`), oldest first; a deleted Plan is no Plan (contract §2.2 "Deleted").
pub(super) fn query(
    conn: &Connection,
    filter: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<PlanRow>, CpdError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM plans WHERE deleted_at IS NULL AND ({filter}) ORDER BY created_at, id"))?;
    let rows = stmt
        .query_map(params, raw)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(PlanRow::parse).collect()
}

/// The Plan `id`, if it exists and is not deleted.
pub fn find(conn: &Connection, id: &PlanId) -> Result<Option<PlanRow>, CpdError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM plans WHERE id = ?1 AND deleted_at IS NULL"),
            [id.as_str()],
            raw,
        )
        .optional()?;
    raw.map(PlanRow::parse).transpose()
}

/// The Plan `id`, else `plan_not_found`.
pub fn load(conn: &Connection, id: &PlanId) -> Result<PlanRow, CpdError> {
    find(conn, id)?.ok_or_else(|| CpdError::plan_not_found(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    /// commands-G3: the one Plan row is the Plan as `plan show` prints it
    /// (§2.2 Plan except `prs`), with or without a Workflow.
    #[test]
    fn the_plan_row_is_the_plans_row_alone() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, jira, phase, lead_session, created_at)
                       VALUES ('pay', 'ctx', 'Pay', 'PAY-1', 'grounding', 'lead1', 't');",
                )?;
                Ok(())
            })
            .unwrap();
        let id = PlanId::parse("pay").unwrap();
        let plan = store.read(|conn| load(conn, &id)).unwrap();
        assert_eq!(
            plan.to_json(),
            json!({"id": "pay", "context": "ctx", "title": "Pay", "jira": "PAY-1", "phase": "grounding", "lead_session": "lead1",
                   "created_at": "t", "locked_at": null, "done_at": null, "archived_at": null, "outline": null, "reviewed_pr": null})
        );
        let all = store
            .read(|conn| query(conn, "context = ?1", &[&"ctx"]))
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].to_json(), plan.to_json());
    }
}
