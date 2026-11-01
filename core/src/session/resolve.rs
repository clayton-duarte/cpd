//! Short Session ids (commands.md §1.3, hs-a3): the one place a [`SessionRef`]
//! becomes a [`SessionId`]. Every command that takes a Session calls it at the
//! boundary, so downstream code only holds a `SessionId`.

use rusqlite::{Connection, OptionalExtension};

use crate::args::SessionRef;
use crate::error::{CpdError, ErrorCode};
use crate::ids::SessionId;

/// A bare prefix shorter than this is no form (4).
const MIN_BARE_PREFIX: usize = 4;

/// The Session `reference` names, in the order of §1.3: the full id; a Plan
/// id (its Lead Session); `<plan>·<prefix>`; a bare prefix of at least
/// [`MIN_BARE_PREFIX`] characters. Archived Sessions count. A form that
/// matches nothing falls through; none: `session_not_found`; several for
/// a prefix form: `session_ambiguous`.
pub(super) fn resolve(conn: &Connection, reference: &SessionRef) -> Result<SessionId, CpdError> {
    match reference {
        SessionRef::Plain(value) => {
            let raw = value.as_str();
            if let Some(id) = conn
                .query_row("SELECT id FROM sessions WHERE id = ?1", [raw], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
            {
                return parse(&id);
            }
            if let Some(lead) = conn
                .query_row("SELECT lead_session FROM plans WHERE id = ?1", [raw], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
            {
                return parse(&lead);
            }
            if raw.len() >= MIN_BARE_PREFIX {
                return one(starting_with(conn, None, raw)?, reference);
            }
            Err(not_found(reference))
        }
        SessionRef::Scoped { plan, prefix } => one(
            starting_with(conn, Some(plan.as_str()), prefix.as_str())?,
            reference,
        ),
    }
}

/// The ids of the Sessions (of `plan`, if given) whose id starts with `prefix` (exact, case-sensitive).
fn starting_with(
    conn: &Connection,
    plan: Option<&str>,
    prefix: &str,
) -> Result<Vec<String>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM sessions WHERE (?1 IS NULL OR plan = ?1) AND substr(id, 1, length(?2)) = ?2 ORDER BY created_at, id",
    )?;
    let ids = stmt
        .query_map((plan, prefix), |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

fn one(mut ids: Vec<String>, reference: &SessionRef) -> Result<SessionId, CpdError> {
    match ids.len() {
        0 => Err(not_found(reference)),
        1 => parse(&ids.remove(0)),
        n => Err(CpdError::new(
            ErrorCode::SessionAmbiguous,
            format!(
                "{reference} matches {n} Sessions ({}): use more characters or the full id.",
                ids.join(", ")
            ),
        )
        .with("session", reference.to_string())
        .with("candidates", ids)),
    }
}

fn not_found(reference: &SessionRef) -> CpdError {
    CpdError::new(
        ErrorCode::SessionNotFound,
        format!("No Session {reference}."),
    )
    .with("session", reference.to_string())
}

fn parse(id: &str) -> Result<SessionId, CpdError> {
    SessionId::parse(id).map_err(|_| CpdError::bad_row(format!("id {id:?}")))
}
