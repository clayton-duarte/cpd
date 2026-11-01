//! Reading Events: the selection of §5.4 and §11, and the Event as printed.

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use super::kind::Kind;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, PlanId, SessionId};

/// An Event as stored and printed (§2.2 Event).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Event {
    pub id: i64,
    pub at: String,
    pub kind: String,
    pub context: Option<String>,
    pub plan: Option<String>,
    pub job: Option<String>,
    pub attempt: Option<String>,
    pub session: Option<String>,
    pub text: String,
    pub data: Value,
}

/// Which Events: `--plan`, `--context`, `--session` or none, and `--kind`.
/// The flags are filters: they never fall back to `CPD_PLAN` or `CPD_SESSION`.
#[derive(Clone, Debug)]
pub struct Selection {
    filter: Filter,
    kinds: Vec<Kind>,
    /// A follower of a Lead or another Session never gets an adapter `error` (it names its Session: it would get its own error back); `events list` does.
    follower: bool,
}

#[derive(Clone, Debug)]
enum Filter {
    All,
    Plan(PlanId),
    Context(ContextId),
    /// A Session that is its Plan's `lead_session` (amendment 7): its Plan, its
    /// Context's Events about no Plan, and itself; an adapter `error` of another Session is not among them (it is the Manager's), nor, for its follower, its own.
    Lead {
        session: SessionId,
        plan: PlanId,
        context: ContextId,
    },
    /// Any other Session (a Worker, a side fork, a Manager): Events addressed to itself; its follower never gets an adapter `error`.
    Session(SessionId),
}

fn not_found(code: ErrorCode, what: &str, id: &str) -> CpdError {
    CpdError::new(code, format!("No {what} {id}.")).with(what.to_lowercase().as_str(), id)
}

impl Selection {
    /// At most one of `plan`, `context`, `session` is given (the command line
    /// enforces it); the selected object must exist.
    pub fn resolve(
        conn: &Connection,
        plan: Option<PlanId>,
        context: Option<ContextId>,
        session: Option<SessionId>,
        kinds: Vec<Kind>,
    ) -> Result<Selection, CpdError> {
        let filter = match (plan, context, session) {
            (None, None, None) => Filter::All,
            (Some(plan), None, None) => {
                exists(
                    conn,
                    "plans",
                    plan.as_str(),
                    ErrorCode::PlanNotFound,
                    "Plan",
                )?;
                Filter::Plan(plan)
            }
            (None, Some(context), None) => {
                exists(
                    conn,
                    "contexts",
                    context.as_str(),
                    ErrorCode::ContextNotFound,
                    "Context",
                )?;
                Filter::Context(context)
            }
            (None, None, Some(session)) => {
                exists(
                    conn,
                    "sessions",
                    session.as_str(),
                    ErrorCode::SessionNotFound,
                    "Session",
                )?;
                let lead: Option<(String, String)> = conn
                    .query_row(
                        "SELECT id, context FROM plans WHERE lead_session = ?1",
                        [session.as_str()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                match lead {
                    Some((plan, context)) => Filter::Lead {
                        session,
                        plan: PlanId::parse(&plan)?,
                        context: ContextId::parse(&context)?,
                    },
                    None => Filter::Session(session),
                }
            }
            _ => unreachable!("--plan, --context and --session are exclusive on the command line"),
        };
        Ok(Selection {
            filter,
            kinds,
            follower: false,
        })
    }

    /// The selection as `events follow` reads it.
    pub fn following(mut self) -> Selection {
        self.follower = true;
        self
    }

    /// Whether a line about to be printed is checked for staleness (§5.4): only a follower of a Session.
    pub fn drops_stale(&self) -> bool {
        self.follower && matches!(self.filter, Filter::Lead { .. } | Filter::Session(_))
    }

    /// Up to `limit` matching Events with `after < id` (and `id <= up_to` when
    /// given), in increasing `id`.
    pub fn fetch(
        &self,
        conn: &Connection,
        after: i64,
        up_to: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Event>, CpdError> {
        let text = |s: &str| Sql::Text(s.to_owned());
        let mut sql = String::from(
            "SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > ?",
        );
        let mut params = vec![Sql::Integer(after)];
        if let Some(up_to) = up_to {
            sql.push_str(" AND id <= ?");
            params.push(Sql::Integer(up_to));
        }
        match &self.filter {
            Filter::All => {}
            Filter::Plan(plan) => {
                sql.push_str(" AND plan = ?");
                params.push(text(plan.as_str()));
            }
            Filter::Context(context) => {
                sql.push_str(" AND context = ?");
                params.push(text(context.as_str()));
            }
            Filter::Lead {
                session,
                plan,
                context,
            } => {
                sql.push_str(" AND (plan = ? OR (plan IS NULL AND context = ? AND NOT (kind = 'error' AND json_extract(data, '$.source') = 'adapter')) OR session = ?)");
                params.extend([
                    text(plan.as_str()),
                    text(context.as_str()),
                    text(session.as_str()),
                ]);
            }
            Filter::Session(session) => {
                sql.push_str(" AND session = ?");
                params.push(text(session.as_str()));
            }
        }
        if self.drops_stale() {
            sql.push_str(
                " AND NOT (kind = 'error' AND json_extract(data, '$.source') = 'adapter')",
            );
        }
        if !self.kinds.is_empty() {
            sql.push_str(&format!(
                " AND kind IN ({})",
                vec!["?"; self.kinds.len()].join(", ")
            ));
            params.extend(self.kinds.iter().map(|k| text(k.as_str())));
        }
        sql.push_str(" ORDER BY id LIMIT ?");
        params.push(Sql::Integer(limit));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
            Ok((
                Event {
                    id: r.get(0)?,
                    at: r.get(1)?,
                    kind: r.get(2)?,
                    context: r.get(3)?,
                    plan: r.get(4)?,
                    job: r.get(5)?,
                    attempt: r.get(6)?,
                    session: r.get(7)?,
                    text: r.get(8)?,
                    data: Value::Null,
                },
                r.get::<_, String>(9)?,
            ))
        })?;
        rows.map(|row| {
            let (mut event, data) = row?;
            event.data = serde_json::from_str(&data).map_err(|e| {
                CpdError::new(
                    ErrorCode::StoreFailed,
                    format!("The data of Event {} is not JSON: {e}.", event.id),
                )
                .with("event", event.id)
            })?;
            Ok(event)
        })
        .collect()
    }
}

fn exists(
    conn: &Connection,
    table: &str,
    id: &str,
    code: ErrorCode,
    what: &str,
) -> Result<(), CpdError> {
    let found: Option<i64> = conn
        .query_row(&format!("SELECT 1 FROM {table} WHERE id = ?1"), [id], |r| {
            r.get(0)
        })
        .optional()?;
    found.map(|_| ()).ok_or_else(|| not_found(code, what, id))
}

/// The highest Event id, 0 when there is none.
pub fn highest_id(conn: &Connection) -> Result<i64, CpdError> {
    Ok(conn.query_row("SELECT coalesce(max(id), 0) FROM events", [], |r| r.get(0))?)
}
