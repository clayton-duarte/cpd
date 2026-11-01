//! `plan archive` and `plan unarchive` (commands.md §4.10, §4.11), and the
//! cascade that `context archive` and `unarchive` share (§3.8, §3.9; contract
//! §2.2 "Archived"). The archive rule and the pane closing are the session
//! area's: [`blockers`] and [`close_lead`].

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{ArchiveArgs, Env, UnarchiveArgs, row};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::{Tmux, chat_slot};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::session::{archive_blocked, blockers, close_lead};
use crate::store::Store;

/// What one archive or unarchive covers: a Context, Plans and their Lead Sessions.
#[derive(Clone, Debug)]
pub struct Objects {
    pub context: Option<ContextId>,
    pub plans: Vec<PlanId>,
    pub sessions: Vec<SessionId>,
}

/// One object of a cascade.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Object {
    Context(ContextId),
    Plan(PlanId),
    Session(SessionId),
}

impl Object {
    fn table(&self) -> &'static str {
        match self {
            Object::Context(_) => "contexts",
            Object::Plan(_) => "plans",
            Object::Session(_) => "sessions",
        }
    }

    /// What archiving and unarchiving also set: a Session loses its `asleep_at` (contract §2.2 "Asleep").
    fn wakes(&self) -> &'static str {
        match self {
            Object::Session(_) => ", asleep_at = NULL",
            _ => "",
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Object::Context(_) => "context",
            Object::Plan(_) => "plan",
            Object::Session(_) => "session",
        }
    }

    fn id(&self) -> &str {
        match self {
            Object::Context(id) => id.as_str(),
            Object::Plan(id) => id.as_str(),
            Object::Session(id) => id.as_str(),
        }
    }

    fn to_json(&self) -> Value {
        json!({"kind": self.kind(), "id": self.id()})
    }

    fn archived(&self, conn: &Connection) -> Result<bool, CpdError> {
        let sql = format!(
            "SELECT archived_at IS NOT NULL FROM {} WHERE id = ?1",
            self.table()
        );
        Ok(conn.query_row(&sql, [self.id()], |r| r.get(0))?)
    }

    /// The Event of this object's change: `archived` is the new value.
    fn event(&self, conn: &Connection, archived: bool) -> Result<NewEvent, CpdError> {
        let verb = if archived { "archived" } else { "unarchived" };
        Ok(match self {
            Object::Context(id) => {
                NewEvent::context_archived(format!("Context {id} {verb}."), archived)
                    .context(id.clone())
            }
            Object::Plan(id) => {
                NewEvent::plan_archived(format!("Plan {id} {verb}."), archived).plan(id.clone())
            }
            Object::Session(id) => {
                let (name, context, plan): (String, String, Option<String>) = conn.query_row(
                    "SELECT name, context, plan FROM sessions WHERE id = ?1",
                    [id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                let event = NewEvent::session_archived(format!("Session {name} {verb}."), archived)
                    .session(id.clone())
                    .context(ContextId::parse(&context)?);
                match plan {
                    Some(plan) => event.plan(PlanId::parse(&plan)?),
                    None => event,
                }
            }
        })
    }
}

impl Objects {
    /// The Plan, its Lead and its side forks; `plan_not_found`.
    pub fn of_plan(conn: &Connection, plan: &PlanId) -> Result<Objects, CpdError> {
        row::load(conn, plan)?;
        let sessions = session_ids(
            conn,
            "SELECT id FROM sessions WHERE role = 'lead' AND plan = ?1 ORDER BY created_at, id",
            plan.as_str(),
        )?;
        Ok(Objects {
            context: None,
            plans: vec![plan.clone()],
            sessions,
        })
    }

    /// The Context, its Plans, and every Lead and Manager Session of it.
    /// The caller has checked that the Context exists.
    pub fn of_context(conn: &Connection, context: &ContextId) -> Result<Objects, CpdError> {
        let mut stmt = conn.prepare("SELECT id FROM plans WHERE context = ?1 AND deleted_at IS NULL ORDER BY created_at, id")?;
        let plans = stmt
            .query_map([context.as_str()], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .map(|id| PlanId::parse(id))
            .collect::<Result<Vec<_>, _>>()?;
        let sessions = session_ids(
            conn,
            "SELECT id FROM sessions WHERE role IN ('lead', 'manager') AND context = ?1 ORDER BY created_at, id",
            context.as_str(),
        )?;
        Ok(Objects {
            context: Some(context.clone()),
            plans,
            sessions,
        })
    }

    /// The Context, then the Plans, then the Sessions.
    fn all(&self) -> Vec<Object> {
        let context = self.context.iter().cloned().map(Object::Context);
        context
            .chain(self.plans.iter().cloned().map(Object::Plan))
            .chain(self.sessions.iter().cloned().map(Object::Session))
            .collect()
    }
}

fn session_ids(conn: &Connection, sql: &str, id: &str) -> Result<Vec<SessionId>, CpdError> {
    let mut stmt = conn.prepare(sql)?;
    let ids = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| SessionId::parse(id)).collect()
}

fn result(changed: &[Object], closed: Vec<String>) -> Output {
    let archived: Vec<Value> = changed.iter().map(Object::to_json).collect();
    let mut out = json!({"archived": archived, "closed": closed});
    if changed.is_empty() {
        out["already"] = json!(true);
    }
    Output::success(out)
}

/// `in_chat_slot` when a Lead of `sessions` holds the chat slot: checked
/// before any pane is closed, so a refusal leaves everything as it was.
pub(crate) fn refuse_chat_slot(
    store: &mut Store,
    tmux: &Tmux,
    cockpit: &str,
    sessions: &[SessionId],
) -> Result<(), CpdError> {
    // The rows are read first, then the panes, as `close_lead` does.
    let leads = store.read(|conn| {
        let mut found = Vec::new();
        for id in sessions {
            let (name, place): (String, Option<String>) = conn.query_row(
                "SELECT name, (SELECT json_extract(e.data, '$.place') FROM events e
                                WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1)
                   FROM sessions s WHERE s.id = ?1",
                [id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            found.push((id, name, place.as_deref() == Some("chat")));
        }
        Ok(found)
    })?;
    let panes = tmux.list_panes()?;
    let slot = chat_slot(&panes, cockpit).and_then(|p| p.cpd_session.as_ref());
    for (id, name, placed) in leads {
        if placed || slot == Some(id) {
            return Err(CpdError::new(
                ErrorCode::InChatSlot,
                format!("Lead {name} is in the chat slot: park it or open another Lead first."),
            )
            .with("session", id.as_str())
            .with("next", "cpd session park"));
        }
    }
    Ok(())
}

/// The archive of `objects`, all or nothing (commands.md §3.8, §4.10): check
/// every object, close every live Lead pane, then write the rows and Events
/// in one transaction that checks the rule again.
pub fn archive(
    store: &mut Store,
    config: &Config,
    command: &str,
    objects: &Objects,
) -> Result<Output, CpdError> {
    archive_keeping(store, config, command, objects, None)
}

/// [`archive`] that leaves the pane of Lead `keep` alone: its caller runs in that pane
/// and closes it itself, last (`session run` on the User's quit, commands.md §10.8).
pub fn archive_keeping(
    store: &mut Store,
    config: &Config,
    command: &str,
    objects: &Objects,
    keep: Option<&SessionId>,
) -> Result<Output, CpdError> {
    let all = objects.all();
    let (pending, live) = store.read(|conn| {
        let mut pending = false;
        for object in &all {
            pending |= !object.archived(conn)?;
        }
        Ok((pending, blockers(conn, &objects.sessions, &objects.plans)?))
    })?;
    if !pending {
        return Ok(result(&[], Vec::new()));
    }
    if !live.is_empty() {
        return Err(archive_blocked(&live, None));
    }
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = &config.tmux.cockpit_session;
    refuse_chat_slot(store, &tmux, cockpit, &objects.sessions)?;

    let mut closed = Vec::new();
    for id in objects.sessions.iter().filter(|id| Some(*id) != keep) {
        let lead = close_lead(store, &tmux, cockpit, id, command)?;
        closed.extend(lead.pane.filter(|_| !lead.already));
    }

    let changed = store.write(command, |tx| {
        let live = blockers(tx, &objects.sessions, &objects.plans)?;
        if !live.is_empty() {
            return Err(archive_blocked(&live, None));
        }
        let mut changed = Vec::new();
        for object in &all {
            let sql = format!(
                "UPDATE {} SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn()){} WHERE id = ?1 AND archived_at IS NULL",
                object.table(),
                object.wakes()
            );
            if tx.execute(&sql, [object.id()])? > 0 {
                append(tx, object.event(tx, true)?)?;
                changed.push(object.clone());
            }
        }
        Ok(changed)
    })?;
    Ok(result(&changed, closed))
}

/// The unarchive of `objects` (commands.md §3.9, §4.11): clears `archived_at`
/// with one Event per object changed, and touches no pane. With `parent`, that
/// Context must not be archived (`archived`) when anything is to change.
pub fn unarchive(
    store: &mut Store,
    command: &str,
    objects: &Objects,
    parent: Option<&ContextId>,
) -> Result<Output, CpdError> {
    let all = objects.all();
    let changed = store.write(command, |tx| {
        let mut archived = Vec::new();
        for object in &all {
            if object.archived(tx)? {
                archived.push(object);
            }
        }
        if archived.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(context) = parent {
            let at: Option<String> = tx.query_row(
                "SELECT archived_at FROM contexts WHERE id = ?1",
                [context.as_str()],
                |r| r.get(0),
            )?;
            if at.is_some() {
                return Err(CpdError::new(
                    ErrorCode::Archived,
                    format!("Context {context} is archived: unarchive it first."),
                )
                .with("context", context.as_str())
                .with("next", format!("cpd context unarchive {context}")));
            }
        }
        let mut changed = Vec::new();
        for object in archived {
            tx.execute(
                &format!(
                    "UPDATE {} SET archived_at = NULL{} WHERE id = ?1",
                    object.table(),
                    object.wakes()
                ),
                [object.id()],
            )?;
            append(tx, object.event(tx, false)?)?;
            changed.push(object.clone());
        }
        Ok(changed)
    })?;
    Ok(result(&changed, Vec::new()))
}

/// `plan archive` (§4.10).
pub(super) fn plan_archive(args: ArchiveArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let objects = env.store.read(|conn| Objects::of_plan(conn, &id))?;
    archive(&mut env.store, &env.config, "plan archive", &objects)
}

/// `plan unarchive` (§4.11).
pub(super) fn plan_unarchive(args: UnarchiveArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (objects, context) = env
        .store
        .read(|conn| Ok((Objects::of_plan(conn, &id)?, row::load(conn, &id)?.context)))?;
    unarchive(&mut env.store, "plan unarchive", &objects, Some(&context))
}
