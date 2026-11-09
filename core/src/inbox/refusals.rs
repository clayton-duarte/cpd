//! The `guard`-Event refusal count (contract §3.7 E25, §2.2 `refusal-alert`; Plan `agent-refusals`): the one
//! query over the sliding window of a Session's `guard` Events, and the per-Session count both E25's alert and
//! E8's `refusal-alert` item read. One query, one place (§3.7 E25: "the count is one query, the same one E8's
//! `refusal-alert` item uses").

use std::collections::BTreeMap;

use rusqlite::Connection;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::CpdError;

/// One Session's counted refusals over the window: the count and what a report and an item link.
pub struct Refusal {
    pub session: String,
    pub context: String,
    pub plan: Option<String>,
    /// Whether this Session is its Plan's `lead_session` (the tree labels a Plan Lead by the Plan id).
    pub plan_lead: bool,
    pub role: String,
    pub tier: String,
    /// The `data.model` of the newest counted `guard` Event, `unknown` when it stores none.
    pub model: String,
    pub count: u64,
    /// `data.code` → count over the window.
    pub per_guard: BTreeMap<String, u64>,
    /// The `at` of the window's oldest and newest counted `guard` Events.
    pub first: String,
    pub last: String,
    /// The newest counted `guard` Event's `data.excerpt`.
    pub excerpt: String,
}

/// Every Session that is not archived whose `guard` Events' `at` is at or after `since`, oldest Session first.
/// A Session with no counted Event is left out (E25 and E8 both need only the counted ones). One query, the one
/// E25's alert and E8's item share.
pub fn over_window(conn: &Connection, since: OffsetDateTime) -> Result<Vec<Refusal>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT e.session, s.context, s.plan,
                EXISTS (SELECT 1 FROM plans q WHERE q.lead_session = s.id),
                s.role, s.tier, json_extract(e.data, '$.code'),
                json_extract(e.data, '$.model'), json_extract(e.data, '$.excerpt'), e.at
         FROM events e JOIN sessions s ON s.id = e.session
         WHERE e.kind = 'guard' AND e.session IS NOT NULL AND s.archived_at IS NULL
         ORDER BY e.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, i64>(3)? == 1,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;

    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeMap<String, Refusal> = BTreeMap::new();
    for row in rows {
        let (session, context, plan, plan_lead, role, tier, code, model, excerpt, at) = row?;
        let Some(at_dt) = OffsetDateTime::parse(&at, &Rfc3339).ok() else {
            continue;
        };
        if at_dt < since {
            continue;
        }
        let entry = seen.entry(session.clone()).or_insert_with(|| {
            order.push(session.clone());
            Refusal {
                session: session.clone(),
                context: context.clone(),
                plan: plan.clone(),
                plan_lead,
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

    Ok(order.into_iter().filter_map(|s| seen.remove(&s)).collect())
}
