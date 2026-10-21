//! `session send` (commands.md §10.13, hs-d92, hs-d133): one message to a Lead or a Manager,
//! as a `session-message` Event its adapter delivers (§15.3).

use rusqlite::OptionalExtension;
use serde_json::json;

use super::row::{self, Role, Row};
use super::state::scoped;
use super::{Env, SendArgs, archive};
use crate::args::Text;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::WriteTxn;

/// How much of the message the Event's `text` quotes.
const PREVIEW_CHARS: usize = 60;

/// `CPD_SESSION`, else `PI_SESSION_ID`, else none: the User.
fn sender() -> Result<Option<SessionId>, CpdError> {
    ["CPD_SESSION", "PI_SESSION_ID"]
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
        .map(|v| SessionId::parse(&v.to_string_lossy()))
        .transpose()
}

/// The target is a Lead or a Manager and is not archived (a `stopped` one is fine).
fn check(target: &Row) -> Result<(), CpdError> {
    match target.role {
        Role::Lead | Role::Manager if target.archived_at.is_none() => Ok(()),
        Role::Lead | Role::Manager => Err(archive::archived(target)),
        role => Err(CpdError::new(ErrorCode::NotALead, format!("Session {} is a {}, not a Lead or a Manager.", target.id, role.as_str()))
            .with("session", target.id.as_str())
            .with("role", role.as_str())),
    }
}

/// The `session-message` Event for `target`, from `from` (the User when none), in the caller's transaction: the write of
/// `session send`, whatever sends.
fn write(tx: &WriteTxn<'_>, target: &Row, from: &Option<SessionId>, text: &str, wake: bool) -> Result<i64, CpdError> {
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
    let summary = format!("Message for {} from {who}: {preview}", target.name);
    let message = NewEvent::session_message(summary, from, text, wake);
    append(tx, scoped(message, target))
}

/// The engine's report of an `error` Event (contract §3.7, hs-d314): `text` as a message from the User's side (`from` null,
/// no wake: `session send` with no sender and no `--wake`) to the live Managers (role `manager`, not archived) of the
/// Event's Context, or of every Context when the Event has none. Returns how many it sent; none when there is no Manager.
pub fn tell_managers(tx: &WriteTxn<'_>, error: i64, text: &str) -> Result<usize, CpdError> {
    let context: Option<String> = tx.query_row("SELECT context FROM events WHERE id = ?1", [error], |r| r.get(0))?;
    let ids = {
        let mut stmt = tx.prepare(
            "SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL AND (?1 IS NULL OR context = ?1) ORDER BY created_at, id",
        )?;
        stmt.query_map([context], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in &ids {
        write(tx, &row::load(tx, &SessionId::parse(id)?)?, &None, text, false)?;
    }
    Ok(ids.len())
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
    let (event, target) = env.store.write("session send", |tx| {
        let target = row::load(tx, &id)?;
        check(&target)?;
        let event = write(tx, &target, &from, text.as_str(), args.wake)?;
        Ok((event, target))
    })?;
    Ok(Output::success(json!({"event": event, "session": target.id, "from": from, "wake": args.wake})))
}
