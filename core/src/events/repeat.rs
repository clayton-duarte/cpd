//! The one repeat lookup of `error` Events (contract §3.7): the engine, the driver and `session report`
//! append the same error at most once per window (`watch.error_repeat_seconds`).

use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

use super::{NewEvent, append};
use crate::error::CpdError;
use crate::ids::{PlanId, SessionId};
use crate::store::WriteTxn;

/// What an earlier Event's `Scope` is: the Event's `plan` or `session` column.
#[derive(Clone, Copy, Debug)]
pub enum Scope<'a> {
    Plan(&'a PlanId),
    Session(&'a SessionId),
}

/// When two `error` Events are the same. `source` and `code` are always compared.
#[derive(Clone, Copy, Debug)]
pub enum Same<'a> {
    /// The engine's: the text begins with `lead`.
    Lead { source: &'a str, code: &'a str, lead: &'a str },
    /// The driver's and the adapter's: `data.message` is equal, within the same `scope`.
    Message { source: &'a str, code: &'a str, message: &'a str, scope: Scope<'a> },
}

/// The earlier Event found: its id and how long ago it was appended.
#[derive(Clone, Copy, Debug)]
pub struct Recent {
    pub event: i64,
    pub age: Duration,
}

/// The newest identical `error` Event appended less than `window` ago. `at` is real UTC time
/// (crate::clock::now), so the cutoff is SQL's now. `None` when `window` is zero.
pub fn recent(conn: &Connection, window: Duration, same: &Same) -> Result<Option<Recent>, CpdError> {
    if window.is_zero() {
        return Ok(None);
    }
    let window = format!("-{} seconds", window.as_secs_f64());
    let (source, code) = match same {
        Same::Lead { source, code, .. } | Same::Message { source, code, .. } => (*source, *code),
    };
    let (rule, scope_column, value_a, value_b): (&str, &str, &str, String) = match same {
        Same::Lead { lead, .. } => ("substr(text, 1, length(?4)) = ?4", "?5 IS NULL", lead, String::new()),
        Same::Message { message, scope, .. } => match scope {
            Scope::Plan(plan) => ("json_extract(data, '$.message') = ?4", "plan = ?5", message, plan.as_str().to_owned()),
            Scope::Session(session) => ("json_extract(data, '$.message') = ?4", "session = ?5", message, session.to_string()),
        },
    };
    let sql = format!(
        "SELECT id, (julianday('now') - julianday(at)) * 86400.0 FROM events
          WHERE kind = 'error' AND json_extract(data, '$.source') = ?1
            AND json_extract(data, '$.code') = ?2 AND at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?3)
            AND {rule} AND {scope_column}
          ORDER BY id DESC LIMIT 1"
    );
    let scope_value = matches!(same, Same::Message { .. }).then_some(value_b);
    let row: Option<(i64, f64)> = conn
        .query_row(&sql, params![source, code, window, value_a, scope_value], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(row.map(|(event, secs)| Recent { event, age: Duration::from_secs_f64(secs.max(0.0)) }))
}

/// Appends `event` unless an identical one is within `window`; returns the id of the one appended, or of
/// the earlier one. The lookup and the append share `tx`.
pub fn append_once(tx: &WriteTxn<'_>, window: Duration, same: &Same, event: NewEvent) -> Result<i64, CpdError> {
    match recent(tx, window, same)? {
        Some(earlier) => Ok(earlier.event),
        None => append(tx, event),
    }
}
