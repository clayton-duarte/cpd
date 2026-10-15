//! The `plan` view's snapshot (commands.md §17): a Plan's Jobs with their
//! Dependencies, Attempts and Tiers, its open Escalations, its latest Plan
//! review and its PR (`plan show`); and which Plan the chat slot holds.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};

use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::events::highest_id;
use crate::ids::{AttemptId, EscalationId, JobKey, PlanId, SessionId};
use crate::plan::jobs;
use crate::session::State;
use crate::store::Store;

use super::shared::{
    AttemptStatus, Cause, JobStatus, Origin, Outcome, PlanHead, ReviewStatus, bad_row, id, plan_heads, tier, value_enum,
};

/// A Plan, for the flow chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub head: PlanHead,
    /// In creation order.
    pub jobs: Vec<JobNode>,
    /// The Plan's `open` Escalations, oldest first.
    pub escalations: Vec<OpenEscalation>,
    /// The latest Plan review, if there is one.
    pub review: Option<Review>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobNode {
    pub key: JobKey,
    pub seq: i64,
    pub title: String,
    pub status: JobStatus,
    pub tier: Tier,
    /// Keys of the Jobs it needs, sorted.
    pub needs: Vec<JobKey>,
    /// Added after the Plan was locked (PR feedback).
    pub appended: bool,
    pub retry_tier: Option<Tier>,
    pub failed_cause: Option<String>,
    pub cancel_reason: Option<String>,
    pub merged_sha: Option<String>,
    /// Oldest first.
    pub attempts: Vec<AttemptNode>,
}

impl JobNode {
    /// The Attempt not `ended`, if there is one (at most one exists).
    pub fn active_attempt(&self) -> Option<&AttemptNode> {
        self.attempts.iter().find(|a| a.status != AttemptStatus::Ended)
    }

    /// The Attempt a pill shows: the active one, else the last.
    pub fn current_attempt(&self) -> Option<&AttemptNode> {
        self.active_attempt().or(self.attempts.last())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptNode {
    pub id: AttemptId,
    pub number: u32,
    pub tier: Tier,
    pub origin: Origin,
    pub status: AttemptStatus,
    pub outcome: Option<Outcome>,
    pub cause: Option<Cause>,
    pub worker: Option<WorkerRef>,
    pub started_at: String,
    pub submitted_at: Option<String>,
    pub ended_at: Option<String>,
}

/// An Attempt's Worker Session and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerRef {
    pub session: SessionId,
    pub state: State,
    pub state_label: Option<String>,
    pub state_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenEscalation {
    pub id: EscalationId,
    pub attempt: AttemptId,
    pub question: String,
    pub asked_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub number: u32,
    pub status: ReviewStatus,
    pub fingerprint: String,
    /// Its fingerprint is still the plan fingerprint of the draft Jobs (§2.3).
    pub current: bool,
    pub feedback: Option<String>,
    pub requested_at: String,
    pub decided_at: Option<String>,
}

fn plan_not_found(plan: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str())
}

/// The snapshot of `plan` (`plan show`): `plan_not_found` when it does not exist.
pub fn flow(store: &mut Store, plan: &PlanId) -> Result<Flow, CpdError> {
    store.read(|conn| {
        let head = plan_heads(conn, Some(plan))?.pop().ok_or_else(|| plan_not_found(plan))?;
        let rows = jobs::load(conn, plan)?;
        let mut attempts = attempts(conn, plan)?;
        let review = latest_review(conn, plan, &jobs::plan_fingerprint(&rows))?;
        let nodes = rows
            .into_iter()
            .map(|j| {
                let what = format!("Job {plan}/{}", j.key);
                let key = id("Job key", &j.key, JobKey::parse)?;
                Ok(JobNode {
                    status: JobStatus::parse(&format!("{what} has status"), &j.status)?,
                    tier: tier(&what, &j.tier)?,
                    needs: j.needs.iter().map(|n| id("Job key", n, JobKey::parse)).collect::<Result<_, _>>()?,
                    retry_tier: j.retry_tier.as_deref().map(|t| tier(&what, t)).transpose()?,
                    attempts: attempts.remove(j.key.as_str()).unwrap_or_default(),
                    seq: j.seq,
                    title: j.title,
                    appended: j.appended,
                    failed_cause: j.failed_cause,
                    cancel_reason: j.cancel_reason,
                    merged_sha: j.merged_sha,
                    key,
                })
            })
            .collect::<Result<Vec<_>, CpdError>>()?;
        Ok(Flow { head, jobs: nodes, escalations: open_escalations(conn, plan)?, review, events_cursor: highest_id(conn)? })
    })
}

/// Every Attempt of the Plan by Job key, oldest first, with its Worker's state.
fn attempts(conn: &Connection, plan: &PlanId) -> Result<BTreeMap<String, Vec<AttemptNode>>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT a.job, a.id, a.number, a.tier, a.origin, a.status, a.outcome, a.cause, a.started_at, a.submitted_at,
                a.ended_at, a.worker_session, s.state, s.state_label, s.state_at
           FROM attempts a LEFT JOIN sessions s ON s.id = a.worker_session
          WHERE a.plan = ?1 ORDER BY a.job, a.number",
    )?;
    let mut rows = stmt.query([plan.as_str()])?;
    let mut by_job: BTreeMap<String, Vec<AttemptNode>> = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let (job, aid): (String, String) = (r.get(0)?, r.get(1)?);
        let attempt_id = id("Attempt id", &aid, AttemptId::parse)?;
        let what = format!("Attempt {plan}/{aid}");
        let (tier_text, origin, status): (String, String, String) = (r.get(3)?, r.get(4)?, r.get(5)?);
        let (outcome, cause): (Option<String>, Option<String>) = (r.get(6)?, r.get(7)?);
        let worker = match r.get::<_, Option<String>>(11)? {
            None => None,
            Some(session) => {
                let state: Option<String> = r.get(12)?;
                let state = state.ok_or_else(|| bad_row(format!("{what} has Worker {session}, which is no Session")))?;
                Some(WorkerRef {
                    session: id("Session id", &session, SessionId::parse)?,
                    state: value_enum(&format!("{what} has Worker state"), &state)?,
                    state_label: r.get(13)?,
                    state_at: r.get(14)?,
                })
            }
        };
        by_job.entry(job).or_default().push(AttemptNode {
            number: attempt_id.number().get(),
            tier: tier(&what, &tier_text)?,
            origin: Origin::parse(&format!("{what} has origin"), &origin)?,
            status: AttemptStatus::parse(&format!("{what} has status"), &status)?,
            outcome: outcome.map(|o| Outcome::parse(&format!("{what} has outcome"), &o)).transpose()?,
            cause: cause.map(|c| Cause::parse(&format!("{what} has cause"), &c)).transpose()?,
            started_at: r.get(8)?,
            submitted_at: r.get(9)?,
            ended_at: r.get(10)?,
            worker,
            id: attempt_id,
        });
    }
    Ok(by_job)
}

fn open_escalations(conn: &Connection, plan: &PlanId) -> Result<Vec<OpenEscalation>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, attempt, question, asked_at FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id",
    )?;
    let mut rows = stmt.query([plan.as_str()])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(open_escalation(r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)?);
    }
    Ok(out)
}

/// An `open` Escalation row, parsed.
pub(super) fn open_escalation(eid: String, attempt: String, question: String, asked_at: String) -> Result<OpenEscalation, CpdError> {
    Ok(OpenEscalation {
        id: id("Escalation id", &eid, EscalationId::parse)?,
        attempt: id("Attempt id", &attempt, AttemptId::parse)?,
        question,
        asked_at,
    })
}

fn latest_review(conn: &Connection, plan: &PlanId, plan_fingerprint: &str) -> Result<Option<Review>, CpdError> {
    let row = conn
        .query_row(
            "SELECT id, number, status, fingerprint, feedback, requested_at, decided_at FROM plan_reviews
              WHERE plan = ?1 ORDER BY number DESC LIMIT 1",
            [plan.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?;
    row.map(|(rid, number, status, fingerprint, feedback, requested_at, decided_at)| {
        Ok(Review {
            status: ReviewStatus::parse(&format!("Plan review {plan}/{rid} has status"), &status)?,
            current: fingerprint == plan_fingerprint,
            id: rid,
            number,
            fingerprint,
            feedback,
            requested_at,
            decided_at,
        })
    })
    .transpose()
}

/// The Lead Session in the cockpit's chat slot: the Plan Lead whose latest
/// `session-moved` Event says `place: "chat"` (§13.1, §17); the newest when
/// there are several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSlot {
    pub session: SessionId,
    pub name: String,
    pub plan: PlanId,
    /// The id of that `session-moved` Event.
    pub moved_event: i64,
}

/// The chat slot's Lead, `None` while the placeholder holds it. The `plan` view
/// switches Plan when this changes.
pub fn chat_slot(store: &mut Store) -> Result<Option<ChatSlot>, CpdError> {
    store.read(|conn| {
        let row: Option<(String, String, String, i64)> = conn
            .query_row(
                "SELECT s.id, s.name, s.plan, m.id FROM sessions s
                   JOIN events m ON m.id = (SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved')
                  WHERE s.role = 'lead' AND s.plan IS NOT NULL AND json_extract(m.data, '$.place') = 'chat'
                  ORDER BY m.id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        row.map(|(session, name, plan, moved_event)| {
            Ok(ChatSlot {
                session: id("Session id", &session, SessionId::parse)?,
                plan: id("Plan id", &plan, PlanId::parse)?,
                name,
                moved_event,
            })
        })
        .transpose()
    })
}
