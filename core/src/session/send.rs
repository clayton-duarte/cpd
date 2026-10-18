//! `session send` (commands.md §10.12, hs-d92, hs-d133): one message to a Lead or a Manager,
//! as a `session-message` Event its adapter delivers (§15.3).

use rusqlite::OptionalExtension;
use serde_json::json;

use super::moves::unreadable;
use super::row::{self, Role, Row};
use super::state::scoped;
use super::{Env, SendArgs, archive};
use crate::args::Text;
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;

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

pub(super) fn send(args: SendArgs) -> Result<Output, CpdError> {
    let text = match (args.text, &args.file) {
        (Some(text), None) => text,
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|e| unreadable(path, e))?.parse::<Text>()?,
        _ => unreachable!("clap requires exactly one of <text> and --file"),
    };
    let from = sender()?;
    let mut env = Env::open()?;
    let (event, target) = env.store.write("session send", |tx| {
        let target = row::load(tx, &args.id)?;
        check(&target)?;
        let from_name: Option<String> = match &from {
            Some(id) => tx.query_row("SELECT name FROM sessions WHERE id = ?1", [id.as_str()], |r| r.get(0)).optional()?,
            None => None,
        };
        let who = match (&from, from_name) {
            (Some(id), Some(name)) => format!("{name} ({id})"),
            (Some(id), None) => id.to_string(),
            (None, _) => "the User".to_owned(),
        };
        let preview: String = text.as_str().chars().take(PREVIEW_CHARS).collect();
        let summary = format!("Message for {} from {who}: {preview}", target.name);
        let data = json!({"from": from, "text": text.as_str()});
        let event = append(tx, scoped(NewEvent::new(Kind::SessionMessage, summary, data), &target))?;
        Ok((event, target))
    })?;
    Ok(Output::success(json!({"event": event, "session": target.id, "from": from})))
}
