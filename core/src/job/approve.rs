//! `job approve-tests` (commands.md §5.9, contract §2.2 "Job kinds", §7.7): the Lead lets an `impl` Job change red tests.
//! Sets `jobs.test_approved` to the union of its old value and the paths; not a Locked field, in no fingerprint. The
//! Event is `job-edited` with `fields: ["test_approved"]`.

use rusqlite::Connection;
use serde_json::json;

use super::{ApproveArgs, Env, find, guard, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::tamper::path_matches;
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};

const COMMAND: &str = "job approve-tests";

enum Decision {
    /// Every path is approved already: nothing to write.
    Already(Job, Vec<String>),
    /// The Job's `test_approved` becomes this.
    Approve(Job, Vec<String>),
}

pub(super) fn approve(args: ApproveArgs) -> Result<Output, CpdError> {
    // The Lead only, through `CPD_ATTEMPT` (hs-d497): cpd sets it in every Worker's pane. Nothing else authenticates the caller.
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead approves a change to red tests: ask it with cpd ask, and it runs cpd job approve-tests.",
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
    let mut env = Env::open()?;

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &paths))?;
    let (job, approved, already) = match decision {
        Decision::Already(job, approved) => (job, approved, true),
        Decision::Approve(..) => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &paths)? {
            Decision::Already(job, approved) => Ok((job, approved, true)),
            Decision::Approve(job, approved) => {
                let stored = serde_json::to_string(&approved).expect("a list of text serialises");
                tx.execute("UPDATE jobs SET test_approved = ?3 WHERE plan = ?1 AND key = ?2", rusqlite::params![plan.as_str(), key.as_str(), stored])?;
                let text = format!(
                    "Job {key} of Plan {plan} may change the red tests {}: {}. Next: cpd job show {key} --plan {plan}",
                    paths.join(", "),
                    reason
                );
                append(tx, NewEvent::job_edited(text, ["test_approved"]).plan(plan.clone()).job(key.clone()))?;
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
        "fields": if already { json!([]) } else { json!(["test_approved"]) },
    });
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// The preconditions of §5.9, in order; returns what `test_approved` is, or is to become.
fn decide(
    conn: &Connection,
    plan: &PlanId,
    key: &JobKey,
    paths: &[String],
) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?.clone();
    if job.kind != "impl" {
        return Err(CpdError::new(
            ErrorCode::NotAnImplJob,
            format!("Job {key} is a {} Job: only an impl Job is approved to change red tests (a test Job guards its own files).", job.kind),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("kind", job.kind.as_str()));
    }
    if !matches!(job.status.as_str(), "queued" | "running" | "failed") {
        return Err(CpdError::new(
            ErrorCode::IllegalTransition,
            format!("Job {key} is {}: only a queued, running or failed Job is approved (a draft is not checked yet, an ended one never will be).", job.status),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", job.status.as_str()));
    }
    let guard = guard::of(&all, &job);
    let unguarded: Vec<&String> = paths
        .iter()
        .filter(|p| !guard.files.iter().any(|entry| path_matches(entry, p)))
        .collect();
    if !unguarded.is_empty() {
        return Err(CpdError::new(
            ErrorCode::PathNotGuarded,
            format!("No test_files entry of the test Jobs {key} needs guards: {}. Only a guarded path is approved.", unguarded.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("paths", json!(unguarded))
        .with("jobs", json!(guard.jobs)));
    }
    let mut approved = guard.approved.clone();
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
