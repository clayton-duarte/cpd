//! The `attempts` and `escalations` rows as read (contract §2.2 Attempt and Escalation).

use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use serde_json::{Value, json};

use crate::error::{CpdError, ErrorCode};
use crate::ids::{AttemptId, JobKey, PlanId};

/// One Attempt with every field of §2.2.
#[derive(Clone, Debug)]
pub(super) struct Attempt {
    pub plan: PlanId,
    pub id: String,
    pub job: String,
    pub number: i64,
    pub tier: String,
    pub origin: String,
    pub status: String,
    pub outcome: Option<String>,
    pub cause: Option<String>,
    pub branch: String,
    pub worktree: String,
    pub base_sha: Option<String>,
    pub driver_pid: Option<i64>,
    pub worker_session: Option<String>,
    pub spec_path: Option<String>,
    pub submitted_sha: Option<String>,
    pub submitted_at: Option<String>,
    pub check_tail: Option<String>,
    pub merged_sha: Option<String>,
    pub nudged: bool,
    pub stop_request: Option<String>,
    pub metrics: Value,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub runner: String,
    pub worker_pid: Option<i64>,
    pub gate_hook: Option<String>,
    pub gate_command: Option<String>,
    pub gate_exit: Option<i64>,
    pub actual_files: Value,
}

impl Attempt {
    /// Every field of the Attempt (commands.md §6.3 `attempt show`).
    pub fn to_json(&self) -> Value {
        json!({
            "plan": self.plan, "id": self.id, "job": self.job, "number": self.number, "tier": self.tier,
            "origin": self.origin, "status": self.status, "outcome": self.outcome, "cause": self.cause,
            "branch": self.branch, "worktree": self.worktree, "base_sha": self.base_sha,
            "driver_pid": self.driver_pid, "worker_session": self.worker_session, "spec_path": self.spec_path,
            "submitted_sha": self.submitted_sha, "submitted_at": self.submitted_at, "check_tail": self.check_tail,
            "merged_sha": self.merged_sha, "nudged": self.nudged, "stop_request": self.stop_request,
            "metrics": self.metrics, "started_at": self.started_at, "ended_at": self.ended_at,
            "runner": self.runner, "worker_pid": self.worker_pid, "gate_hook": self.gate_hook,
            "gate_command": self.gate_command, "gate_exit": self.gate_exit, "actual_files": self.actual_files,
        })
    }
}

const COLUMNS: &str = "id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha, driver_pid,
    worker_session, spec_path, submitted_sha, submitted_at, check_tail, merged_sha, nudged, stop_request, metrics,
    started_at, ended_at, runner, worker_pid, gate_hook, gate_command, gate_exit, actual_files";

/// One row; `metrics` and `actual_files` are still text.
fn raw(plan: &PlanId, r: &Row<'_>) -> rusqlite::Result<(Attempt, Option<String>, Option<String>)> {
    let attempt = Attempt {
        plan: plan.clone(),
        id: r.get(0)?,
        job: r.get(1)?,
        number: r.get(2)?,
        tier: r.get(3)?,
        origin: r.get(4)?,
        status: r.get(5)?,
        outcome: r.get(6)?,
        cause: r.get(7)?,
        branch: r.get(8)?,
        worktree: r.get(9)?,
        base_sha: r.get(10)?,
        driver_pid: r.get(11)?,
        worker_session: r.get(12)?,
        spec_path: r.get(13)?,
        submitted_sha: r.get(14)?,
        submitted_at: r.get(15)?,
        check_tail: r.get(16)?,
        merged_sha: r.get(17)?,
        nudged: r.get(18)?,
        stop_request: r.get(19)?,
        metrics: Value::Null,
        started_at: r.get(21)?,
        ended_at: r.get(22)?,
        runner: r.get(23)?,
        worker_pid: r.get(24)?,
        gate_hook: r.get(25)?,
        gate_command: r.get(26)?,
        gate_exit: r.get(27)?,
        actual_files: Value::Null,
    };
    Ok((attempt, r.get(20)?, r.get(28)?))
}

/// The Attempts of `plan` that satisfy `filter` (SQL over `attempts`, with `?2…`
/// bound to `args`), in creation order.
fn query(conn: &Connection, plan: &PlanId, filter: &str, args: &[&str]) -> Result<Vec<Attempt>, CpdError> {
    let sql = format!("SELECT {COLUMNS} FROM attempts WHERE plan = ?1 AND {filter} ORDER BY rowid");
    let mut stmt = conn.prepare(&sql)?;
    let bound = std::iter::once(plan.as_str()).chain(args.iter().copied());
    let rows = stmt.query_map(params_from_iter(bound), |r| raw(plan, r))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(mut attempt, metrics, files)| {
            attempt.metrics = match metrics {
                None => Value::Null,
                Some(text) => serde_json::from_str(&text)
                    .map_err(|_| CpdError::bad_row(format!("Attempt {plan}/{} has metrics {text:?}", attempt.id)))?,
            };
            attempt.actual_files = match files {
                None => Value::Null,
                Some(text) => serde_json::from_str(&text)
                    .map_err(|_| CpdError::bad_row(format!("Attempt {plan}/{} has actual_files {text:?}", attempt.id)))?,
            };
            Ok(attempt)
        })
        .collect()
}

/// `plan_not_found` unless the Plan exists.
pub(super) fn require_plan(conn: &Connection, plan: &PlanId) -> Result<(), CpdError> {
    let found: Option<i64> = conn.query_row("SELECT 1 FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0)).optional()?;
    found.map(|_| ()).ok_or_else(|| {
        CpdError::plan_not_found(plan)
    })
}

/// Attempt `id` of `plan`: `plan_not_found`, else `attempt_not_found` (exit 4).
pub(super) fn load(conn: &Connection, plan: &PlanId, id: &AttemptId) -> Result<Attempt, CpdError> {
    require_plan(conn, plan)?;
    query(conn, plan, "id = ?2", &[&id.to_string()])?.into_iter().next().ok_or_else(|| {
        CpdError::new(ErrorCode::AttemptNotFound, format!("No Attempt {id} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("attempt", id.to_string())
    })
}

/// The Attempts of the Plan, oldest first: of one Job (`job_not_found` when the
/// Job does not exist), and only those not `ended` when `active`.
pub(super) fn list(conn: &Connection, plan: &PlanId, job: Option<&JobKey>, active: bool) -> Result<Vec<Attempt>, CpdError> {
    require_plan(conn, plan)?;
    if let Some(job) = job {
        let found: Option<i64> = conn
            .query_row("SELECT 1 FROM jobs WHERE plan = ?1 AND key = ?2", [plan.as_str(), job.as_str()], |r| r.get(0))
            .optional()?;
        if found.is_none() {
            return Err(CpdError::new(ErrorCode::JobNotFound, format!("No Job {job} in Plan {plan}."))
                .with("plan", plan.as_str())
                .with("job", job.as_str()));
        }
    }
    let mut attempts = match job {
        Some(job) => query(conn, plan, "job = ?2", &[job.as_str()])?,
        None => query(conn, plan, "1", &[])?,
    };
    if active {
        attempts.retain(|a| a.status != "ended");
    }
    Ok(attempts)
}

/// The Attempt's Escalations, in order.
pub(super) fn escalations(conn: &Connection, plan: &PlanId, attempt: &AttemptId) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at
           FROM escalations WHERE plan = ?1 AND attempt = ?2 ORDER BY number",
    )?;
    let rows = stmt
        .query_map([plan.as_str(), &attempt.to_string()], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "question": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?, "answer": r.get::<_, Option<String>>(4)?,
                "asker_pid": r.get::<_, i64>(5)?, "asked_at": r.get::<_, String>(6)?,
                "answered_at": r.get::<_, Option<String>>(7)?, "expired_at": r.get::<_, Option<String>>(8)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
