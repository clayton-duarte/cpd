//! What the driver reads from and writes to the Ledger (contract §3.2, §3.4,
//! §5.2): the Attempt it drives, with its Plan, and the transactions that end
//! an Attempt before it is `merging`. A `merging` Attempt is ended by the
//! merge (`crate::merge`), never here.

use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::config::{Catalog, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::ids::{AttemptId, ContextId, JobKey, PlanId, SessionId};
use crate::store::WriteTxn;

/// The `txns.command` and the `source` of the driver's `error` Events.
pub const SOURCE: &str = "attempt run";

/// An Attempt with what the driver needs of its Plan.
#[derive(Clone, Debug)]
pub struct Target {
    pub plan: PlanId,
    pub context: ContextId,
    /// The repo of the Job's PR: the realpath of its main worktree.
    pub repo: PathBuf,
    /// The Lead Branch of the Job's PR (R34 A5).
    pub lead_branch: String,
    pub lead_worktree: PathBuf,
    pub id: AttemptId,
    pub job: JobKey,
    pub tier: Tier,
    pub status: String,
    pub branch: String,
    pub worktree: PathBuf,
    pub worker_session: Option<SessionId>,
    pub nudged: bool,
    pub stop_request: Option<String>,
}

fn corrupt(what: String) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The Ledger holds an invalid value: {what}."))
}

/// Attempt `id` of `plan`: `plan_not_found`, else `attempt_not_found` (exit 4).
pub fn load(conn: &Connection, plan: &PlanId, id: &AttemptId) -> Result<Target, CpdError> {
    let found: Option<String> =
        conn.query_row("SELECT context FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0)).optional()?;
    let context = found.ok_or_else(|| CpdError::plan_not_found(plan))?;
    // The repo and Lead worktree are those of the Job's PR (R34 A7).
    let row = conn
        .query_row(
            "SELECT a.job, a.tier, a.status, a.branch, a.worktree, a.worker_session, a.nudged, a.stop_request,
                    r.repo, r.lead_worktree, r.lead_branch
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
               JOIN prs r ON r.plan = j.plan AND r.key = j.pr
              WHERE a.plan = ?1 AND a.id = ?2",
            params![plan.as_str(), id.to_string()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    (r.get::<_, String>(8)?, r.get::<_, Option<String>>(9)?, r.get::<_, String>(10)?),
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            CpdError::new(ErrorCode::AttemptNotFound, format!("No Attempt {id} in Plan {plan}."))
                .with("plan", plan.as_str())
                .with("attempt", id.to_string())
        })?;
    let (job, tier, status, branch, worktree, worker_session, nudged, stop_request, (repo, lead_worktree, lead_branch)) = row;
    let lead_worktree = lead_worktree.ok_or_else(|| corrupt(format!("Plan {plan} has an Attempt but no Lead worktree")))?;
    Ok(Target {
        plan: plan.clone(),
        context: ContextId::parse(&context).map_err(|_| corrupt(format!("context id {context:?}")))?,
        repo: PathBuf::from(repo),
        lead_branch,
        lead_worktree: PathBuf::from(lead_worktree),
        id: id.clone(),
        job: JobKey::parse(&job).map_err(|_| corrupt(format!("job key {job:?}")))?,
        tier: Tier::parse(&tier).map_err(|_| corrupt(format!("tier {tier:?} of Attempt {id}")))?,
        status,
        branch,
        worktree: PathBuf::from(worktree),
        worker_session: worker_session
            .map(|s| SessionId::parse(&s).map_err(|_| corrupt(format!("session id {s:?}"))))
            .transpose()?,
        nudged,
        stop_request,
    })
}

/// The Job's cancel reason when a cancel was requested (§3.2 "Cancel").
pub fn cancel_request(conn: &Connection, plan: &PlanId, job: &JobKey) -> Result<Option<String>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT cancel_reason FROM jobs WHERE plan = ?1 AND key = ?2 AND cancel_requested_at IS NOT NULL",
            params![plan.as_str(), job.as_str()],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// The causes the driver ends an Attempt with before its merge (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// A1–A2: the worktree or `pre-start` failed.
    SetupFailed,
    /// A5 after E2 set `stop_request = timed_out`.
    TimedOut,
    /// The Worker pane is dead, pi stopped badly, or the driver failed.
    Crashed,
    /// A5: idle twice without a submit, nudged in between.
    NoSubmit,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::SetupFailed => "setup_failed",
            Cause::TimedOut => "timed_out",
            Cause::Crashed => "crashed",
            Cause::NoSubmit => "no_submit",
        }
    }

    /// A ladder cause sends the Job back to `queued` while the ladder allows (§3.4, §9).
    pub fn is_ladder(self) -> bool {
        matches!(self, Cause::Crashed | Cause::NoSubmit)
    }
}

/// How an Attempt that is not `merging` ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// `cancelled` / `cancel_requested`.
    Cancelled,
    /// `failed` / `cause`; `detail` explains it.
    Failed { cause: Cause, detail: String },
}

impl End {
    pub fn outcome(&self) -> &'static str {
        match self {
            End::Cancelled => "cancelled",
            End::Failed { .. } => "failed",
        }
    }

    pub fn cause(&self) -> &'static str {
        match self {
            End::Cancelled => "cancel_requested",
            End::Failed { cause, .. } => cause.as_str(),
        }
    }
}

/// What the ladder needs (§9): pi's catalog and `retry_cap`.
pub struct Ladder<'a> {
    pub catalog: &'a Catalog,
    pub retry_cap: u32,
}

/// Ends `target`, which must still be in the status it was read in
/// (`starting` or `running`), and moves its Job out of `running`, in the
/// caller's transaction (§3.2, §3.4): `cas_lost` when either changed. Records
/// `metrics` (A9) and appends the Event of §3.4's table: `job-cancelled`,
/// `attempt-retry` or `job-failed`.
pub fn end(tx: &WriteTxn<'_>, target: &Target, end: &End, metrics: Option<&Value>, ladder: &Ladder<'_>) -> Result<(), CpdError> {
    let (plan, job, id) = (target.plan.as_str(), target.job.as_str(), target.id.to_string());
    let cas_lost = |what: &str| {
        CpdError::new(ErrorCode::CasLost, format!("{what} changed while the driver ran; nothing was written."))
            .with("plan", plan)
            .with("attempt", id.clone())
    };
    if !matches!(target.status.as_str(), "starting" | "running") {
        return Err(cas_lost("The Attempt's status").with("status", target.status.clone()));
    }
    let changed = tx.execute(
        "UPDATE attempts SET status = 'ended', outcome = ?4, cause = ?5, metrics = ?6,
                ended_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE plan = ?1 AND id = ?2 AND status = ?3",
        params![plan, id, target.status, end.outcome(), end.cause(), metrics.map(Value::to_string)],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Attempt's status"));
    }
    let tier = target.tier.to_string();
    let show = format!("cpd job show {job} --plan {plan}");
    let (status, failed_cause, event) = match end {
        End::Cancelled => {
            let reason = cancel_request(tx, &target.plan, &target.job)?.ok_or_else(|| cas_lost("The Job's cancel request"))?;
            let text = format!("Job {job} cancelled ({reason}): its driver stopped Attempt {id}. Next: cpd plan show --plan {plan}");
            ("cancelled", None, NewEvent::job_cancelled(text, &reason))
        }
        End::Failed { cause, detail } => match next_tier(tx, target, *cause, ladder)? {
            Some(next) => {
                let text = format!(
                    "Attempt {id} of Job {job} failed ({}): {detail} The Job is queued again at Tier {next}. Next: {show}",
                    cause.as_str()
                );
                ("queued", None, NewEvent::attempt_retry(text, "failed", cause.as_str(), &next.to_string(), detail))
            }
            None => {
                let attempts: i64 =
                    tx.query_row("SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2", params![plan, job], |r| r.get(0))?;
                let text = format!(
                    "Job {job} failed ({}): {detail} Decide: cpd job retry {job} --tier <T> --plan {plan}, or cpd job cancel {job} --reason <text> --plan {plan}",
                    cause.as_str()
                );
                ("failed", Some(cause.as_str()), NewEvent::job_failed(text, cause.as_str(), detail, &tier, attempts))
            }
        },
    };
    let changed = tx.execute(
        "UPDATE jobs SET status = ?3, failed_cause = ?4 WHERE plan = ?1 AND key = ?2 AND status = 'running'",
        params![plan, job, status, failed_cause],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Job's status"));
    }
    events::append(tx, event.plan(target.plan.clone()).job(target.job.clone()).attempt(target.id.clone()))?;
    Ok(())
}

/// The automatic Attempts of Job `job` since the last `job retry` (that one
/// included), for `retry_cap` (§9). An ended Attempt whose driver never started
/// (`driver_pid` null, §3.4) is not one: it does not use up a try.
pub(crate) fn automatic_attempts(tx: &WriteTxn<'_>, plan: &str, job: &str) -> Result<i64, CpdError> {
    Ok(tx.query_row(
        "SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2
           AND number >= coalesce((SELECT max(number) FROM attempts WHERE plan = ?1 AND job = ?2 AND origin = 'retry'), 1)
           AND NOT (status = 'ended' AND driver_pid IS NULL)",
        params![plan, job],
        |r| r.get(0),
    )?)
}

/// The ladder (§3.2, §9): when `cause` is a ladder cause, the next Tier if the
/// ladder has one and the automatic Attempts since the last `job retry` are
/// fewer than `1 + retry_cap`. An Attempt whose driver never started
/// (`driver_pid` null, §3.4) does not raise the Tier: its own Tier again.
fn next_tier(tx: &WriteTxn<'_>, target: &Target, cause: Cause, ladder: &Ladder<'_>) -> Result<Option<Tier>, CpdError> {
    if !cause.is_ladder() {
        return Ok(None);
    }
    let never_started: bool = tx.query_row(
        "SELECT driver_pid IS NULL FROM attempts WHERE plan = ?1 AND id = ?2",
        params![target.plan.as_str(), target.id.to_string()],
        |r| r.get(0),
    )?;
    if never_started {
        return Ok(Some(target.tier));
    }
    if automatic_attempts(tx, target.plan.as_str(), target.job.as_str())? >= 1 + i64::from(ladder.retry_cap) {
        return Ok(None);
    }
    ladder.catalog.next(target.tier)
}

/// Appends an `error` Event about `target` (§5.2): an unexpected failure of
/// the driver (commands.md §6.4).
pub fn append_error(tx: &WriteTxn<'_>, target: &Target, error: &CpdError) -> Result<(), CpdError> {
    let text = format!(
        "The driver of Attempt {} failed: {} See: cpd attempt show {} --plan {}",
        target.id,
        error.message(),
        target.id,
        target.plan
    );
    events::append(
        tx,
        NewEvent::error(text, SOURCE, error.code().as_str(), error.message(), error.details()).plan(target.plan.clone()).job(target.job.clone()).attempt(target.id.clone()),
    )?;
    Ok(())
}
