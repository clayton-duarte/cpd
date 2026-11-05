//! Archiving Sessions (contract §2.2 "Archived"; commands.md §10.10, §10.11):
//! `session archive` and `unarchive`, and the archive rule that `plan archive`
//! and `context archive` share.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::moves::{Closed, close_lead, close_own, park_lead};
use super::row::{self, Row};
use super::state::scoped;
use super::{ArchiveArgs, Env, UnarchiveArgs, not_a_lead};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::chat_slot;
use crate::ids::{PlanId, SessionId};
use crate::output::Output;
use crate::plan::archive::{Objects, archive_keeping, refuse_agent_caller};

/// One live object that stops an archive (contract §2.2 "Archived").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocker {
    /// `session`, `job`, `escalation`, `review`, `plan`, `workflow` or `inbox`.
    pub kind: &'static str,
    /// The object's id: the Session's id, the Job's key, the Escalation's, the Plan review's or the Inbox item's id, the Plan's id, the Workflow's key.
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
/// and `plans`, read in `conn`'s one transaction. A Session `working`,
/// `waiting` or `blocked`; a Job `queued` or `running`, an open Escalation and a pending
/// Plan review of those Plans. Empty means the set may be archived.
pub fn blockers(
    conn: &Connection,
    sessions: &[SessionId],
    plans: &[PlanId],
) -> Result<Vec<Blocker>, CpdError> {
    let mut found = Vec::new();
    let mut stmt = conn.prepare("SELECT name, state FROM sessions WHERE id = ?1 AND state IN ('working', 'waiting', 'blocked')")?;
    for id in sessions {
        let live: Option<(String, String)> = stmt
            .query_row([id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((name, state)) = live {
            found.push(Blocker {
                kind: "session",
                id: id.to_string(),
                plan: None,
                reason: format!("Session {name} is {state}."),
            });
        }
    }
    let query = |sql: &str,
                 kind: &'static str,
                 plan: &PlanId,
                 found: &mut Vec<Blocker>|
     -> Result<(), CpdError> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([plan.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, status) = row?;
            let reason = match kind {
                "job" => format!("Job {id} of Plan {plan} is {status}."),
                "escalation" => format!("Escalation {id} of Plan {plan} is open."),
                _ => format!("Plan {plan}'s review {id} is pending."),
            };
            found.push(Blocker {
                kind,
                id,
                plan: Some(plan.to_string()),
                reason,
            });
        }
        Ok(())
    };
    for plan in plans {
        query(
            "SELECT key, status FROM jobs WHERE plan = ?1 AND status IN ('queued', 'running') ORDER BY seq",
            "job",
            plan,
            &mut found,
        )?;
        let mut stmt =
            conn.prepare("SELECT key FROM jobs WHERE plan = ?1 AND status = 'draft' ORDER BY seq")?;
        for key in stmt.query_map([plan.as_str()], |r| r.get::<_, String>(0))? {
            found.push(Blocker {
                kind: "job",
                id: format!("{plan}/{}", key?),
                plan: None,
                reason: "draft".to_owned(),
            });
        }
        query(
            "SELECT id, status FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id",
            "escalation",
            plan,
            &mut found,
        )?;
        query(
            "SELECT id, status FROM plan_reviews WHERE plan = ?1 AND status = 'pending' ORDER BY number",
            "review",
            plan,
            &mut found,
        )?;
    }
    Ok(found)
}

/// The archive rule for archiving (contract §2.2 "Archived"): [`blockers`] plus, per Plan, a Workflow
/// not `merged` or `cancelled` (or cancelled with an open pull request) and a `failed` Job. Plan phase and Inbox items never block.
pub fn archive_blockers(
    conn: &Connection,
    sessions: &[SessionId],
    plans: &[PlanId],
) -> Result<Vec<Blocker>, CpdError> {
    let mut found = blockers(conn, sessions, plans)?;
    for plan in plans {
        let mut stmt = conn.prepare("SELECT key, status FROM workflows WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled') ORDER BY key")?;
        let rows = stmt
            .query_map([plan.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (key, status) in rows {
            found.push(Blocker {
                kind: "workflow",
                reason: format!("Workflow {key} of Plan {plan} is {status}."),
                id: key,
                plan: Some(plan.to_string()),
            });
        }
        let mut stmt = conn.prepare("SELECT w.key FROM workflows w JOIN pull_requests r ON r.id = w.pull_request WHERE w.plan = ?1 AND w.status = 'cancelled' AND r.state = 'open' ORDER BY w.key")?;
        let rows = stmt
            .query_map([plan.as_str()], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for key in rows {
            found.push(Blocker {
                kind: "workflow",
                reason: format!(
                    "Workflow {key} of Plan {plan} is cancelled with an open pull request."
                ),
                id: key,
                plan: Some(plan.to_string()),
            });
        }
        let mut stmt = conn
            .prepare("SELECT key FROM jobs WHERE plan = ?1 AND status = 'failed' ORDER BY seq")?;
        let rows = stmt
            .query_map([plan.as_str()], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for key in rows {
            found.push(Blocker {
                kind: "job",
                reason: format!("Job {key} of Plan {plan} is failed."),
                id: key,
                plan: Some(plan.to_string()),
            });
        }
    }
    Ok(found)
}

/// `archive_blocked` for a non-empty list of [`blockers`]; `details.next` is
/// `next` when there is a command that helps.
pub fn archive_blocked(blockers: &[Blocker], next: Option<&str>) -> CpdError {
    let list = blockers
        .iter()
        .filter(|b| b.reason != "draft")
        .map(|b| b.reason.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let drafts = blockers
        .iter()
        .filter(|b| b.reason == "draft")
        .map(|b| b.id.as_str())
        .collect::<Vec<_>>();
    let mut message = format!("Something is still live: {list}");
    if !drafts.is_empty() {
        message = format!(
            "{message} Draft Jobs: {}. A draft is never archived: `cpd plan cancel` abandons a Plan and `cpd job cancel` one draft.",
            drafts.join(", ")
        );
    }
    let mut error = CpdError::new(ErrorCode::ArchiveBlocked, message).with(
        "blockers",
        Value::Array(blockers.iter().map(Blocker::to_json).collect()),
    );
    if let Some(next) = next {
        error = error.with("next", next);
    }
    error
}

/// `archived` for Session `row`: its own `archived_at` is set.
pub(super) fn archived(row: &Row) -> CpdError {
    CpdError::new(
        ErrorCode::Archived,
        format!("Session {} is archived: unarchive it first.", row.name),
    )
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
    let text = if archived {
        format!("Session {} archived.", row.name)
    } else {
        format!("Session {} unarchived.", row.name)
    };
    scoped(NewEvent::session_archived(text, archived), row)
}

/// `session archive` (commands.md §10.10).
pub(super) fn archive(args: ArchiveArgs) -> Result<Output, CpdError> {
    refuse_agent_caller("session archive")?;
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let lead = env.store.read(|conn| row::load(conn, &id))?;
    require_lead(&lead)?;
    archive_lead(&mut env, &lead, false)
}

/// The archive of Lead `lead` (a side fork, or a Lead with no Plan left by migration 0010): the rules of §10.10, then its
/// pane is closed and the row written. With `keep` its pane is left alone: the caller runs in
/// it and closes it itself, after the row is written (`session run`, commands.md §10.8).
pub(super) fn archive_lead(env: &mut Env, lead: &Row, keep: bool) -> Result<Output, CpdError> {
    if lead.archived_at.is_some() {
        return Ok(Output::success(
            json!({"archived": [], "closed": [], "already": true}),
        ));
    }
    if let (true, Some(plan)) = (lead.plan_lead, &lead.plan) {
        return Err(CpdError::new(
            ErrorCode::ArchiveBlocked,
            format!(
                "Lead {} is the Lead of Plan {plan}: it is archived with its Plan.",
                lead.name
            ),
        )
        .with("session", lead.id.as_str())
        .with("next", format!("cpd plan archive --plan {plan}")));
    }
    let ids = [lead.id.clone()];
    let live = env.store.read(|conn| archive_blockers(conn, &ids, &[]))?;
    if !live.is_empty() {
        return Err(archive_blocked(&live, None));
    }

    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let closed = if keep {
        Closed {
            pane: None,
            already: true,
        }
    } else {
        close_lead(&mut env.store, &tmux, &cockpit, &lead.id, "session archive")?
    };

    let changed = env.store.write("session archive", |tx| {
        let live = archive_blockers(tx, &ids, &[])?;
        if !live.is_empty() {
            return Err(archive_blocked(&live, None));
        }
        let changed = tx.execute(
            "UPDATE sessions SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn()), asleep_at = NULL WHERE id = ?1 AND archived_at IS NULL",
            [lead.id.as_str()],
        )?;
        if changed == 0 {
            return Ok(false);
        }
        append(tx, archived_event(lead, true))?;
        Ok(true)
    })?;
    if !changed {
        return Ok(Output::success(
            json!({"archived": [], "closed": [], "already": true}),
        ));
    }
    let closed: Vec<String> = closed
        .pane
        .into_iter()
        .filter(|_| !closed.already)
        .collect();
    Ok(Output::success(
        json!({"archived": [{"kind": "session", "id": lead.id}], "closed": closed}),
    ))
}

/// E12, the Worker archive (contract §3.7): archives every stopped, unarchived Worker Session whose Attempt is `ended`,
/// one transaction and one `session-archived` Event each, as `session archive` writes them. Returns the ids archived.
pub fn archive_ended_workers(store: &mut crate::store::Store) -> Result<Vec<SessionId>, CpdError> {
    const WHERE: &str = "s.role = 'worker' AND s.archived_at IS NULL AND s.state = 'stopped'
        AND EXISTS (SELECT 1 FROM attempts a WHERE a.plan = s.plan AND a.id = s.attempt AND a.status = 'ended')";
    let ids: Vec<String> = store.read(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT s.id FROM sessions s WHERE {WHERE} ORDER BY s.id"
        ))?;
        let ids = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    })?;
    let mut done = Vec::new();
    for id in ids {
        let id = SessionId::parse(&id)?;
        let archived = store.write("session archive", |tx| {
            let changed = tx.execute(
                &format!("UPDATE sessions AS s SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE s.id = ?1 AND {WHERE}"),
                [id.as_str()],
            )?;
            if changed == 0 {
                return Ok(false);
            }
            let row = row::load(tx, &id)?;
            append(tx, archived_event(&row, true))?;
            Ok(true)
        })?;
        if archived {
            done.push(id);
        }
    }
    Ok(done)
}

/// The archive when the User quits pi in Lead `id`'s pane (commands.md §10.8), called by
/// `session run` in that pane, in-process: no agent_forbidden guard applies. Refused (`archive_blocked`) before
/// anything moves when something is live. Else: the Lead is parked when it holds the chat slot, so the placeholder
/// takes the slot; then the archive of a Plan's own Lead (`plan archive`, forks included) or of any other Lead
/// (`session archive`) writes its rows and closes the other panes; the wrapper's own pane is closed last. Returns
/// only when that pane could not be closed.
pub(super) fn archive_on_quit(env: &mut Env, id: &SessionId) -> Result<(), CpdError> {
    let lead = env.store.read(|conn| row::load(conn, id))?;
    let own_plan = lead.plan.clone().filter(|_| lead.plan_lead);
    let objects = match &own_plan {
        Some(plan) => env.store.read(|conn| Objects::of_plan(conn, plan))?,
        None => Objects {
            context: None,
            plans: Vec::new(),
            sessions: vec![lead.id.clone()],
        },
    };
    let live = env
        .store
        .read(|conn| archive_blockers(conn, &objects.sessions, &objects.plans))?;
    if !live.is_empty() {
        return Err(archive_blocked(&live, None));
    }

    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let panes = tmux.list_panes()?;
    if let Some(slot) =
        chat_slot(&panes, &cockpit).filter(|p| p.cpd_session.as_ref() == Some(&lead.id))
    {
        park_lead(env, &tmux, &cockpit, &lead, slot, "session park")?;
    }
    match own_plan {
        Some(_) => {
            archive_keeping(
                &mut env.store,
                &env.config,
                "plan archive",
                &objects,
                Some(&lead.id),
            )?;
        }
        None => {
            // Re-read: parking moved it.
            let lead = env.store.read(|conn| row::load(conn, id))?;
            archive_lead(env, &lead, true)?;
        }
    }
    close_own(env, &lead.id, "session archive")
}

/// `session unarchive` (commands.md §10.11).
pub(super) fn unarchive(args: UnarchiveArgs) -> Result<Output, CpdError> {
    refuse_agent_caller("session unarchive")?;
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let lead = env.store.read(|conn| row::load(conn, &id))?;
    require_lead(&lead)?;
    if lead.archived_at.is_none() {
        return Ok(Output::success(
            json!({"archived": [], "closed": [], "already": true}),
        ));
    }
    let (context_archived, plan_archived) = env.store.read(|conn| {
        let context: Option<String> = conn.query_row(
            "SELECT archived_at FROM contexts WHERE id = ?1",
            [lead.context.as_str()],
            |r| r.get(0),
        )?;
        let plan: Option<String> = match &lead.plan {
            Some(plan) => conn.query_row(
                "SELECT archived_at FROM plans WHERE id = ?1",
                [plan.as_str()],
                |r| r.get(0),
            )?,
            None => None,
        };
        Ok((context.is_some(), plan.is_some()))
    })?;
    if context_archived {
        return Err(parent_archived(
            &lead,
            format!("Context {} is archived", lead.context),
            format!("cpd context unarchive {}", lead.context),
        ));
    }
    if let (true, Some(plan)) = (plan_archived, &lead.plan) {
        return Err(parent_archived(
            &lead,
            format!("Plan {plan} is archived"),
            format!("cpd plan unarchive --plan {plan}"),
        ));
    }
    let changed = env.store.write("session unarchive", |tx| {
        let changed = tx.execute("UPDATE sessions SET archived_at = NULL, asleep_at = NULL WHERE id = ?1 AND archived_at IS NOT NULL", [lead.id.as_str()])?;
        if changed == 0 {
            return Ok(false);
        }
        append(tx, archived_event(&lead, false))?;
        Ok(true)
    })?;
    if !changed {
        return Ok(Output::success(
            json!({"archived": [], "closed": [], "already": true}),
        ));
    }
    Ok(Output::success(
        json!({"archived": [{"kind": "session", "id": lead.id}], "closed": []}),
    ))
}

fn parent_archived(lead: &Row, what: String, next: String) -> CpdError {
    CpdError::new(
        ErrorCode::Archived,
        format!("{what}: unarchive it before Session {}.", lead.name),
    )
    .with("session", lead.id.as_str())
    .with("next", next)
}
