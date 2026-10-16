//! `plan lock` (commands.md §4.8, contract §3.1, §7.2): `plan → dispatch`.
//! Creates the Lead worktree, queues every draft Job with its Locked-Job
//! fingerprint, and starts the engine with `cpd watch --ensure`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::jobs::{self, Job};
use super::review::{latest_approved_for, latest_json, no_draft_jobs};
use super::row::{self, Phase, PlanRow};
use super::{Env, LockArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git::{self, Setup, TAIL_LINES, discard_worktree, tail};
use crate::home::Hook;
use crate::ids::PlanId;
use crate::output::Output;
use crate::session::PROGRAM;

const COMMAND: &str = "plan lock";

pub(super) fn lock(args: LockArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (plan, jobs) = env.store.read(|conn| Ok((row::load(conn, &id)?, jobs::load(conn, &id)?)))?;
    match plan.phase {
        Phase::Plan => {}
        Phase::Dispatch => {
            let watch = ensure_watch(&env, &plan)?;
            return Ok(Output::success(json!({
                "plan": id, "phase": "dispatch", "lead_branch": plan.lead_branch,
                "lead_worktree": plan.lead_worktree, "watch": watch, "already": true,
            })));
        }
        other => {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!("Plan {id} is in phase {}: only a Plan in phase plan is locked.", other.as_str()),
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
    let fingerprint = jobs::plan_fingerprint(&jobs);
    let (review, latest) = env.store.read(|conn| Ok((latest_approved_for(conn, &id, &fingerprint)?, latest_json(conn, &id)?)))?;
    let Some(review) = review else {
        return Err(CpdError::new(
            ErrorCode::PlanNotApproved,
            format!("The latest Plan review of {id} is not approved for the current draft Jobs: run cpd plan review."),
        )
        .with("plan", id.as_str())
        .with("fingerprint", fingerprint)
        .with("review", latest)
        .with("next", format!("cpd plan review --plan {id}")));
    };
    if let Some(cycle) = jobs::cycle(&jobs) {
        return Err(CpdError::new(ErrorCode::DependencyCycle, format!("The draft Jobs of {id} depend on each other in a cycle: {}.", cycle.join(" → ")))
            .with("plan", id.as_str())
            .with("cycle", cycle));
    }

    // git, outside any transaction (§7.2); on failure nothing was written.
    let repo = Path::new(&plan.repo);
    let setup = Setup {
        timeout: Duration::from_secs(u64::from(env.config.timeouts.setup_minutes) * 60),
        log: env.home.lead_hook_log(&id, Hook::PreStart),
    };
    let created = git::create_lead_worktree(repo, &id, &plan.base, &setup)?;
    let worktree = created.path.display().to_string();

    let keys: Vec<String> = drafts.iter().map(|j| j.key.clone()).collect();
    let committed = env.store.write(COMMAND, |tx| {
        // Compare-and-swap on what was read: the phase, the drafts and the review.
        let jobs_now = jobs::load(tx, &id)?;
        let review_now = latest_approved_for(tx, &id, &fingerprint)?;
        if jobs::plan_fingerprint(&jobs_now) != fingerprint || review_now.as_deref() != Some(review.as_str()) {
            return Err(cas_lost(&id));
        }
        let moved = tx.execute(
            "UPDATE plans SET phase = 'dispatch', lead_worktree = ?1, locked_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE id = ?2 AND phase = 'plan'",
            [worktree.as_str(), id.as_str()],
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
        append(tx, NewEvent::new(Kind::PlanPhase, text, json!({"from": "plan", "to": "dispatch", "jobs": keys})).plan(id.clone()))?;
        if let Some(error) = &created.push_failed {
            // R35: a failed push does not fail the lock; the next merge or `pr open` pushes again.
            let text = format!(
                "The Lead Branch {} of Plan {id} is locked but was not pushed to origin: {}. The next merge or cpd pr open pushes it again.",
                created.branch,
                error.message()
            );
            let data = json!({
                "source": COMMAND, "code": error.code().as_str(),
                "message": error.message(), "details": error.details(),
            });
            append(tx, NewEvent::new(Kind::Error, text, data).plan(id.clone()))?;
        }
        Ok(())
    });
    if let Err(e) = committed {
        // Best effort: the failure to report is the transaction's.
        let _ = discard_worktree(repo, &created.path, &created.branch);
        return Err(e);
    }

    let plan = env.store.read(|conn| row::load(conn, &id))?;
    let watch = ensure_watch(&env, &plan)?;
    Ok(Output::success(json!({
        "plan": id, "phase": "dispatch", "lead_branch": created.branch, "lead_worktree": worktree,
        "base_sha": created.base_sha, "jobs": keys, "watch": watch,
    })))
}

fn cas_lost(id: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::CasLost, format!("Plan {id} changed while it was being locked: rerun the command."))
        .with("plan", id.as_str())
}

/// `cpd watch --ensure` (commands.md §12), run as a subprocess with `cpd`
/// resolved on `PATH` as the panes do (Escalation `rs-plan-a1-q2`): the
/// engine's start belongs to the watch area. Returns its fields without `ok`.
/// If it fails, the Plan stays locked and the result is `watch_start_failed`
/// with the ensure's error in `details.error` (commands.md §4.8).
fn ensure_watch(env: &Env, plan: &PlanRow) -> Result<Value, CpdError> {
    let argv = [PROGRAM, "watch", "--ensure"];
    let failed = |why: String, error: Value| {
        CpdError::new(
            ErrorCode::WatchStartFailed,
            format!("Plan {} is locked, but the engine could not be started: {why}", plan.id),
        )
        .with("plan", plan.id.as_str())
        .with("lead_worktree", plan.lead_worktree.clone())
        .with("argv", json!(argv))
        .with("error", error)
        .with("next", "cpd watch --ensure")
    };
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .env("CPD_HOME", env.home.root())
        .current_dir(env.home.root())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| failed(format!("cannot run {}: {e}.", argv[0]), json!({"message": e.to_string()})))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Option<Map<String, Value>> = serde_json::from_str::<Value>(stdout.trim()).ok().and_then(|v| match v {
        Value::Object(map) => Some(map),
        _ => None,
    });
    let unexpected = || {
        json!({"exit": out.status.code(), "stdout": tail(&stdout, TAIL_LINES),
               "stderr": tail(&String::from_utf8_lossy(&out.stderr), TAIL_LINES)})
    };
    if !out.status.success() {
        let error = parsed.as_ref().and_then(|m| m.get("error")).cloned().unwrap_or_else(unexpected);
        let why = error.get("message").and_then(Value::as_str).unwrap_or("it failed").to_owned();
        return Err(failed(why, error));
    }
    match parsed {
        Some(mut fields) if fields.get("ok") == Some(&json!(true)) => {
            fields.remove("ok");
            Ok(Value::Object(fields))
        }
        _ => Err(failed("it printed no success value.".to_owned(), unexpected())),
    }
}
