//! The `worker` view's snapshot (commands.md §17): an Attempt, its Worker
//! Session and its Escalations (`attempt show`). The Worker's session file is
//! not in the database: the view reads `session.file` itself, read-only.

use cpd_core::attempt::ledger::Stored;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use cpd_core::config::Tier;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::highest_id;
use cpd_core::ids::{AttemptId, EscalationId, JobKey, PlanId, SessionId};
use cpd_core::store::Store;

use super::flow::{WorkerRef, waiting_on};
use super::shared::{
    AttemptStatus, SHOWN_STATUS, EscalationStatus, JobStatus, Origin, Outcome, SessionNode, bad_row, id, stored_cause, sessions, tier,
};

/// What a `worker` view shows: an Attempt (its form) or a Job (its form, which has no Attempt until the Job starts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Attempt(AttemptId),
    Job(JobKey),
}

/// The Job of a snapshot, with what says why it is in its state (the header's data).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerJob {
    pub key: JobKey,
    pub title: String,
    pub status: JobStatus,
    pub cancel_reason: Option<String>,
    pub failed_cause: Option<String>,
    /// What a `queued` Job waits for, sorted; empty when it is Ready.
    pub waiting_on: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Worker {
    pub plan: PlanId,
    pub job: WorkerJob,
    /// The Attempt shown: the one named, or the Job's latest; none for a Job that has not started.
    pub attempt: Option<AttemptDetail>,
    /// The Worker's Session, once the driver has registered it.
    pub session: Option<SessionNode>,
    /// Every Escalation of the Attempt, oldest first.
    pub escalations: Vec<AttemptEscalation>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
}

impl Worker {
    pub fn worker_ref(&self) -> Option<WorkerRef> {
        self.session.as_ref().map(|s| WorkerRef {
            session: s.id.clone(),
            state: s.state,
            state_label: s.state_label.clone(),
            state_at: s.state_at.clone(),
        })
    }
}

/// Every field of an Attempt (§2.2).
#[derive(Clone, Debug, PartialEq)]
pub struct AttemptDetail {
    pub id: AttemptId,
    pub tier: Tier,
    pub origin: Origin,
    pub status: AttemptStatus,
    pub outcome: Option<Outcome>,
    pub cause: Option<Stored>,
    pub branch: String,
    pub worktree: String,
    pub worker_session: Option<SessionId>,
    pub spec_path: Option<String>,
    pub submitted_sha: Option<String>,
    pub submitted_at: Option<String>,
    pub check_tail: Option<String>,
    pub merged_sha: Option<String>,
    pub nudged: bool,
    pub stop_request: Option<String>,
    pub metrics: Option<Value>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptEscalation {
    pub id: EscalationId,
    pub question: String,
    pub status: EscalationStatus,
    pub answer: Option<String>,
    pub asked_at: String,
    pub answered_at: Option<String>,
    pub expired_at: Option<String>,
}

/// The snapshot of `target` in `plan`: `plan_not_found`, else `attempt_not_found` or `job_not_found`. A Job target
/// takes its latest Attempt in the same transaction.
pub fn worker(store: &mut Store, plan: &PlanId, target: &Target) -> Result<Worker, CpdError> {
    store.read(|conn| {
        let known: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM plans WHERE id = ?1)", [plan.as_str()], |r| r.get(0))?;
        if !known {
            return Err(CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str()));
        }
        let (job, attempt) = match target {
            Target::Attempt(attempt) => {
                let key: Option<String> = conn
                    .query_row("SELECT job FROM attempts WHERE plan = ?1 AND id = ?2", [plan.as_str(), &attempt.to_string()], |r| r.get(0))
                    .optional()?;
                let Some(key) = key else {
                    return Err(CpdError::new(ErrorCode::AttemptNotFound, format!("No Attempt {attempt} in Plan {plan}."))
                        .with("plan", plan.as_str())
                        .with("attempt", attempt.to_string()));
                };
                (id("Job key", &key, JobKey::parse)?, Some(attempt.clone()))
            }
            Target::Job(key) => (key.clone(), latest(conn, plan, key)?),
        };
        let job = job_row(conn, plan, &job)?;
        let detail = attempt.as_ref().map(|a| attempt_row(conn, plan, a)).transpose()?;
        let session = match detail.as_ref().and_then(|d| d.worker_session.as_ref()) {
            Some(s) => sessions(conn, "s.id = ?1", &[&s.as_str()])?.pop(),
            None => None,
        };
        Ok(Worker {
            plan: plan.clone(),
            escalations: match &attempt {
                Some(a) => escalations(conn, plan, a)?,
                None => Vec::new(),
            },
            attempt: detail,
            job,
            session,
            events_cursor: highest_id(conn)?,
        })
    })
}

/// The latest Attempt of Job `job` of `plan`: the one with the highest number; none for a Job that has not started.
pub fn latest_attempt(store: &mut Store, plan: &PlanId, job: &JobKey) -> Result<Option<AttemptId>, CpdError> {
    store.read(|conn| latest(conn, plan, job))
}

fn latest(conn: &Connection, plan: &PlanId, job: &JobKey) -> Result<Option<AttemptId>, CpdError> {
    let latest: Option<String> = conn
        .query_row(
            "SELECT id FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number DESC LIMIT 1",
            [plan.as_str(), job.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    latest.map(|latest| id("Attempt id", &latest, AttemptId::parse)).transpose()
}

fn job_row(conn: &Connection, plan: &PlanId, job: &JobKey) -> Result<WorkerJob, CpdError> {
    let row: Option<(String, String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT title, status, cancel_reason, failed_cause FROM jobs WHERE plan = ?1 AND key = ?2",
            [plan.as_str(), job.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((title, status, cancel_reason, failed_cause)) = row else {
        return Err(CpdError::new(ErrorCode::JobNotFound, format!("No Job {job} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("job", job.as_str()));
    };
    Ok(WorkerJob {
        key: job.clone(),
        title,
        status: JobStatus::parse(&format!("Job {plan}/{job} has status"), &status)?,
        cancel_reason,
        failed_cause,
        waiting_on: waiting_on(conn, plan, job)?,
    })
}

/// The Attempt `attempt`, which exists (its Job was found by it or is its Job's latest).
fn attempt_row(conn: &Connection, plan: &PlanId, attempt: &AttemptId) -> Result<AttemptDetail, CpdError> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT a.tier, a.origin, {SHOWN_STATUS}, a.outcome, a.cause, a.branch, a.worktree, a.worker_session, a.spec_path,
                a.submitted_sha, a.submitted_at, a.check_tail, a.merged_sha, a.nudged, a.stop_request, a.metrics, a.started_at, a.ended_at
           FROM attempts a WHERE a.plan = ?1 AND a.id = ?2"
        ),
    )?;
    let mut rows = stmt.query([plan.as_str(), &attempt.to_string()])?;
    let Some(r) = rows.next()? else {
        return Err(CpdError::new(ErrorCode::AttemptNotFound, format!("No Attempt {attempt} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("attempt", attempt.to_string()));
    };
    let what = format!("Attempt {plan}/{attempt}");
    let (tier_text, origin, status, outcome, cause): (String, String, String, Option<String>, Option<String>) =
        (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
    let (worker, metrics): (Option<String>, Option<String>) = (r.get(7)?, r.get(15)?);
    Ok(AttemptDetail {
        id: attempt.clone(),
        tier: tier(&what, &tier_text)?,
        origin: Origin::parse(&format!("{what} has origin"), &origin)?,
        status: AttemptStatus::parse(&format!("{what} has status"), &status)?,
        outcome: outcome.map(|o| Outcome::parse(&format!("{what} has outcome"), &o)).transpose()?,
        cause: cause.map(|c| stored_cause(&format!("{what} has cause"), &c)).transpose()?,
        branch: r.get(5)?,
        worktree: r.get(6)?,
        worker_session: worker.map(|w| id("Session id", &w, SessionId::parse)).transpose()?,
        spec_path: r.get(8)?,
        submitted_sha: r.get(9)?,
        submitted_at: r.get(10)?,
        check_tail: r.get(11)?,
        merged_sha: r.get(12)?,
        nudged: r.get(13)?,
        stop_request: r.get(14)?,
        metrics: metrics
            .map(|m| serde_json::from_str(&m).map_err(|_| bad_row(format!("{what} has metrics {m:?}"))))
            .transpose()?,
        started_at: r.get(16)?,
        ended_at: r.get(17)?,
    })
}

fn escalations(conn: &Connection, plan: &PlanId, attempt: &AttemptId) -> Result<Vec<AttemptEscalation>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, question, status, answer, asked_at, answered_at, expired_at FROM escalations
          WHERE plan = ?1 AND attempt = ?2 ORDER BY number",
    )?;
    let mut rows = stmt.query([plan.as_str(), &attempt.to_string()])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (eid, status): (String, String) = (r.get(0)?, r.get(2)?);
        out.push(AttemptEscalation {
            status: EscalationStatus::parse(&format!("Escalation {eid} has status"), &status)?,
            id: id("Escalation id", &eid, EscalationId::parse)?,
            question: r.get(1)?,
            answer: r.get(3)?,
            asked_at: r.get(4)?,
            answered_at: r.get(5)?,
            expired_at: r.get(6)?,
        });
    }
    Ok(out)
}
