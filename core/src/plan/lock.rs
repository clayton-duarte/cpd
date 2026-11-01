//! `plan lock` (commands.md §4.8, contract §3.1, §7.2): `plan → dispatch`.
//! Starts the PRs that need no other PR (all or nothing, `pr::start`), queues
//! every draft Job with its Locked-Job fingerprint, and starts the engine with
//! `cpd watch --ensure`.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

use super::fingerprint::{load_prs, plan_fingerprint};
use super::jobs::{self, Job};
use super::prs::PrRow;
use super::review::{check_drafts_complete, latest_approved_for, latest_json, no_draft_jobs};
use super::row::{self, Phase};
use super::{Env, LockArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::{TAIL_LINES, tail};
use crate::host::deadline::Failed;
use crate::host::runner::{NO_DEADLINE, Runner};
use crate::ids::PlanId;
use crate::output::Output;
use crate::ports::Ports;
use crate::pr::start::{self, By, NotReady, Pushed};
use crate::session::PROGRAM;

const COMMAND: &str = "plan lock";

pub(super) fn lock(args: LockArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (plan, jobs, prs, held) = env.store.read(|conn| {
        let prs = load_prs(conn, &id)?;
        let mut held = HashMap::new();
        for pr in prs.iter().filter(|p| p.status != "cancelled") {
            held.insert(pr.key.to_string(), start::not_ready(conn, &id, &pr.key)?);
        }
        Ok((row::load(conn, &id)?, jobs::load(conn, &id)?, prs, held))
    })?;
    match plan.phase {
        Phase::Plan => {}
        Phase::Dispatch => {
            let watch = ensure_watch(&env, &id)?;
            return Ok(Output::success(json!({
                "plan": id, "phase": "dispatch", "prs": listed(&prs, &[], &held), "watch": watch, "already": true,
            })));
        }
        other => {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!(
                    "Plan {id} is in phase {}: only a Plan in phase plan is locked.",
                    other.as_str()
                ),
            )
            .with("plan", id.as_str())
            .with("from", other.as_str())
            .with("to", "dispatch"));
        }
    }
    let drafts: Vec<&Job> = jobs.iter().filter(|j| j.is_draft()).collect();
    if drafts.is_empty() {
        return Err(no_draft_jobs(&id));
    }
    env.store
        .read(|conn| check_drafts_complete(conn, &id, &jobs))?;
    super::shape::check(&id, &prs, &jobs)?;
    let fingerprint = env.store.read(|conn| plan_fingerprint(conn, &id))?;
    let (review, latest) = env.store.read(|conn| {
        Ok((
            latest_approved_for(conn, &id, &fingerprint)?,
            latest_json(conn, &id)?,
        ))
    })?;
    let Some(review) = review else {
        return Err(CpdError::new(
            ErrorCode::PlanNotApproved,
            format!("The latest Plan review of {id} is not approved for the current draft Jobs: run cpd plan review."),
        )
        .with("plan", id.as_str())
        .with("fingerprint", fingerprint)
        .with("review", latest)
        .with("next", format!("cpd plan review --plan {id} --title <title>")));
    };
    let live: Vec<&PrRow> = prs.iter().filter(|p| p.status != "cancelled").collect();
    let empty: Vec<&str> = live
        .iter()
        .filter(|p| !drafts.iter().any(|j| j.pr_str() == p.key.as_str()))
        .map(|p| p.key.as_str())
        .collect();
    if live.is_empty() || !empty.is_empty() {
        let message = if live.is_empty() {
            format!("Plan {id} has no PR: add one with cpd pr add.")
        } else {
            format!(
                "Plan {id} has PRs without a draft Job: {}.",
                empty.join(", ")
            )
        };
        let mut error = CpdError::new(ErrorCode::PrEmpty, message)
            .with("plan", id.as_str())
            .with("prs", empty);
        if live.is_empty() {
            error = error.with(
                "next",
                format!("cpd pr add <key> --repo <path> --plan {id}"),
            );
        }
        return Err(error);
    }
    if let Some(cycle) = jobs::cycle(&jobs) {
        return Err(CpdError::new(
            ErrorCode::DependencyCycle,
            format!(
                "The draft Jobs of {id} depend on each other in a cycle: {}.",
                cycle.join(" → ")
            ),
        )
        .with("plan", id.as_str())
        .with("cycle", cycle));
    }

    // git, outside any transaction (§7.2): every PR with no needs is prepared first,
    // and only when all have passed are the new branches pushed. On failure nothing is left.
    let mut prepared = Vec::new();
    for pr in prs.iter().filter(|p| {
        p.status == "waiting" && held.get(p.key.as_str()).is_some_and(NotReady::is_empty)
    }) {
        match start::prepare(&env.home, &env.config, &mut env.store, &id, pr) {
            Ok(p) => prepared.push(p),
            Err(failed) => {
                prepared.iter().for_each(start::Prepared::discard);
                return Err(failed.into_error());
            }
        }
    }
    let started: Vec<Pushed> = prepared.into_iter().map(start::Prepared::push).collect();

    let keys: Vec<String> = drafts.iter().map(|j| j.key.clone()).collect();
    let committed = env.store.write(COMMAND, |tx| {
        // Compare-and-swap on what was read: the phase, the drafts and the review.
        let jobs_now = jobs::load(tx, &id)?;
        let review_now = latest_approved_for(tx, &id, &fingerprint)?;
        if plan_fingerprint(tx, &id)? != fingerprint || review_now.as_deref() != Some(review.as_str()) {
            return Err(cas_lost(&id));
        }
        let moved = tx.execute(
            "UPDATE plans SET phase = 'dispatch', locked_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE id = ?1 AND phase = 'plan'",
            [id.as_str()],
        )?;
        if moved == 0 {
            return Err(cas_lost(&id));
        }
        for job in jobs_now.iter().filter(|j| j.is_draft()) {
            tx.execute(
                "UPDATE jobs SET status = 'queued', fingerprint = ?1 WHERE plan = ?2 AND key = ?3 AND status = 'draft'",
                [jobs::locked_fingerprint(job).as_str(), id.as_str(), job.key.as_str()],
            )?;
        }
        let text = format!("Plan {id} is locked: {} Jobs queued for the engine. Next: cpd plan show --plan {id}", keys.len());
        let started_keys: Vec<&str> = started.iter().map(|p| p.key()).collect();
        append(
            tx,
            NewEvent::plan_phase_locked(text, &keys, started_keys).plan(id.clone()),
        )?;
        for pr in &started {
            start::record(tx, &id, pr, By::Lock)?;
        }
        Ok(())
    });
    if let Err(e) = committed {
        started.iter().for_each(Pushed::discard);
        return Err(e);
    }

    let watch = ensure_watch(&env, &id)?;
    Ok(Output::success(
        json!({"plan": id, "phase": "dispatch", "jobs": keys, "prs": listed(&prs, &started, &held), "watch": watch}),
    ))
}

/// `prs` of the result (commands.md §4.8): every PR that is not `cancelled`, in
/// key order. A PR started by this lock has its worktree and R35's outcome;
/// every other has null for those.
fn listed(prs: &[PrRow], started: &[Pushed], held: &HashMap<String, NotReady>) -> Vec<Value> {
    prs.iter()
        .filter(|p| p.status != "cancelled")
        .map(|pr| {
            let mut one = match started.iter().find(|s| s.key() == pr.key.as_str()) {
                Some(s) => {
                    let mut one = s.to_json();
                    one["needs"] = json!(pr.needs);
                    one
                }
                None => json!({
                    "key": pr.key, "status": pr.status, "needs": pr.needs, "lead_branch": pr.lead_branch,
                    "lead_worktree": pr.lead_worktree, "base_sha": null, "reused": null, "pushed": null,
                }),
            };
            one["base_pr"] = json!(held.get(pr.key.as_str()).and_then(|h| h.base_pr.clone()));
            one
        })
        .collect()
}

fn cas_lost(id: &PlanId) -> CpdError {
    CpdError::new(
        ErrorCode::CasLost,
        format!("Plan {id} changed while it was being locked: rerun the command."),
    )
    .with("plan", id.as_str())
}

/// `cpd watch --ensure` (commands.md §12), run as a subprocess with `cpd`
/// resolved on `PATH` as the panes do (Escalation `rs-plan-a1-q2`): the
/// engine's start belongs to the watch area. Returns its fields without `ok`.
/// If it fails, the Plan stays locked and the result is `watch_start_failed`
/// with the ensure's error in `details.error` (commands.md §4.8).
fn ensure_watch(env: &Env, plan: &PlanId) -> Result<Value, CpdError> {
    ensure_watch_with(&*Ports::current().runner, env.home.root(), plan)
}

/// [`ensure_watch`] on `runner`: one call of `cpd watch --ensure` with `CPD_HOME=<root>` and `root` as its cwd.
pub(crate) fn ensure_watch_with(
    runner: &dyn Runner,
    root: &Path,
    plan: &PlanId,
) -> Result<Value, CpdError> {
    let argv = [PROGRAM, "watch", "--ensure"];
    let failed = |why: String, error: Value| {
        CpdError::new(
            ErrorCode::WatchStartFailed,
            format!("Plan {plan} is locked, but the engine could not be started: {why}"),
        )
        .with("plan", plan.as_str())
        .with("argv", json!(argv))
        .with("error", error)
        .with("next", "cpd watch --ensure")
    };
    let mut command = Command::new(argv[0]);
    command
        .args(&argv[1..])
        .env("CPD_HOME", root)
        .current_dir(root)
        .stdin(Stdio::null());
    let out = runner
        .output(&mut command, "cpd", NO_DEADLINE)
        .map_err(|e| match e {
            Failed::Io(e) => failed(
                format!("cannot run {}: {e}.", argv[0]),
                json!({"message": e.to_string()}),
            ),
            Failed::Timeout(e) => failed(e.message().to_owned(), json!({"message": e.message()})),
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Option<Map<String, Value>> = serde_json::from_str::<Value>(stdout.trim())
        .ok()
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        });
    let unexpected = || {
        json!({"exit": out.status.code(), "stdout": tail(&stdout, TAIL_LINES),
               "stderr": tail(&String::from_utf8_lossy(&out.stderr), TAIL_LINES)})
    };
    if !out.status.success() {
        let error = parsed
            .as_ref()
            .and_then(|m| m.get("error"))
            .cloned()
            .unwrap_or_else(unexpected);
        let why = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("it failed")
            .to_owned();
        return Err(failed(why, error));
    }
    match parsed {
        Some(mut fields) if fields.get("ok") == Some(&json!(true)) => {
            fields.remove("ok");
            Ok(Value::Object(fields))
        }
        _ => Err(failed(
            "it printed no success value.".to_owned(),
            unexpected(),
        )),
    }
}
