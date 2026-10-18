//! `job promote` (commands.md §5.8): `draft → queued` of a Job of a Plan past
//! the lock, with its Locked-Job fingerprint stored (contract §2.3).

use rusqlite::Connection;
use serde_json::json;

use super::{Env, PromoteArgs, check_pr_live, find, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;

const COMMAND: &str = "job promote";

pub(super) fn promote(args: PromoteArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;

    env.store.read(|conn| decide(conn, &plan, &key))?;
    let job = env.store.write(COMMAND, |tx| {
        let job = decide(tx, &plan, &key)?;
        let changed = tx.execute(
            "UPDATE jobs SET status = 'queued', fingerprint = ?3 WHERE plan = ?1 AND key = ?2 AND status = 'draft'",
            [plan.as_str(), key.as_str(), locked_fingerprint(&job).as_str()],
        )?;
        if changed != 1 {
            return Err(not_a_draft(&plan, &key, "changed"));
        }
        let text = format!("Job {key} of Plan {plan} was promoted: it is queued. Next: cpd job show {key} --plan {plan}");
        append(tx, NewEvent::new(Kind::JobEdited, text, json!({"fields": ["status"]})).plan(plan.clone()).job(key.clone()))?;
        Ok(job)
    })?;
    Ok(Output::success(json!({"job": {"key": job.key, "pr": job.pr, "status": "queued", "appended": job.appended}})))
}

/// The draft to promote, after §5.8's preconditions in order.
fn decide(conn: &Connection, plan: &PlanId, key: &JobKey) -> Result<Job, CpdError> {
    let phase = plan_phase(conn, plan)?;
    if !matches!(phase.as_str(), "dispatch" | "pr") {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {plan} is in phase {phase}: a draft Job is promoted in phase dispatch or pr (in phase plan, plan lock promotes it)."),
        )
        .with("plan", plan.as_str())
        .with("phase", phase));
    }
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?;
    if !job.is_draft() {
        return Err(not_a_draft(plan, key, &job.status));
    }
    check_pr_live(&load_prs(conn, plan)?, plan, &job.pr)?;
    let unavailable: Vec<&str> =
        all.iter().filter(|j| job.needs.contains(&j.key) && matches!(j.status.as_str(), "cancelled" | "failed")).map(|j| j.key.as_str()).collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(
            ErrorCode::NeedsUnavailable,
            format!("Job {key} needs {}, cancelled or failed: edit its needs first.", unavailable.join(", ")),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("jobs", unavailable));
    }
    Ok(job.clone())
}

fn not_a_draft(plan: &PlanId, key: &JobKey, status: &str) -> CpdError {
    CpdError::new(ErrorCode::JobLocked, format!("Job {key} is {status}: only a draft Job is promoted."))
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", status)
}
