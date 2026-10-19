//! `job retry` (commands.md §5.4): `failed → queued` at a Tier the Lead chooses.

use rusqlite::Connection;
use serde_json::json;

use super::{Env, RetryArgs, find, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;

const COMMAND: &str = "job retry";

enum Decision {
    /// Already `queued` with this `retry_tier` and no Attempt since.
    Already,
    Queue(Job),
}

pub(super) fn retry(args: RetryArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let tier = args.tier.to_string();
    let mut env = Env::open()?;
    env.check_tier(args.tier)?;

    let already = match env.store.read(|conn| decide(conn, &plan, &key, &tier))? {
        Decision::Already => true,
        Decision::Queue(_) => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &tier)? {
            Decision::Already => Ok(true),
            Decision::Queue(job) => {
                tx.execute(
                    "UPDATE jobs SET status = 'queued', retry_tier = ?3, failed_cause = NULL WHERE plan = ?1 AND key = ?2",
                    [plan.as_str(), job.key.as_str(), tier.as_str()],
                )?;
                let text = format!("Job {key} of Plan {plan} is queued again at Tier {tier}. Next: cpd job show {key} --plan {plan}");
                append(tx, NewEvent::job_retried(text, &tier).plan(plan.clone()).job(key.clone()))?;
                Ok(false)
            }
        })?,
    };
    let mut out = json!({"job": key, "status": "queued", "tier": tier});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

fn decide(conn: &Connection, plan: &PlanId, key: &JobKey, tier: &str) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?;
    if let Some(stored) = &job.fingerprint {
        let found = locked_fingerprint(job);
        if *stored != found {
            return Err(CpdError::new(ErrorCode::LockedJobTampered, format!("Job {key} does not match its Locked-Job fingerprint."))
                .with("plan", plan.as_str())
                .with("job", key.as_str())
                .with("expected", stored.as_str())
                .with("found", found));
        }
    }
    if job.status == "queued" && job.retry_tier.as_deref() == Some(tier) && retried_since_last_start(conn, plan, key)? {
        return Ok(Decision::Already);
    }
    if job.status != "failed" {
        return Err(CpdError::new(
            ErrorCode::JobNotFailed,
            format!("Job {key} is {}: only a failed Job is retried.", job.status),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", job.status.as_str()));
    }
    Ok(Decision::Queue(job.clone()))
}

/// The Job's latest `job-retried` Event is newer than its latest
/// `attempt-started`: no Attempt has begun since the retry.
pub(crate) fn retried_since_last_start(conn: &Connection, plan: &PlanId, key: &JobKey) -> Result<bool, CpdError> {
    let latest = |kind: &str| -> Result<Option<i64>, CpdError> {
        Ok(conn.query_row(
            "SELECT max(id) FROM events WHERE plan = ?1 AND job = ?2 AND kind = ?3",
            [plan.as_str(), key.as_str(), kind],
            |r| r.get(0),
        )?)
    };
    Ok(match (latest("job-retried")?, latest("attempt-started")?) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(retried), Some(started)) => started < retried,
    })
}
