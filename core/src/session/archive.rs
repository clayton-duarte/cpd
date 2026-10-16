//! Archiving Sessions (contract §2.2 "Archived"; commands.md §10.10, §10.11):
//! `session archive` and `unarchive`, and the archive rule that `plan archive`
//! and `context archive` share.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::moves::close_lead;
use super::row::{self, Row};
use super::state::scoped;
use super::{ArchiveArgs, Env, UnarchiveArgs, not_a_lead};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;

/// One live object that stops an archive (contract §2.2 "Archived").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocker {
    /// `session`, `job`, `escalation` or `review`.
    pub kind: &'static str,
    /// The object's id: the Session's id, the Job's key, the Escalation's or the Plan review's id.
    pub id: String,
    /// Its Plan, for a `job`, `escalation` or `review`.
    pub plan: Option<String>,
    pub reason: String,
}

impl Blocker {
    fn to_json(&self) -> Value {
        let mut out = json!({"kind": self.kind, "id": self.id, "reason": self.reason});
        if let Some(plan) = &self.plan {
            out["plan"] = json!(plan);
        }
        out
    }
}

/// The archive rule (contract §2.2 "Archived"): what is live among `sessions`
/// and `plans`, read in `conn`'s one transaction. A Session `working` or
/// `blocked`; a Job `queued` or `running`, an open Escalation and a pending
/// Plan review of those Plans. Empty means the set may be archived.
pub fn blockers(conn: &Connection, sessions: &[SessionId], plans: &[PlanId]) -> Result<Vec<Blocker>, CpdError> {
    let mut found = Vec::new();
    let mut stmt = conn.prepare("SELECT name, state FROM sessions WHERE id = ?1 AND state IN ('working', 'blocked')")?;
    for id in sessions {
        let live: Option<(String, String)> = stmt.query_row([id.as_str()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((name, state)) = live {
            found.push(Blocker {
                kind: "session",
                id: id.to_string(),
                plan: None,
                reason: format!("Session {name} is {state}."),
            });
        }
    }
    let query = |sql: &str, kind: &'static str, plan: &PlanId, found: &mut Vec<Blocker>| -> Result<(), CpdError> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, status) = row?;
            let reason = match kind {
                "job" => format!("Job {id} of Plan {plan} is {status}."),
                "escalation" => format!("Escalation {id} of Plan {plan} is open."),
                _ => format!("Plan {plan}'s review {id} is pending."),
            };
            found.push(Blocker { kind, id, plan: Some(plan.to_string()), reason });
        }
        Ok(())
    };
    for plan in plans {
        query("SELECT key, status FROM jobs WHERE plan = ?1 AND status IN ('queued', 'running') ORDER BY seq", "job", plan, &mut found)?;
        query("SELECT id, status FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id", "escalation", plan, &mut found)?;
        query("SELECT id, status FROM plan_reviews WHERE plan = ?1 AND status = 'pending' ORDER BY number", "review", plan, &mut found)?;
    }
    Ok(found)
}

/// `archive_blocked` for a non-empty list of [`blockers`]; `details.next` is
/// `next` when there is a command that helps.
pub fn archive_blocked(blockers: &[Blocker], next: Option<&str>) -> CpdError {
    let list = blockers.iter().map(|b| b.reason.as_str()).collect::<Vec<_>>().join(" ");
    let mut error = CpdError::new(ErrorCode::ArchiveBlocked, format!("Something is still live: {list}"))
        .with("blockers", Value::Array(blockers.iter().map(Blocker::to_json).collect()));
    if let Some(next) = next {
        error = error.with("next", next);
    }
    error
}

/// `archived` for Session `row`: its own `archived_at` is set.
pub(super) fn archived(row: &Row) -> CpdError {
    CpdError::new(ErrorCode::Archived, format!("Session {} is archived: unarchive it first.", row.name))
        .with("session", row.id.as_str())
        .with("next", format!("cpd session unarchive {}", row.id))
}

fn require_lead(row: &Row) -> Result<(), CpdError> {
    match row.role {
        row::Role::Lead => Ok(()),
        role => Err(not_a_lead(&row.id, role)),
    }
}

fn archived_event(row: &Row, archived: bool) -> NewEvent {
    let text = if archived { format!("Session {} archived.", row.name) } else { format!("Session {} unarchived.", row.name) };
    scoped(NewEvent::new(Kind::SessionArchived, text, json!({"archived": archived})), row)
}

/// `session archive` (commands.md §10.10).
pub(super) fn archive(args: ArchiveArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let lead = env.store.read(|conn| row::load(conn, &args.id))?;
    require_lead(&lead)?;
    if lead.archived_at.is_some() {
        return Ok(Output::success(json!({"archived": [], "closed": [], "already": true})));
    }
    if let (true, Some(plan)) = (lead.plan_lead, &lead.plan) {
        return Err(CpdError::new(
            ErrorCode::ArchiveBlocked,
            format!("Lead {} is the Lead of Plan {plan}: it is archived with its Plan.", lead.name),
        )
        .with("session", lead.id.as_str())
        .with("next", format!("cpd plan archive --plan {plan}")));
    }
    let ids = [lead.id.clone()];
    let live = env.store.read(|conn| blockers(conn, &ids, &[]))?;
    if !live.is_empty() {
        return Err(archive_blocked(&live, None));
    }

    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let closed = close_lead(&mut env.store, &tmux, &cockpit, &lead.id, "session archive")?;

    let changed = env.store.write("session archive", |tx| {
        let live = blockers(tx, &ids, &[])?;
        if !live.is_empty() {
            return Err(archive_blocked(&live, None));
        }
        let changed = tx.execute(
            "UPDATE sessions SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND archived_at IS NULL",
            [lead.id.as_str()],
        )?;
        if changed == 0 {
            return Ok(false);
        }
        append(tx, archived_event(&lead, true))?;
        Ok(true)
    })?;
    if !changed {
        return Ok(Output::success(json!({"archived": [], "closed": [], "already": true})));
    }
    let closed: Vec<String> = closed.pane.into_iter().filter(|_| !closed.already).collect();
    Ok(Output::success(json!({"archived": [{"kind": "session", "id": lead.id}], "closed": closed})))
}

/// `session unarchive` (commands.md §10.11).
pub(super) fn unarchive(args: UnarchiveArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let lead = env.store.read(|conn| row::load(conn, &args.id))?;
    require_lead(&lead)?;
    if lead.archived_at.is_none() {
        return Ok(Output::success(json!({"archived": [], "closed": [], "already": true})));
    }
    let (context_archived, plan_archived) = env.store.read(|conn| {
        let context: Option<String> =
            conn.query_row("SELECT archived_at FROM contexts WHERE id = ?1", [lead.context.as_str()], |r| r.get(0))?;
        let plan: Option<String> = match &lead.plan {
            Some(plan) => conn.query_row("SELECT archived_at FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?,
            None => None,
        };
        Ok((context.is_some(), plan.is_some()))
    })?;
    if context_archived {
        return Err(parent_archived(&lead, format!("Context {} is archived", lead.context), format!("cpd context unarchive {}", lead.context)));
    }
    if let (true, Some(plan)) = (plan_archived, &lead.plan) {
        return Err(parent_archived(&lead, format!("Plan {plan} is archived"), format!("cpd plan unarchive --plan {plan}")));
    }
    let changed = env.store.write("session unarchive", |tx| {
        let changed = tx.execute("UPDATE sessions SET archived_at = NULL WHERE id = ?1 AND archived_at IS NOT NULL", [lead.id.as_str()])?;
        if changed == 0 {
            return Ok(false);
        }
        append(tx, archived_event(&lead, false))?;
        Ok(true)
    })?;
    if !changed {
        return Ok(Output::success(json!({"archived": [], "closed": [], "already": true})));
    }
    Ok(Output::success(json!({"archived": [{"kind": "session", "id": lead.id}], "closed": []})))
}

fn parent_archived(lead: &Row, what: String, next: String) -> CpdError {
    CpdError::new(ErrorCode::Archived, format!("{what}: unarchive it before Session {}.", lead.name))
        .with("session", lead.id.as_str())
        .with("next", next)
}
