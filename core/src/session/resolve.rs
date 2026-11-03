//! Short Session ids (commands.md §1.3, hs-a3): the one place a [`SessionRef`]
//! becomes a [`SessionId`]. Every command that takes a Session calls it at the
//! boundary, so downstream code only holds a `SessionId`.

use rusqlite::{Connection, OptionalExtension};

use crate::args::SessionRef;
use crate::error::{CpdError, ErrorCode};
use crate::ids::SessionId;

/// A bare prefix shorter than this is no form (4).
const MIN_BARE_PREFIX: usize = 4;

/// The Session `reference` names for the caller in `CPD_SESSION` (see [`resolve_as`]).
pub(super) fn resolve(conn: &Connection, reference: &SessionRef) -> Result<SessionId, CpdError> {
    let caller = super::sender()?;
    resolve_as(conn, reference, caller.as_ref())
}

/// The Session `reference` names for `caller`, in order: exact id; exact name (live, non-fork, latest Attempt first);
/// the `<context>-manager` alias; a relative Plan; a relative Job; the legacy forms. Archived Sessions count.
/// None: `session_not_found`; several for a prefix: `session_ambiguous`.
pub(super) fn resolve_as(
    conn: &Connection,
    reference: &SessionRef,
    caller: Option<&SessionId>,
) -> Result<SessionId, CpdError> {
    let raw = match reference {
        SessionRef::Scoped { plan, prefix } => {
            return one(
                starting_with(conn, Some(plan.as_str()), prefix.as_str())?,
                reference,
            );
        }
        SessionRef::Plain(value) => value.as_str(),
        SessionRef::Key(key) => key.as_str(),
    };
    if let Some(id) = scalar(conn, "SELECT id FROM sessions WHERE id = ?1", &[raw])? {
        return parse(&id);
    }
    if let Some(id) = by_name(conn, raw)? {
        return parse(&id);
    }
    if let Some(context) = raw.strip_suffix("-manager")
        && let Some(id) = scalar(
            conn,
            "SELECT s.id FROM sessions s JOIN contexts c ON c.id = s.context
             WHERE c.id = ?1 AND s.role = 'manager'
             ORDER BY s.archived_at IS NOT NULL, s.created_at DESC, s.id LIMIT 1",
            &[context],
        )?
    {
        return parse(&id);
    }
    if !raw.contains(':')
        && let Some(caller) = caller
        && let Some(id) = relative(conn, raw, caller)?
    {
        return parse(&id);
    }
    legacy(conn, raw, reference)
}

/// Steps 4 and 5: the bare `raw` as a Plan id of the caller's Context, then as a Job key of the caller's Plan.
fn relative(conn: &Connection, raw: &str, caller: &SessionId) -> Result<Option<String>, CpdError> {
    let (context, plan): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT context, plan FROM sessions WHERE id = ?1",
            [caller.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((None, None));
    let Some(context) = context else {
        return Ok(None);
    };
    if scalar(
        conn,
        "SELECT id FROM plans WHERE id = ?1 AND context = ?2",
        &[raw, &context],
    )?
    .is_some()
        && let Some(id) = by_name(conn, &format!("{context}:{raw}"))?
    {
        return Ok(Some(id));
    }
    if let Some(plan) = plan
        && scalar(
            conn,
            "SELECT key FROM jobs WHERE plan = ?1 AND key = ?2",
            &[&plan, raw],
        )?
        .is_some()
    {
        return by_name(conn, &format!("{context}:{plan}:{raw}"));
    }
    Ok(None)
}

/// Step 6: a Plan id (its Lead) or a bare id prefix of at least [`MIN_BARE_PREFIX`] characters.
fn legacy(conn: &Connection, raw: &str, reference: &SessionRef) -> Result<SessionId, CpdError> {
    if let Some(lead) = scalar(conn, "SELECT lead_session FROM plans WHERE id = ?1", &[raw])? {
        return parse(&lead);
    }
    if raw.len() >= MIN_BARE_PREFIX {
        return one(starting_with(conn, None, raw)?, reference);
    }
    Err(not_found(reference))
}

/// The best Session named `name`: live, then non-fork, then the highest Attempt, then newest.
fn by_name(conn: &Connection, name: &str) -> Result<Option<String>, CpdError> {
    scalar(
        conn,
        "SELECT s.id FROM sessions s
         LEFT JOIN attempts a ON a.plan = s.plan AND a.id = s.attempt
         WHERE s.name = ?1
         ORDER BY s.archived_at IS NOT NULL, s.forked_from IS NOT NULL, a.number DESC, s.created_at DESC, s.id
         LIMIT 1",
        &[name],
    )
}

fn scalar(conn: &Connection, sql: &str, params: &[&str]) -> Result<Option<String>, CpdError> {
    Ok(conn
        .query_row(sql, rusqlite::params_from_iter(params), |r| r.get(0))
        .optional()?)
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
