//! `job approve-tests` (commands.md §5.9, contract §2.2 "Job kinds", §7.7): the Lead lets an `impl` Job change red tests.
//! Sets `jobs.test_approved` to the union of its old value and the paths; not a Locked field, in no fingerprint. The
//! Event is `job-edited` with `fields: ["test_approved"]`.

use rusqlite::Connection;
use serde_json::json;

use super::{
    ApproveArgs, ApproveTestRemovalArgs, ApproveTouchesArgs, Env, find, guard, plan_phase,
};
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
                if let Some(workflow) = &job.workflow {
                    crate::session::working_pr::name(tx, &plan, &crate::ids::WorkflowKey::parse(workflow)?)?;
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
    if let Some(workflow) = &job.workflow {
        let refactor: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM workflows WHERE plan = ?1 AND key = ?2 AND shape = 'refactor')",
            [plan.as_str(), workflow.as_str()],
            |row| row.get(0),
        )?;
        if refactor {
            return Err(CpdError::new(
                ErrorCode::TestChangeForbidden,
                "Tests cannot be approved for a refactor; amend the coverage workflow instead.",
            ));
        }
    }
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

/// `job approve-test-removal` (commands.md §5.9c, contract §7.5; b70): the Lead lets a Job's submit remove or weaken the
/// named test cases. Sets `jobs.test_removal_approved` to the union of its old value and the names; not a Locked field,
/// in no fingerprint. The Event is `job-edited` with `fields: ["test_removal_approved"]`, its text carrying the reason.
pub(super) fn approve_test_removal(args: ApproveTestRemovalArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    approve_test_removal_in(&mut env, args)
}

/// [`approve_test_removal`] on an open `env`: what a test hands in.
pub(super) fn approve_test_removal_in(
    env: &mut Env,
    args: ApproveTestRemovalArgs,
) -> Result<Output, CpdError> {
    const COMMAND: &str = "job approve-test-removal";
    // The Lead only, through `CPD_ATTEMPT` (hs-d497), as `job approve-tests`.
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead approves the removal of a test: ask it with cpd ask, and it runs cpd job approve-test-removal.",
        ));
    }
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut names: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for name in args.tests {
        if name.trim().is_empty() {
            return Err(CpdError::new(
                ErrorCode::Usage,
                "--test needs a non-empty test name.",
            ));
        }
        lines.push(crate::git::tamper::approve_test_removal(
            &mut names,
            name.trim(),
            args.reason.as_str(),
        )?);
    }

    let decide = |conn: &Connection| -> Result<(Job, Vec<String>, bool), CpdError> {
        plan_phase(conn, &plan)?;
        let all = jobs::load(conn, &plan)?;
        let job = find(&all, &plan, &key)?.clone();
        if !matches!(job.status.as_str(), "queued" | "running" | "failed") {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!("Job {key} is {}: only a queued, running or failed Job is approved (a draft is not checked yet, an ended one never will be).", job.status),
            )
            .with("plan", plan.as_str())
            .with("job", key.as_str())
            .with("status", job.status.as_str()));
        }
        let mut approved = approved_test_removals(conn, &plan, &key)?;
        let before = approved.len();
        for name in &names {
            if !approved.contains(name) {
                approved.push(name.clone());
            }
        }
        let new = approved.len() != before;
        Ok((job, approved, new))
    };
    let (job, approved, new) = env.store.read(|conn| decide(conn))?;
    if new {
        env.store.write(COMMAND, |tx| {
            let (job, approved, new) = decide(tx)?;
            if !new {
                return Ok(());
            }
            let stored = serde_json::to_string(&approved).expect("a list of text serialises");
            tx.execute(
                "UPDATE jobs SET test_removal_approved = ?3 WHERE plan = ?1 AND key = ?2",
                rusqlite::params![plan.as_str(), key.as_str(), stored],
            )?;
            let text = format!(
                "Job {key} of Plan {plan} may remove or weaken tests. {} Next: cpd job show {key} --plan {plan}",
                lines.join(" ")
            );
            append(
                tx,
                NewEvent::job_edited(text, ["test_removal_approved"])
                    .plan(plan.clone())
                    .job(key.clone()),
            )?;
            if let Some(workflow) = &job.workflow {
                crate::session::working_pr::name(tx, &plan, &crate::ids::WorkflowKey::parse(workflow)?)?;
            }
            Ok(())
        })?;
    }
    let mut out = json!({
        "job": {"key": job.key, "status": job.status, "kind": job.kind},
        "approved": approved,
        "fields": if new { json!(["test_removal_approved"]) } else { json!([]) },
    });
    if !new {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// The test names the Lead approved Job `key` to remove or weaken (`jobs.test_removal_approved`), read as stored now so
/// an approval reaches a running Attempt; empty when it approved none.
pub fn approved_test_removals(
    conn: &Connection,
    plan: &PlanId,
    key: &JobKey,
) -> Result<Vec<String>, CpdError> {
    let stored: Option<String> = conn.query_row(
        "SELECT test_removal_approved FROM jobs WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), key.as_str()],
        |r| r.get(0),
    )?;
    Ok(stored.map_or_else(Vec::new, |text| {
        serde_json::from_str(&text).expect("the column's CHECK keeps a JSON array of text")
    }))
}

/// The Lead waives the declared `touches` path `path` of Job `job` as unchanged, with a required `reason`
/// (`cpd job approve-untouched`). The waiver is stored in the Job's own `touches_waived`, which only the unchanged check reads
/// (never the outside check), and the `job-edited` Event carries the reason, visible to the User.
pub(crate) fn waive_untouched(
    env: &mut Env,
    plan: &PlanId,
    job: &str,
    path: &str,
    reason: &str,
) -> Result<(), CpdError> {
    if reason.trim().is_empty() {
        return Err(CpdError::new(
            ErrorCode::Usage,
            "A waiver of an unchanged touch needs a --reason.",
        ));
    }
    env.store.write("job approve-untouched", |tx| {
        let key = crate::ids::JobKey::parse(job)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE plan = ?1 AND key = ?2)",
            rusqlite::params![plan.as_str(), job],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(CpdError::new(
                ErrorCode::JobNotFound,
                format!("No Job {job}."),
            ));
        }
        let mut approved = waived_untouched(tx, plan, job)?;
        if approved.iter().any(|p| p == path) {
            return Ok(());
        }
        approved.push(path.to_owned());
        let stored = serde_json::to_string(&approved).expect("a list of text serialises");
        tx.execute(
            "UPDATE jobs SET touches_waived = ?3 WHERE plan = ?1 AND key = ?2",
            rusqlite::params![plan.as_str(), job, stored],
        )?;
        let text = format!(
            "Job {job} of Plan {plan} may submit with the declared touch {path} unchanged: {}.",
            reason.trim()
        );
        crate::events::append(
            tx,
            crate::events::NewEvent::job_edited(text, ["touches_waived"])
                .plan(plan.clone())
                .job(key),
        )?;
        Ok(())
    })
}

/// `cpd job approve-untouched <job> --path <p>… --reason <text>` (commands.md §5.9b): the Lead only; waives each path.
pub(super) fn approve_untouched(args: ApproveTouchesArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    approve_untouched_in(&mut env, args)
}

/// [`approve_untouched`] on an open `env`: what a test hands in.
pub(crate) fn approve_untouched_in(
    env: &mut Env,
    args: ApproveTouchesArgs,
) -> Result<Output, CpdError> {
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead waives an unchanged touch: ask it with cpd ask, and it runs cpd job approve-untouched.",
        ));
    }
    let plan = args.plan.require()?.clone();
    for path in &args.paths {
        waive_untouched(env, &plan, args.key.as_str(), path, args.reason.as_str())?;
    }
    Ok(Output::success(json!({
        "job": {"key": args.key.as_str()},
        "approved": args.paths,
        "fields": ["touches_waived"],
    })))
}

/// The declared `touches` paths the Lead waived for Job `job` as unchanged (`jobs.touches_waived`); empty when none.
pub(crate) fn waived_untouched(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    job: &str,
) -> Result<Vec<String>, CpdError> {
    let stored: Option<String> = conn.query_row(
        "SELECT touches_waived FROM jobs WHERE plan = ?1 AND key = ?2",
        rusqlite::params![plan.as_str(), job],
        |r| r.get(0),
    )?;
    Ok(stored
        .map(|s| serde_json::from_str(&s).expect("touches_waived is a JSON array of text"))
        .unwrap_or_default())
}
