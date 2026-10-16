//! `plan list` and `plan show` (commands.md §4.2, §4.3): read-only snapshots,
//! each in one read transaction with its `events_cursor` (§17).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::fingerprint::plan_fingerprint;
use super::jobs::{self, Job};
use super::row::{self, PlanRow};
use super::{Env, ListArgs, ShowArgs};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::ids::PlanId;
use crate::output::Output;

/// `plan list` (§4.2).
pub(super) fn list(args: ListArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let (plans, cursor) = env.store.read(|conn| {
        let context = args.context.as_ref().map(|c| c.as_str().to_owned());
        let phase = args.phase.map(|p| p.as_str());
        let plans = row::query(conn, "(?1 IS NULL OR p.context = ?1) AND (?2 IS NULL OR p.phase = ?2)", &[&context, &phase])?;
        let mut out = Vec::new();
        for plan in plans {
            let jobs = jobs::load(conn, &plan.id)?;
            out.push(json!({
                "id": plan.id, "context": plan.context, "title": plan.title, "phase": plan.phase.as_str(),
                "repo": plan.repo, "lead_session": plan.lead_session, "counts": jobs::counts(&jobs), "pr": plan.pr,
            }));
        }
        Ok((out, highest_id(conn)?))
    })?;
    Ok(Output::success(json!({"plans": plans, "events_cursor": cursor})))
}

/// `plan show` (§4.3).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let snapshot = env.store.read(|conn| {
        let plan = row::load(conn, &id)?;
        let jobs = jobs::load(conn, &id)?;
        let attempts = attempts(conn, &id)?;
        let escalations = open_escalations(conn, &id)?;
        let review = latest_review(conn, &id)?;
        Ok((plan, jobs, attempts, escalations, review, highest_id(conn)?))
    })?;
    let (plan, jobs, mut attempts, escalations, review, cursor): (PlanRow, Vec<Job>, _, _, _, _) = snapshot;
    let mut tamper = env.store.tamper()?;
    tamper.extend(jobs::tampered(&id, &jobs));
    let job_values: Vec<Value> = jobs
        .iter()
        .map(|j| {
            let mut v = j.to_json();
            v["attempts"] = json!(attempts.remove(&j.key).unwrap_or_default());
            v
        })
        .collect();
    Ok(Output::success(json!({
        "plan": plan.to_json(),
        "jobs": job_values,
        "escalations": escalations,
        "review": review,
        "counts": jobs::counts(&jobs),
        "tamper": tamper,
        "events_cursor": cursor,
    })))
}

/// Every Attempt of the Plan by Job, oldest first.
fn attempts(conn: &Connection, plan: &PlanId) -> Result<BTreeMap<String, Vec<Value>>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT job, id, number, tier, origin, status, outcome, cause, worker_session, started_at, ended_at
           FROM attempts WHERE plan = ?1 ORDER BY job, number",
    )?;
    let mut by_job: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let rows = stmt.query_map([plan.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            json!({
                "id": r.get::<_, String>(1)?, "number": r.get::<_, i64>(2)?, "tier": r.get::<_, String>(3)?,
                "origin": r.get::<_, String>(4)?, "status": r.get::<_, String>(5)?,
                "outcome": r.get::<_, Option<String>>(6)?, "cause": r.get::<_, Option<String>>(7)?,
                "worker_session": r.get::<_, Option<String>>(8)?,
                "started_at": r.get::<_, String>(9)?, "ended_at": r.get::<_, Option<String>>(10)?,
            }),
        ))
    })?;
    for row in rows {
        let (job, attempt) = row?;
        by_job.entry(job).or_default().push(attempt);
    }
    Ok(by_job)
}

/// The Plan's `open` Escalations, oldest first.
fn open_escalations(conn: &Connection, plan: &PlanId) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, attempt, question, asked_at FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "attempt": r.get::<_, String>(1)?,
                      "question": r.get::<_, String>(2)?, "asked_at": r.get::<_, String>(3)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The latest Plan review, with `current`: whether its fingerprint is still
/// the plan fingerprint. Null when the Plan has none.
fn latest_review(conn: &Connection, plan: &PlanId) -> Result<Value, CpdError> {
    let review = conn
        .query_row(
            "SELECT id, status, fingerprint, feedback, requested_at, decided_at FROM plan_reviews
              WHERE plan = ?1 ORDER BY number DESC LIMIT 1",
            [plan.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;
    Ok(match review {
        None => Value::Null,
        Some((id, status, fingerprint, feedback, requested_at, decided_at)) => {
            let current = fingerprint == plan_fingerprint(conn, plan)?;
            json!({"id": id, "status": status, "fingerprint": fingerprint, "current": current,
                   "feedback": feedback, "requested_at": requested_at, "decided_at": decided_at})
        }
    })
}
