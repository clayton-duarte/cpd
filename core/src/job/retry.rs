//! `job retry` (commands.md §5.4): `failed → queued` at a Tier the Lead chooses.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{Env, RetryArgs, check_needs_not_cancelled, find, plan_phase};
use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;
use crate::store::WriteTxn;

const COMMAND: &str = "job retry";

// The variant carries the one value this enum exists to hand back; boxing is a type change out of scope.
#[allow(clippy::large_enum_variant)]
enum Decision {
    /// Already `queued` with this `retry_tier` and no Attempt since.
    Already,
    Queue(Job),
}

pub(super) fn retry(args: RetryArgs) -> Result<Output, CpdError> {
    retry_in(&mut Env::open()?, args)
}

/// [`retry`] on an open `env`: what a test hands in.
pub(super) fn retry_in(env: &mut Env, args: RetryArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let _ = env
        .store
        .read(|conn| crate::plan::shaping::guard(conn, &plan))?;
    let key = args.key;
    let tier = args.tier.to_string();
    let row: Option<(String, String, Option<String>, Option<String>)> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT kind, status, retry_tier, tier FROM jobs WHERE plan = ?1 AND key = ?2",
                [plan.as_str(), key.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?)
    })?;
    let kind = row.as_ref().map(|r| r.0.clone());
    // pro-d484: a queued Job whose effective Tier fails the worker range (raised floor) is retried at a new Tier.
    let stuck = match &row {
        Some((kind, status, retry_tier, tier)) if status == "queued" => {
            match retry_tier.as_deref().or(tier.as_deref()).map(Tier::parse) {
                Some(Ok(effective)) => matches!(
                    env.check_tier(&plan, &key, None, kind, effective),
                    Err(e) if e.code() == ErrorCode::TierNotAllowed
                ),
                _ => false,
            }
        }
        _ => false,
    };
    env.check_tier(
        &plan,
        &key,
        None,
        kind.as_deref().unwrap_or("work"),
        args.tier,
    )?;

    let already = match env.store.read(|conn| decide(conn, &plan, &key, &tier, stuck))? {
        Decision::Already => true,
        Decision::Queue(_) => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &tier, stuck)? {
            Decision::Already => Ok(true),
            Decision::Queue(job) => {
                let text = format!("Job {key} of Plan {plan} is queued again at Tier {tier}. Next: cpd job show {key} --plan {plan}");
                requeue(tx, &plan, &key, &tier, text)?;
                if let Some(workflow) = &job.workflow {
                    crate::session::working_pr::name(tx, &plan, &crate::ids::WorkflowKey::parse(workflow)?)?;
                }
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

/// Requeues failed Job `key` at `tier` and appends its `job-retried` Event with `text`. Shared by `job retry` and the engine's retry after a fix.
pub(crate) fn requeue(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    key: &JobKey,
    tier: &str,
    text: String,
) -> Result<(), CpdError> {
    tx.execute(
        "UPDATE jobs SET status = 'queued', retry_tier = ?3, failed_cause = NULL WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), key.as_str(), tier],
    )?;
    append(
        tx,
        NewEvent::job_retried(text, tier)
            .plan(plan.clone())
            .job(key.clone()),
    )?;
    Ok(())
}

fn decide(
    conn: &Connection,
    plan: &PlanId,
    key: &JobKey,
    tier: &str,
    stuck: bool,
) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    crate::job::replace_test::check_fence(conn, plan, key.as_str())?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?;
    if let Some(stored) = &job.fingerprint {
        let found = locked_fingerprint(job);
        if *stored != found {
            return Err(CpdError::new(
                ErrorCode::LockedJobTampered,
                format!("Job {key} does not match its Locked-Job fingerprint."),
            )
            .with("plan", plan.as_str())
            .with("job", key.as_str())
            .with("expected", stored.as_str())
            .with("found", found));
        }
    }
    if job.status == "queued"
        && job.retry_tier.as_deref() == Some(tier)
        && retried_since_last_start(conn, plan, key)?
    {
        return Ok(Decision::Already);
    }
    if job.status != "failed" && !(stuck && job.status == "queued") {
        return Err(CpdError::new(
            ErrorCode::JobNotFailed,
            format!("Job {key} is {}: only a failed Job is retried.", job.status),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", job.status.as_str()));
    }
    // A cancelled need can never be retried back, so retrying a Job held by one is refused (§5.4, hs-d647).
    check_needs_not_cancelled(&all, plan, &job.needs)?;
    Ok(Decision::Queue(job.clone()))
}

/// The Jobs of `plan` with a retry pending, by key: the Job's latest `job-retried` Event is newer than its latest
/// `attempt-started`, so no Attempt has begun since the retry. The one source of that rule.
pub(crate) fn retries_pending(
    conn: &Connection,
    plan: &PlanId,
) -> Result<BTreeSet<String>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT job FROM events WHERE plan = ?1 AND job IS NOT NULL AND kind IN ('job-retried', 'attempt-started')
          GROUP BY job HAVING max(CASE WHEN kind = 'job-retried' THEN id END) > coalesce(max(CASE WHEN kind = 'attempt-started' THEN id END), 0)",
    )?;
    let keys = stmt
        .query_map([plan.as_str()], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(keys)
}

/// No Attempt has begun since the Job's latest retry: see [`retries_pending`].
pub(crate) fn retried_since_last_start(
    conn: &Connection,
    plan: &PlanId,
    key: &JobKey,
) -> Result<bool, CpdError> {
    Ok(retries_pending(conn, plan)?.contains(key.as_str()))
}
