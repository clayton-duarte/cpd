//! `job cancel` (commands.md §5.3, contract §3.2 "Cancel").

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{CancelArgs, Env, bad_row, find, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};

const COMMAND: &str = "job cancel";

enum Decision {
    /// Already `cancelled`, or a request is already recorded.
    Already(Job),
    /// `draft`, `queued` or `failed`: `status = cancelled`.
    Cancel(Job),
    /// `running`: record the request against the active Attempt.
    Request(Job, AttemptId),
}

pub(super) fn cancel(args: CancelArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let reason = args.reason.as_str().to_owned();
    let key = args.key;
    let mut env = Env::open()?;

    let decision = env.store.read(|conn| decide(conn, &plan, &key))?;
    let (status, requested, already) = match decision {
        Decision::Already(job) => (job.status.clone(), job.status == "running", true),
        _ => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key)? {
            Decision::Already(job) => Ok((job.status.clone(), job.status == "running", true)),
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
                Ok(("cancelled".to_owned(), false, false))
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
                Ok(("running".to_owned(), true, false))
            }
        })?,
    };
    let mut out = json!({"job": key, "status": status, "cancel_requested": requested});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

fn decide(conn: &Connection, plan: &PlanId, key: &JobKey) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?.clone();
    match job.status.as_str() {
        "merged" => Err(CpdError::new(ErrorCode::JobMerged, format!("Job {key} is merged: a merged Job cannot be cancelled."))
            .with("plan", plan.as_str())
            .with("job", key.as_str())),
        "cancelled" => Ok(Decision::Already(job)),
        "running" if job.cancel_requested_at.is_some() => Ok(Decision::Already(job)),
        "running" => {
            let active: Option<String> = conn
                .query_row("SELECT id FROM attempts WHERE plan = ?1 AND job = ?2 AND status <> 'ended'", [plan.as_str(), key.as_str()], |r| r.get(0))
                .optional()?;
            let attempt = active
                .and_then(|id| AttemptId::parse(&id).ok())
                .ok_or_else(|| bad_row(format!("Job {plan}/{key} is running with no active Attempt")))?;
            Ok(Decision::Request(job, attempt))
        }
        _ => Ok(Decision::Cancel(job)),
    }
}
