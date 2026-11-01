//! `job urgent` (commands.md §5.8, contract §2.2): sets `jobs.urgent` on a
//! `queued` or `draft` Job. The flag is not a Locked field and in no
//! fingerprint, so a Locked Job may take it; the Event is `job-edited` with
//! `fields: ["urgent"]`.

use rusqlite::Connection;
use serde_json::json;

use super::{Env, UrgentArgs, find, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};

const COMMAND: &str = "job urgent";

enum Decision {
    /// Already urgent: nothing to write.
    Already(Job),
    Mark(Job),
}

pub(super) fn urgent(args: UrgentArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;

    let decision = env.store.read(|conn| decide(conn, &plan, &key))?;
    let (job, already) = match decision {
        Decision::Already(job) => (job, true),
        Decision::Mark(_) => env
            .store
            .write(COMMAND, |tx| match decide(tx, &plan, &key)? {
                Decision::Already(job) => Ok((job, true)),
                Decision::Mark(job) => {
                    tx.execute(
                        "UPDATE jobs SET urgent = 1 WHERE plan = ?1 AND key = ?2",
                        [plan.as_str(), key.as_str()],
                    )?;
                    let text = format!(
                        "Job {key} of Plan {plan} is urgent. Next: cpd job show {key} --plan {plan}"
                    );
                    append(
                        tx,
                        NewEvent::job_edited(text, ["urgent"])
                            .plan(plan.clone())
                            .job(key.clone()),
                    )?;
                    if let Some(workflow) = &job.workflow {
                        crate::session::working_pr::name(
                            tx,
                            &plan,
                            &crate::ids::WorkflowKey::parse(workflow)?,
                        )?;
                    }
                    Ok((job, false))
                }
            })?,
    };
    let mut out = json!({"job": {"key": job.key, "status": job.status, "urgent": true}, "fields": if already { json!([]) } else { json!(["urgent"]) }});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// Only a `queued` or `draft` Job takes the flag: E3 starts nothing else.
fn decide(conn: &Connection, plan: &PlanId, key: &JobKey) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?.clone();
    if !matches!(job.status.as_str(), "queued" | "draft") {
        return Err(CpdError::new(
            ErrorCode::IllegalTransition,
            format!("Job {key} is {}: only a queued or draft Job can be marked urgent (a failed one: cpd job retry first).", job.status),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", job.status.as_str()));
    }
    Ok(if job.urgent {
        Decision::Already(job)
    } else {
        Decision::Mark(job)
    })
}
