//! `job cancel` (commands.md §5.3, contract §3.2 "Cancel").

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{CancelArgs, Env, find, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};
use crate::store::WriteTxn;

const COMMAND: &str = "job cancel";

/// What a cancel cascade changed: the Jobs it newly `failed` and the Jobs it newly `cancelled`, in creation
/// order (contract §3.2 "Cancel", hs-d647).
#[derive(Default)]
pub(crate) struct Failed {
    pub failed: Vec<String>,
    pub cancelled: Vec<String>,
}

enum Decision {
    /// Already `cancelled`, or a request is already recorded.
    Already(Job),
    /// `draft`, `queued` or `failed`: `status = cancelled`.
    Cancel(Job),
    /// `running`: record the request against the active Attempt.
    Request(Job, AttemptId),
}

/// Whether the cancel keeps the shape of a `tdd` Workflow (`shape_incomplete`). Only `plan cancel` skips it, by calling
/// [`cancel_without_shape`]: it is a parameter of the function, never a flag or an option of the command.
#[derive(Clone, Copy)]
pub(super) enum Shape {
    Keep,
    Skip,
}

pub(super) fn cancel(args: CancelArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    cancel_with(&plan, args.key, args.reason.as_str(), Shape::Keep)
}

/// `job cancel` as `plan cancel` runs it: no Job is refused `shape_incomplete`, the Plan goes away whole.
pub(crate) fn cancel_without_shape(
    plan: &PlanId,
    key: JobKey,
    reason: &str,
) -> Result<Output, CpdError> {
    cancel_with(plan, key, reason, Shape::Skip)
}

fn cancel_with(plan: &PlanId, key: JobKey, reason: &str, shape: Shape) -> Result<Output, CpdError> {
    cancel_in(&mut Env::open()?, plan, key, reason, shape)
}

/// [`cancel_with`] on an open `env`: what a test hands in.
pub(super) fn cancel_in(
    env: &mut Env,
    plan: &PlanId,
    key: JobKey,
    reason: &str,
    shape: Shape,
) -> Result<Output, CpdError> {
    let (plan, reason) = (plan.clone(), reason.to_owned());

    let decision = env.store.read(|conn| decide(conn, &plan, &key, shape))?;
    let (status, requested, already, failed) = match decision {
        Decision::Already(job) => (job.status.clone(), job.status == "running", true, Failed::default()),
        _ => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, shape)? {
            Decision::Already(job) => Ok((job.status.clone(), job.status == "running", true, Failed::default())),
            Decision::Cancel(job) => {
                tx.execute(
                    "UPDATE jobs SET status = 'cancelled', failed_cause = NULL,
                            cancel_reason = coalesce(cancel_reason, ?3),
                            cancel_requested_at = coalesce(cancel_requested_at, (SELECT at FROM txns WHERE id = cpd_txn()))
                      WHERE plan = ?1 AND key = ?2",
                    [plan.as_str(), job.key.as_str(), reason.as_str()],
                )?;
                let text = format!("Job {key} of Plan {plan} was cancelled: {reason} Next: cpd plan show --plan {plan}");
                append(tx, NewEvent::job_cancelled(text, &reason).plan(plan.clone()).job(key.clone()))?;
                // The dependents fail or are cancelled in the same transaction (contract §3.2 "Cancel", hs-d647).
                let failed = fail_dependents(tx, &plan, &key, &reason)?;
                crate::plan::done::settle_done(tx, &plan, None)?;
                if let Some(workflow) = &job.workflow {
                    crate::session::working_pr::name(tx, &plan, &crate::ids::WorkflowKey::parse(workflow)?)?;
                }
                Ok(("cancelled".to_owned(), false, false, failed))
            }
            Decision::Request(job, attempt) => {
                tx.execute(
                    "UPDATE jobs SET cancel_reason = ?3, cancel_requested_at = (SELECT at FROM txns WHERE id = cpd_txn())
                      WHERE plan = ?1 AND key = ?2",
                    [plan.as_str(), job.key.as_str(), reason.as_str()],
                )?;
                let text = format!(
                    "Cancel requested for Job {key} of Plan {plan}: {reason} The engine stops Attempt {attempt}. Next: cpd job show {key} --plan {plan}"
                );
                append(
                    tx,
                    NewEvent::job_cancel_requested(text, &reason)
                        .plan(plan.clone())
                        .job(key.clone())
                        .attempt(attempt),
                )?;
                if let Some(workflow) = &job.workflow {
                    crate::session::working_pr::name(tx, &plan, &crate::ids::WorkflowKey::parse(workflow)?)?;
                }
                Ok(("running".to_owned(), true, false, Failed::default()))
            }
        })?,
    };
    let mut out = json!({"job": key, "status": status, "cancel_requested": requested});
    if already {
        out["already"] = json!(true);
    }
    if !failed.failed.is_empty() {
        out["failed"] = json!(failed.failed);
    }
    if !failed.cancelled.is_empty() {
        out["cancelled"] = json!(failed.cancelled);
    }
    Ok(Output::success(out))
}

/// In the caller's transaction, fail or cancel every Job of `plan` that needs `cancelled`, directly or
/// transitively through any status (contract §3.2 "Cancel", hs-d647). A `queued` dependent becomes `failed`
/// (`failed_cause` `its need <cancelled> was cancelled`, one `job-failed` Event); a `draft` one becomes
/// `cancelled` (`cancel_reason` the same string, one `job-cancelled` Event). A dependent already `failed`,
/// `merged` or `cancelled` is left as it is and still traversed through. `reason` is the cancelling Job's
/// own reason, for the Event texts. Idempotent: a rerun over dependents already failed or cancelled writes
/// nothing.
pub(crate) fn fail_dependents(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    cancelled: &JobKey,
    reason: &str,
) -> Result<Failed, CpdError> {
    let all = jobs::load(tx, plan)?;
    let cause = format!("its need {cancelled} was cancelled");
    // Reverse reachability: a Job is reached when one of its `needs` is `cancelled` or is already reached.
    let mut reached: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    loop {
        let mut grew = false;
        for job in &all {
            if reached.contains(&job.key) {
                continue;
            }
            if job
                .needs
                .iter()
                .any(|need| need == cancelled.as_str() || reached.contains(need))
            {
                reached.insert(job.key.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut changed = Failed::default();
    for job in &all {
        if !reached.contains(&job.key) {
            continue;
        }
        let key = JobKey::parse(&job.key)
            .map_err(|_| CpdError::bad_row(format!("Job key {:?}", job.key)))?;
        match job.status.as_str() {
            "queued" => {
                tx.execute(
                    "UPDATE jobs SET status = 'failed', failed_cause = ?3 WHERE plan = ?1 AND key = ?2",
                    [plan.as_str(), job.key.as_str(), cause.as_str()],
                )?;
                let text = format!(
                    "Job {} failed ({cause}): {reason} Decide: cpd job retry {} --tier <T> --plan {plan}, or cpd job cancel {} --reason <text> --plan {plan}",
                    job.key, job.key, job.key
                );
                append(
                    tx,
                    NewEvent::job_failed(text, &cause, &cause, job.require_tier()?, 0)
                        .plan(plan.clone())
                        .job(key),
                )?;
                changed.failed.push(job.key.clone());
            }
            "draft" => {
                tx.execute(
                    "UPDATE jobs SET status = 'cancelled', cancel_reason = ?3,
                            cancel_requested_at = (SELECT at FROM txns WHERE id = cpd_txn())
                      WHERE plan = ?1 AND key = ?2",
                    [plan.as_str(), job.key.as_str(), cause.as_str()],
                )?;
                let text = format!(
                    "Job {} of Plan {plan} was cancelled: {cause} Next: cpd plan show --plan {plan}",
                    job.key
                );
                append(
                    tx,
                    NewEvent::job_cancelled(text, &cause)
                        .plan(plan.clone())
                        .job(key),
                )?;
                changed.cancelled.push(job.key.clone());
            }
            _ => {}
        }
    }
    Ok(changed)
}

fn decide(
    conn: &Connection,
    plan: &PlanId,
    key: &JobKey,
    shape: Shape,
) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?.clone();
    match job.status.as_str() {
        "merged" => Err(CpdError::new(
            ErrorCode::JobMerged,
            format!("Job {key} is merged: a merged Job cannot be cancelled."),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())),
        "cancelled" => Ok(Decision::Already(job)),
        "running" if job.cancel_requested_at.is_some() => Ok(Decision::Already(job)),
        "running" => {
            if matches!(shape, Shape::Keep) {
                keeps_the_shape(conn, plan, &all, &job)?;
            }
            let active: Option<String> = conn
                .query_row(
                    "SELECT id FROM attempts WHERE plan = ?1 AND job = ?2 AND status <> 'ended'",
                    [plan.as_str(), key.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let attempt = active
                .and_then(|id| AttemptId::parse(&id).ok())
                .ok_or_else(|| {
                    CpdError::bad_row(format!(
                        "Job {plan}/{key} is running with no active Attempt"
                    ))
                })?;
            Ok(Decision::Request(job, attempt))
        }
        _ => {
            if matches!(shape, Shape::Keep) {
                keeps_the_shape(conn, plan, &all, &job)?;
            }
            Ok(Decision::Cancel(job))
        }
    }
}

/// The last `test` or the last `impl` Job of a `tdd` Workflow that has not opened is not cancelled: add the replacement first
/// (`shape_incomplete`, §5.3, hs-d497).
fn keeps_the_shape(
    conn: &Connection,
    plan: &PlanId,
    all: &[Job],
    job: &Job,
) -> Result<(), CpdError> {
    let workflows = crate::plan::fingerprint::load_workflows(conn, plan)?;
    let Some(workflow) = workflows
        .iter()
        .find(|p| p.key.as_str() == job.workflow_str())
    else {
        return Ok(());
    };
    let lacks = crate::plan::shape::missing(workflow, all, Some(job.key.as_str()));
    // Only what this cancel would take away: a Workflow that already lacked a kind is not this Job's doing.
    let kept = crate::plan::shape::missing(workflow, all, None);
    let taken: Vec<&str> = lacks.into_iter().filter(|k| !kept.contains(k)).collect();
    if taken.is_empty() {
        return Ok(());
    }
    Err(crate::plan::shape::incomplete(
        plan,
        workflow,
        &taken,
        &format!("cancelling {} would leave it with none", job.key),
    ))
}
