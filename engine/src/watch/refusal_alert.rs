//! E25, the refusal threshold (contract §3.7 E25, §2.2 `refusal-alert`; commands.md §14.7; Plan
//! `agent-refusals`): every tick, for each Session that is not archived, count the `guard` Events whose `session` is
//! that Session and whose `at` is inside the sliding window of `watch.refusal_alert_minutes`; at or above
//! `watch.refusal_alert_threshold` append one `error` Event (code `refusal_threshold`) and wake the Context's
//! Manager with a `session-message` (the air-d64 pattern). Below the threshold, and once per window and Session,
//! nothing is written.
//!
//! `refusal_threshold` is not a step failure: the step succeeds, reports nothing to the step-failure machinery and
//! opens no `engine-error` item (contract §3.7 E25).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::SOURCE;
use super::engine::Engine;
use cpd_core::clock::timestamp;
use cpd_core::error::CpdError;
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::SessionId;

/// One Session's counted refusals over the window, at or above the threshold.
struct Crossing {
    session: String,
    context: String,
    role: String,
    tier: String,
    model: String,
    count: u64,
    per_guard: BTreeMap<String, u64>,
    /// The `at` of the window's oldest and newest counted `guard` Events.
    first: String,
    last: String,
    /// The newest counted `guard` Event's `data.excerpt`.
    excerpt: String,
}

/// E25 (§3.7): the refusal crossings of this tick, each reported and delivered.
pub(super) fn run(engine: &mut Engine) {
    let minutes = engine.config.watch.refusal_alert_minutes.max(1);
    let threshold = u64::from(engine.config.watch.refusal_alert_threshold);
    let now = engine.ports.clock.utc();
    let since = now - time::Duration::minutes(i64::from(minutes));
    let since_text = timestamp(since);

    let found = engine
        .store
        .read(|conn| crossings(conn, since, threshold));
    let found = match found {
        Ok(found) => found,
        Err(e) => {
            engine.log.line(&format!(
                "E25: cannot count the guard Events: {} ({})",
                e.message(),
                e.code().as_str()
            ));
            return;
        }
    };

    for crossing in found {
        let text = alert_text(&crossing, minutes);
        let details = alert_details(&crossing, minutes, threshold);
        let written = engine.store.write(SOURCE, |tx| {
            if already_alerted(tx, &since_text, &crossing.session)? {
                return Ok(false);
            }
            events::append(
                tx,
                NewEvent::error(text.clone(), SOURCE, "refusal_threshold", &text, &details),
            )?;
            if let Some(manager) = live_manager(tx, &crossing.context)?
                && manager != crossing.session
            {
                // The air-d64 wake: a message from the User's side (`from` null) that wakes the Manager at once.
                let message = NewEvent::session_message(text.clone(), None::<String>, &text, true, None)?;
                events::append(tx, message.session(SessionId::parse(&manager)?))?;
            }
            Ok(true)
        });
        match written {
            Ok(true) => engine.log.line(&format!(
                "E25: Session {} was refused {} times in the last {} min",
                crossing.session, crossing.count, minutes
            )),
            Ok(false) => {}
            Err(e) => engine.log.line(&format!(
                "E25: cannot append the refusal alert for {}: {} ({})",
                crossing.session,
                e.message(),
                e.code().as_str()
            )),
        }
    }
}

/// Every Session not archived whose counted `guard` Events reach `threshold`, oldest first.
fn crossings(
    conn: &Connection,
    since: OffsetDateTime,
    threshold: u64,
) -> Result<Vec<Crossing>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT e.session, s.context, s.role, s.tier, json_extract(e.data, '$.code'),
                json_extract(e.data, '$.model'), json_extract(e.data, '$.excerpt'), e.at
         FROM events e JOIN sessions s ON s.id = e.session
         WHERE e.kind = 'guard' AND e.session IS NOT NULL AND s.archived_at IS NULL
         ORDER BY e.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;

    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeMap<String, Crossing> = BTreeMap::new();
    for row in rows {
        let (session, context, role, tier, code, model, excerpt, at) = row?;
        let Some(at_dt) = OffsetDateTime::parse(&at, &Rfc3339).ok() else {
            continue;
        };
        if at_dt < since {
            continue;
        }
        let entry = seen.entry(session.clone()).or_insert_with(|| {
            order.push(session.clone());
            Crossing {
                session: session.clone(),
                context: context.clone(),
                role: role.clone(),
                tier: tier.clone(),
                model: "unknown".to_owned(),
                count: 0,
                per_guard: BTreeMap::new(),
                first: at.clone(),
                last: String::new(),
                excerpt: String::new(),
            }
        });
        entry.count += 1;
        *entry
            .per_guard
            .entry(code.unwrap_or_else(|| "unknown".to_owned()))
            .or_insert(0) += 1;
        if entry.count == 1 || at < entry.first {
            entry.first = at.clone();
        }
        if entry.count == 1 || at >= entry.last {
            entry.last = at.clone();
            entry.excerpt = excerpt.unwrap_or_default();
            entry.model = model.unwrap_or_else(|| "unknown".to_owned());
        }
    }

    Ok(order
        .into_iter()
        .filter_map(|session| {
            seen.remove(&session)
                .filter(|crossing| crossing.count >= threshold)
        })
        .collect())
}

/// Whether an `error` Event of code `refusal_threshold` newer than the window already names `session` (the Event sets
/// no scope column, so the Session is matched by the text prefix; contract §3.7 E25).
fn already_alerted(tx: &Connection, since: &str, session: &str) -> Result<bool, CpdError> {
    let prefix = format!("Session {session} was refused ");
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM events WHERE kind = 'error' AND at >= ?1
                         AND json_extract(data, '$.code') = 'refusal_threshold'
                         AND instr(text, ?2) = 1)",
        rusqlite::params![since, prefix],
        |r| r.get(0),
    )?)
}

/// The id of `context`'s live Manager (role `manager`, not archived), the first created.
fn live_manager(tx: &Connection, context: &str) -> Result<Option<String>, CpdError> {
    Ok(tx
        .query_row(
            "SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL AND context = ?1
             ORDER BY created_at, id LIMIT 1",
            [context],
            |r| r.get(0),
        )
        .optional()?)
}

/// The Event's `details` (contract §3.7 E25).
fn alert_details(crossing: &Crossing, minutes: u32, threshold: u64) -> Value {
    json!({
        "session": crossing.session,
        "role": crossing.role,
        "tier": crossing.tier,
        "model": crossing.model,
        "window_minutes": minutes,
        "threshold": threshold,
        "count": crossing.count,
        "per_guard": crossing.per_guard,
        "first": crossing.first,
        "last": crossing.last,
    })
}

/// The alert's text (contract §3.7 E25): the count, the window, the three guards with the most refusals, and the
/// newest excerpt.
fn alert_text(crossing: &Crossing, minutes: u32) -> String {
    let mut guards: Vec<(&String, &u64)> = crossing.per_guard.iter().collect();
    guards.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let shown = guards
        .iter()
        .take(3)
        .map(|(guard, count)| format!("{guard} ×{count}"))
        .collect::<Vec<_>>()
        .join(", ");
    let more = if guards.len() > 3 { "…" } else { "" };
    let excerpt: String = crossing.excerpt.chars().take(200).collect();
    format!(
        "Session {} was refused {} times in the last {} min: {shown}{more}. Last: \"{excerpt}\". \
         Nudge its Lead (cpd session send {} \"<text>\" --unblocks \"<reason>\"), or ask the User.",
        crossing.session, crossing.count, minutes, crossing.session
    )
}
