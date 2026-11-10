//! E25, the refusal threshold (contract §3.7 E25, §2.2 `refusal-alert`; commands.md §14.7; Plan
//! `agent-refusals`): every tick, for each Session that is not archived, count the `guard` Events whose `session` is
//! that Session and whose `at` is inside the sliding window of `watch.refusal_alert_minutes`; at or above
//! `watch.refusal_alert_threshold` append one `error` Event (code `refusal_threshold`) and wake the Context's
//! Manager with a `session-message` (the air-d64 pattern). Below the threshold, and once per window and Session,
//! nothing is written.
//!
//! `refusal_threshold` is not a step failure: the step succeeds, reports nothing to the step-failure machinery and
//! opens no `engine-error` item (contract §3.7 E25).

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::SOURCE;
use super::engine::Engine;
use cpd_core::clock::timestamp;
use cpd_core::error::CpdError;
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::SessionId;
use cpd_core::inbox::refusals::{self, Refusal};

/// E25 (§3.7): the refusal crossings of this tick, each reported and delivered.
pub(super) fn run(engine: &mut Engine) {
    let minutes = engine.config.watch.refusal_alert_minutes.max(1);
    let threshold = u64::from(engine.config.watch.refusal_alert_threshold);
    let now = engine.ports.clock.utc();
    let since = now - time::Duration::minutes(i64::from(minutes));
    let since_text = timestamp(since);

    let found = engine.store.read(|conn| crossings(conn, since, threshold));
    let found = match found {
        Ok(found) => found,
        Err(e) => {
            engine.log.line(&format!(
                "refusal-threshold: cannot count the guard Events: {} ({})",
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
                let message =
                    NewEvent::session_message(text.clone(), None::<String>, &text, true, None)?;
                events::append(tx, message.session(SessionId::parse(&manager)?))?;
            }
            Ok(true)
        });
        match written {
            Ok(true) => engine.log.line(&format!(
                "refusal-threshold: Session {} was refused {} times in the last {} min",
                crossing.session, crossing.count, minutes
            )),
            Ok(false) => {}
            Err(e) => engine.log.line(&format!(
                "refusal-threshold: cannot append the refusal alert for {}: {} ({})",
                crossing.session,
                e.message(),
                e.code().as_str()
            )),
        }
    }
}

/// Every Session not archived whose counted `guard` Events reach `threshold`, oldest first. The count itself is the
/// one query E8's `refusal-alert` item also reads (`cpd_core::inbox::refusals::over_window`, contract §3.7 E25).
fn crossings(
    conn: &Connection,
    since: OffsetDateTime,
    threshold: u64,
) -> Result<Vec<Refusal>, CpdError> {
    Ok(refusals::over_window(conn, since)?
        .into_iter()
        .filter(|crossing| crossing.count >= threshold)
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
fn alert_details(crossing: &Refusal, minutes: u32, threshold: u64) -> Value {
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
fn alert_text(crossing: &Refusal, minutes: u32) -> String {
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
