//! `job approve-touches` (commands.md §5.9a, contract §2.2 "Job kinds", §7.5; hs-n641): the Lead lets a Job change a
//! path outside its Spec `touches`. Sets `jobs.touches_approved` to the union of its old value and the paths; not a Locked
//! field, in no fingerprint. The Event is `job-edited` with `fields: ["touches_approved"]`.

use rusqlite::Connection;
use serde_json::json;

use super::{ApproveTouchesArgs, Env, find, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};

const COMMAND: &str = "job approve-touches";

enum Decision {
    /// Every path is approved already: nothing to write.
    Already(Job, Vec<String>),
    /// The Job's `touches_approved` becomes this.
    Approve(Job, Vec<String>),
}

pub(super) fn approve(args: ApproveTouchesArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    approve_in(&mut env, args)
}

/// [`approve`] on an open `env`: what a test hands in.
pub(super) fn approve_in(env: &mut Env, args: ApproveTouchesArgs) -> Result<Output, CpdError> {
    // The Lead only, through `CPD_ATTEMPT` (hs-d497): cpd sets it in every Worker's pane. Nothing else authenticates the caller.
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead approves a path outside a Job's touches: ask it with cpd ask, and it runs cpd job approve-touches.",
        ));
    }
    let plan = args.plan.require()?.clone();
    let (key, reason) = (args.key, args.reason);
    let mut paths: Vec<String> = Vec::new();
    for path in args.paths {
        if path.is_empty() || path.starts_with('/') {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("--path {path:?} must be a non-empty path relative to the repo."),
            )
            .with("path", path));
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &paths))?;
    let (job, approved, already) = match decision {
        Decision::Already(job, approved) => (job, approved, true),
        Decision::Approve(..) => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &paths)? {
            Decision::Already(job, approved) => Ok((job, approved, true)),
            Decision::Approve(job, approved) => {
                let stored = serde_json::to_string(&approved).expect("a list of text serialises");
                tx.execute("UPDATE jobs SET touches_approved = ?3 WHERE plan = ?1 AND key = ?2", rusqlite::params![plan.as_str(), key.as_str(), stored])?;
                let text = format!(
                    "Job {key} of Plan {plan} may change the paths outside its touches {}: {}. Next: cpd job show {key} --plan {plan}",
                    paths.join(", "),
                    reason
                );
                append(tx, NewEvent::job_edited(text, ["touches_approved"]).plan(plan.clone()).job(key.clone()))?;
                if let Some(pr) = &job.pr {
                    crate::session::working_pr::name(tx, &plan, &crate::ids::PrKey::parse(pr)?)?;
                }
                Ok((job, approved, false))
            }
        })?,
    };
    let mut out = json!({
        "job": {"key": job.key, "status": job.status, "kind": job.kind},
        "approved": approved,
        "fields": if already { json!([]) } else { json!(["touches_approved"]) },
    });
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// The preconditions of §5.9a, in order; returns what `touches_approved` is, or is to become.
fn decide(
    conn: &Connection,
    plan: &PlanId,
    key: &JobKey,
    paths: &[String],
) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?.clone();
    if !matches!(job.status.as_str(), "queued" | "running" | "failed") {
        return Err(CpdError::new(
            ErrorCode::IllegalTransition,
            format!("Job {key} is {}: only a queued, running or failed Job is approved (a draft is not checked yet, an ended one never will be).", job.status),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", job.status.as_str()));
    }
    let mut approved = crate::job::guard::approved_touches(&job);
    let before = approved.len();
    for path in paths {
        if !approved.contains(path) {
            approved.push(path.clone());
        }
    }
    Ok(if approved.len() == before {
        Decision::Already(job, approved)
    } else {
        Decision::Approve(job, approved)
    })
}
