//! The `worker` view's snapshot (commands.md §17): an Attempt, its Worker
//! Session and its Escalations (`attempt show`). The Worker's session file is
//! not in the database: the view reads `session.file` itself, read-only.

use rusqlite::Connection;
use serde_json::Value;

use cpd_core::config::Tier;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::highest_id;
use cpd_core::ids::{AttemptId, EscalationId, PlanId, SessionId};
use cpd_core::store::Store;

use super::flow::WorkerRef;
use super::shared::{
    AttemptStatus, Cause, EscalationStatus, JobStatus, Origin, Outcome, SessionNode, bad_row, id, sessions, tier,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Worker {
    pub plan: PlanId,
    pub attempt: AttemptDetail,
    pub job_title: String,
    pub job_status: JobStatus,
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
    pub cause: Option<Cause>,
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

/// The snapshot of Attempt `attempt` of `plan`: `plan_not_found`, else `attempt_not_found`.
pub fn worker(store: &mut Store, plan: &PlanId, attempt: &AttemptId) -> Result<Worker, CpdError> {
    store.read(|conn| {
        let (detail, job_title, job_status) = attempt_row(conn, plan, attempt)?;
        let session = match &detail.worker_session {
            Some(s) => sessions(conn, "s.id = ?1", &[&s.as_str()])?.pop(),
            None => None,
        };
        Ok(Worker {
            plan: plan.clone(),
            escalations: escalations(conn, plan, attempt)?,
            attempt: detail,
            job_title,
            job_status,
            session,
            events_cursor: highest_id(conn)?,
        })
    })
}

fn attempt_row(conn: &Connection, plan: &PlanId, attempt: &AttemptId) -> Result<(AttemptDetail, String, JobStatus), CpdError> {
    let known: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM plans WHERE id = ?1)", [plan.as_str()], |r| r.get(0))?;
    if !known {
        return Err(CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str()));
    }
    let mut stmt = conn.prepare(
        "SELECT a.tier, a.origin, a.status, a.outcome, a.cause, a.branch, a.worktree, a.worker_session, a.spec_path,
                a.submitted_sha, a.submitted_at, a.check_tail, a.merged_sha, a.nudged, a.stop_request, a.metrics,
                a.started_at, a.ended_at, j.title, j.status
           FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE a.plan = ?1 AND a.id = ?2",
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
    let (worker, metrics, job_status): (Option<String>, Option<String>, String) = (r.get(7)?, r.get(15)?, r.get(19)?);
    let detail = AttemptDetail {
        id: attempt.clone(),
        tier: tier(&what, &tier_text)?,
        origin: Origin::parse(&format!("{what} has origin"), &origin)?,
        status: AttemptStatus::parse(&format!("{what} has status"), &status)?,
        outcome: outcome.map(|o| Outcome::parse(&format!("{what} has outcome"), &o)).transpose()?,
        cause: cause.map(|c| Cause::parse(&format!("{what} has cause"), &c)).transpose()?,
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
    };
    let job_status = JobStatus::parse(&format!("the Job of {what} has status"), &job_status)?;
    Ok((detail, r.get(18)?, job_status))
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
