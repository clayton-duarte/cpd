//! Stale Events (contract §5.4): which Events `events follow --session` skips because a later Event, or the
//! PR's row, proves the condition they report is over. A missing object or field fails open (not stale).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::kind::Kind;
use super::select::Event;
use crate::error::CpdError;

/// Whether `event` is stale: a later Event (an id above its own, anywhere in the store) or the PR's row proves it.
pub fn is_stale(conn: &Connection, event: &Event) -> Result<bool, CpdError> {
    let Some(kind) = Kind::ALL.iter().copied().find(|k| k.as_str() == event.kind) else {
        return Ok(false);
    };
    let plan = event.plan.as_deref();
    let job = event.job.as_deref();
    let id = event.id;
    // §5.4 "Stale Events": the table of kinds and what drops each.
    match kind {
        Kind::PlanDone => {
            let Some(plan) = plan else { return Ok(false) };
            exists(
                conn,
                "SELECT 1 FROM events WHERE plan = ?1 AND id > ?2
                 AND (kind = 'plan-done' OR (kind = 'plan-phase' AND json_extract(data, '$.from') = 'done')) LIMIT 1",
                params![plan, id],
            )
        }
        Kind::JobFailed => later_of_job(conn, event, &["job-retried", "job-merged", "job-cancelled"]),
        Kind::AttemptNudge => {
            later_of_job(conn, event, &["attempt-started", "job-merged", "job-failed", "job-cancelled"])
        }
        Kind::Escalation => {
            if let (Some(plan), Some(escalation)) = (plan, event.data.get("escalation").and_then(Value::as_str)) {
                if exists(
                    conn,
                    "SELECT 1 FROM events WHERE plan = ?1 AND id > ?2
                     AND kind IN ('escalation-answered', 'escalation-expired')
                     AND json_extract(data, '$.escalation') = ?3 LIMIT 1",
                    params![plan, id, escalation],
                )? {
                    return Ok(true);
                }
            }
            later_of_job(conn, event, &["job-merged", "job-cancelled"])
        }
        Kind::Error => {
            if job.is_some() {
                return later_of_job(conn, event, &["attempt-started", "job-merged", "job-cancelled"]);
            }
            let pr = event.data.pointer("/details/pr").and_then(Value::as_str);
            match (plan, pr) {
                (Some(plan), Some(pr)) => exists(
                    conn,
                    "SELECT 1 FROM events WHERE plan = ?1 AND id > ?2
                     AND kind IN ('pr-started', 'pr-merged', 'pr-cancelled')
                     AND json_extract(data, '$.pr') = ?3 LIMIT 1",
                    params![plan, id, pr],
                ),
                _ => Ok(false),
            }
        }
        Kind::PrFeedback => pr_feedback(conn, event),
        _ => Ok(false),
    }
}

/// A later Event of one of `kinds` about the same Job of the same Plan.
fn later_of_job(conn: &Connection, event: &Event, kinds: &[&str]) -> Result<bool, CpdError> {
    let (Some(plan), Some(job)) = (event.plan.as_deref(), event.job.as_deref()) else { return Ok(false) };
    // The kind list is a fixed set of literals of this module, bound as one JSON array: no value is formatted into SQL.
    let kinds = Value::from(kinds).to_string();
    exists(
        conn,
        "SELECT 1 FROM events WHERE plan = ?1 AND id > ?2 AND job = ?3
         AND kind IN (SELECT value FROM json_each(?4)) LIMIT 1",
        params![plan, event.id, job, kinds],
    )
}

fn pr_feedback(conn: &Connection, event: &Event) -> Result<bool, CpdError> {
    let (Some(plan), Some(pr)) = (event.plan.as_deref(), event.data.get("pr").and_then(Value::as_str)) else {
        return Ok(false);
    };
    if exists(
        conn,
        "SELECT 1 FROM events WHERE plan = ?1 AND id > ?2 AND kind IN ('pr-merged', 'pr-cancelled')
         AND json_extract(data, '$.pr') = ?3 LIMIT 1",
        params![plan, event.id, pr],
    )? {
        return Ok(true);
    }
    let Some(reasons) = event.data.get("reasons").and_then(Value::as_array).filter(|r| !r.is_empty()) else {
        return Ok(false);
    };
    let Some(reasons) = reasons.iter().map(Value::as_str).collect::<Option<Vec<&str>>>() else { return Ok(false) };
    let row: Option<(String, Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT p.checks, p.review_decision, p.state, p.feedback FROM workflows
             JOIN pull_requests p ON p.id = workflows.pull_request WHERE workflows.plan = ?1 AND workflows.key = ?2",
            params![plan, pr],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((checks, review, state, feedback)) = row else { return Ok(false) };
    let review = review.unwrap_or_default();
    for reason in reasons {
        let holds = match reason {
            "checks_failed" => checks == "failed",
            "changes_requested" => review == "changes_requested",
            "comments" => match feedback.as_deref() {
                None => return Ok(false),
                Some(text) => match serde_json::from_str::<Value>(text) {
                    Ok(Value::Array(items)) => !items.is_empty(),
                    _ => return Ok(false),
                },
            },
            "closed" => state == "closed",
            "ready_to_post" => {
                let posted = exists(conn, "SELECT 1 FROM pr_posts WHERE plan = ?1 AND workflow = ?2 LIMIT 1", params![plan, pr])?;
                let id: i64 = conn.query_row(
                    "SELECT pull_request FROM workflows WHERE plan = ?1 AND key = ?2",
                    params![plan, pr],
                    |r| r.get(0),
                )?;
                let row = crate::pr::pulls::load(conn, id)?;
                !posted && crate::pr::postable::unready(&row).is_empty()
            }
            "approved" => state == "open" && review == "approved",
            _ => return Ok(false),
        };
        if holds {
            return Ok(false);
        }
    }
    Ok(true)
}

fn exists(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<bool, CpdError> {
    Ok(conn.query_row(sql, params, |_| Ok(())).optional()?.is_some())
}
