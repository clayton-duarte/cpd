//! `session send` (commands.md §10.13, hs-d92, hs-d133): one message to a Lead or a Manager,
//! as a `session-message` Event its adapter delivers (§15.3).

use rusqlite::OptionalExtension;
use serde_json::json;

use super::row::{self, Role, Row};
use super::state::scoped;
use super::moves::start_parked;
use super::{Env, SendArgs, archive, no_cockpit};
use crate::args::Text;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::WriteTxn;

/// How much of the message the Event's `text` quotes.
const PREVIEW_CHARS: usize = 60;

/// The sender of a message or a post: `CPD_SESSION`, else `PI_SESSION_ID`, else none: the User.
pub fn sender() -> Result<Option<SessionId>, CpdError> {
    ["CPD_SESSION", "PI_SESSION_ID"]
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
        .map(|v| SessionId::parse(&v.to_string_lossy()))
        .transpose()
}

/// The target is a Lead or a Manager and is not archived (a `stopped` one is fine).
pub(super) fn check(target: &Row) -> Result<(), CpdError> {
    match target.role {
        Role::Lead | Role::Manager if target.archived_at.is_none() => Ok(()),
        Role::Lead | Role::Manager => Err(archive::archived(target)),
        role => Err(CpdError::new(ErrorCode::NotALead, format!("Session {} is a {}, not a Lead or a Manager.", target.id, role.as_str()))
            .with("session", target.id.as_str())
            .with("role", role.as_str())),
    }
}

/// `plan_inactive` unless `plan` is still live: a Plan in phase `done` or archived takes no message or post (hs-d507, hs-d514).
pub fn plan_inactive(conn: &rusqlite::Connection, plan: &str) -> Result<(), CpdError> {
    let (phase, archived): (String, bool) =
        conn.query_row("SELECT phase, archived_at IS NOT NULL FROM plans WHERE id = ?1", [plan], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let phase = if archived { "archived".to_owned() } else { phase };
    if phase == "done" || phase == "archived" {
        return Err(CpdError::new(ErrorCode::PlanInactive, format!("Plan {plan} is {phase}: its Lead takes no message."))
            .with("plan", plan)
            .with("phase", phase));
    }
    Ok(())
}

/// The guards of an agent's send (hs-d507, hs-d514): the target's Plan is live, and the same trimmed text went neither to
/// another Session (`broadcast`) nor to this one (`duplicate`) in the last 60 minutes.
fn guard(tx: &WriteTxn<'_>, target: &Row, from: &SessionId, text: &str) -> Result<(), CpdError> {
    if target.role == Role::Lead
        && let Some(plan) = &target.plan {
            plan_inactive(tx, plan.as_str())?;
        }
    let earlier = |same: bool| -> Result<Option<(String, String)>, CpdError> {
        Ok(tx
            .query_row(
                "SELECT session, at FROM events
                 WHERE kind = 'session-message' AND json_extract(data, '$.from') = ?1 AND trim(json_extract(data, '$.text')) = ?2
                   AND (session = ?3) = ?4 AND at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-60 minutes')
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![from.as_str(), text.trim(), target.id.as_str(), same],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    };
    if let Some((session, at)) = earlier(false)? {
        return Err(CpdError::new(ErrorCode::Broadcast, format!("You sent this text to {session} within the hour: no broadcasts."))
            .with("session", session)
            .with("at", at));
    }
    if let Some((_, at)) = earlier(true)? {
        return Err(CpdError::new(ErrorCode::Duplicate, "You sent this text to this Session within the hour: no duplicates.").with("at", at));
    }
    Ok(())
}

/// The `session-message` Event for `target`, from `from` (the User when none), in the caller's transaction: the write of
/// `session send`, whatever sends.
fn write(tx: &WriteTxn<'_>, target: &Row, from: &Option<SessionId>, text: &str, unblocks: Option<&str>, wake: bool) -> Result<i64, CpdError> {
    let from_name: Option<String> = match from {
        Some(id) => tx.query_row("SELECT name FROM sessions WHERE id = ?1", [id.as_str()], |r| r.get(0)).optional()?,
        None => None,
    };
    let who = match (from, from_name) {
        (Some(id), Some(name)) => format!("{name} ({id})"),
        (Some(id), None) => id.to_string(),
        (None, _) => "the User".to_owned(),
    };
    let preview: String = text.chars().take(PREVIEW_CHARS).collect();
    let message = match unblocks {
        Some(reason) => {
            let summary = format!("Message for {} from {who} (unblocks: {reason}): {preview}", target.name);
            NewEvent::session_message_unblocking(summary, from, text, reason)
        }
        None => NewEvent::session_message(format!("Message for {} from {who}: {preview}", target.name), from, text, wake),
    };
    append(tx, scoped(message, target))
}

/// The engine's report of a stuck Job (blocked work, hs-d514): `text` as a waking message from the User's side (`from` null)
/// to one live Manager (role `manager`, not archived, the first created) of the Event's Context, or of any Context when the
/// Event has none. Returns how many it sent (0 or 1); none when there is no Manager.
pub fn tell_managers(tx: &WriteTxn<'_>, error: i64, text: &str) -> Result<usize, CpdError> {
    let context: Option<String> = tx.query_row("SELECT context FROM events WHERE id = ?1", [error], |r| r.get(0))?;
    tell_manager_of(tx, context.as_deref(), text)
}

/// [`tell_managers`] for a known Context (`None`: any Context): the message to one live Manager of it. Returns 0 or 1.
pub fn tell_manager_of(tx: &WriteTxn<'_>, context: Option<&str>, text: &str) -> Result<usize, CpdError> {
    let ids = {
        let mut stmt = tx.prepare(
            "SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL AND (?1 IS NULL OR context = ?1) ORDER BY created_at, id LIMIT 1",
        )?;
        stmt.query_map([context], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in &ids {
        write(tx, &row::load(tx, &SessionId::parse(id)?)?, &None, text, None, true)?;
    }
    Ok(ids.len())
}

/// A waking message from the User's side (`from` null) to the live Session `target`, in the caller's transaction: the write
/// of the stuck-Job message, for the engine's other reports to a Lead (E20).
pub fn tell(tx: &WriteTxn<'_>, target: &SessionId, text: &str) -> Result<i64, CpdError> {
    write(tx, &row::load(tx, target)?, &None, text, None, true)
}

/// Whether `target`, a stopped Lead or Manager with no live pane, is to be started after its message commits (hs-d725);
/// `no_cockpit` when it is and there is no cockpit to start it in.
fn needs_pane(env: &Env, target: &Row) -> Result<bool, CpdError> {
    // A durable Session has no pane by design: the message is only an Event.
    if target.runner == row::Runner::Durable {
        return Ok(false);
    }
    let tmux = env.tmux();
    let wake = target.state.as_str() == "stopped"
        && !tmux.list_panes().unwrap_or_default().iter().any(|p| !p.dead && p.cpd_session.as_ref() == Some(&target.id));
    if wake && !tmux.has_session(&env.config.tmux.cockpit_session)? {
        return Err(no_cockpit(&env.config.tmux.cockpit_session));
    }
    Ok(wake)
}

/// What `cpd session send <manager> <text> --unblocks <reason>` run with no sender Session does, for another command's
/// transaction: the same checks, the `session-message` Event written in the transaction of `inside` (whose value is
/// returned), and, after the commit, the start of a stopped Manager. Refuses before any write.
pub(crate) fn send_as_user<T>(
    env: &mut Env,
    id: &SessionId,
    command: &str,
    text: &str,
    unblocks: &str,
    inside: impl FnOnce(&WriteTxn<'_>) -> Result<T, CpdError>,
) -> Result<T, CpdError> {
    let needs_pane = {
        let target = env.store.read(|conn| row::load(conn, id))?;
        check(&target)?;
        needs_pane(env, &target)?
    };
    let (value, target) = env.store.write(command, |tx| {
        let target = row::load(tx, id)?;
        check(&target)?;
        let value = inside(tx)?;
        write(tx, &target, &None, text, Some(unblocks), true)?;
        Ok((value, target))
    })?;
    if needs_pane {
        start_parked(env, &target, command)?;
    }
    Ok(value)
}

pub(super) fn send(args: SendArgs) -> Result<Output, CpdError> {
    let text = match (args.text, &args.file) {
        (Some(text), None) => text,
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|e| CpdError::file_unreadable(path, e))?.parse::<Text>()?,
        _ => unreachable!("clap requires exactly one of <text> and --file"),
    };
    let from = sender()?;
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    // A stopped Lead or Manager with no live pane is started after the Event commits (hs-d725); no cockpit refuses first.
    let needs_pane = {
        let target = env.store.read(|conn| row::load(conn, &id))?;
        check(&target)?;
        needs_pane(&env, &target)?
    };
    let (event, target) = env.store.write("session send", |tx| {
        let target = row::load(tx, &id)?;
        check(&target)?;
        if let Some(from) = &from {
            guard(tx, &target, from, text.as_str())?;
        }
        let event = write(tx, &target, &from, text.as_str(), Some(args.unblocks.as_str()), true)?;
        Ok((event, target))
    })?;
    let pane = if needs_pane { start_parked(&mut env, &target, "session send")? } else { None };
    Ok(Output::success(json!({
        "event": event, "session": target.id, "from": from, "unblocks": args.unblocks.as_str(),
        "started": pane.is_some(), "pane": pane.as_ref().map(|p| p.as_str()),
    })))
}
