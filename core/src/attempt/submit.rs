//! `attempt submit` (commands.md §6.1, contract §7.3): the Worker's preconditions,
//! the quick check, and the one transaction that moves the Attempt to `merging`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::row::{self, Attempt};
use super::{Env, SubmitArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::{self, HookOutcome, Operation, run_hook};
use crate::home::Hook;
use crate::ids::{AttemptId, PlanId};
use crate::output::Output;

const COMMAND: &str = "attempt submit";
const NEXT: &str = "Stop now. Do not make more changes.";

pub(super) fn submit(args: SubmitArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let id = args.attempt;
    let mut env = Env::open()?;
    let attempt = env.store.read(|conn| row::load(conn, &plan, &id))?;
    let worktree = PathBuf::from(&attempt.worktree);

    // 1. The Attempt is running; a submit that already happened is `already`.
    if attempt.status != "running" {
        return match already(&attempt, head(&worktree)?.as_deref()) {
            Some(sha) => Ok(done(&id, &sha, true)),
            None => Err(not_running(&attempt)),
        };
    }

    // 2. The caller is inside the Attempt worktree, on the Attempt branch.
    let root = require_inside(&attempt, &worktree)?;
    // 3. Clean tree, untracked files included.
    let status = git::run::ok(&root, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        return Err(CpdError::new(
            ErrorCode::DirtyTree,
            "The worktree has uncommitted or untracked files: commit them or delete them, then submit again.",
        )
        .with("attempt", id.to_string())
        .with("files", status.lines().map(|l| l.trim().to_owned()).collect::<Vec<_>>()));
    }
    // 4. At least one commit ahead of base_sha; a review Job (B2) instead: none, and its findings.
    let base = attempt
        .base_sha
        .clone()
        .ok_or_else(|| CpdError::bad_row(format!("Attempt {plan}/{id} is running with no base_sha")))?;
    let ahead = git::run::ok(&root, &["rev-list", "--count", &format!("{base}..HEAD")])?;
    let review = env.store.read(|conn| crate::plan::jobs::load(conn, &plan)?.iter().find(|j| j.key == attempt.job).map(|j| j.review).ok_or_else(|| CpdError::bad_row(format!("Attempt {plan}/{id} has no Job {}", attempt.job))))?;
    if review {
        require_review(&env, &plan, &id, &attempt.job, &base, ahead.trim() != "0")?;
    } else if ahead.trim() == "0" {
        return Err(CpdError::new(ErrorCode::NoCommits, "There is no commit on top of the base: commit your work, then submit again.")
            .with("attempt", id.to_string())
            .with("base_sha", base));
    }
    let sha = git::run::rev_parse(&root, "HEAD")?;
    // 5 and 6. The tamper check, against the Lead Branch of the Job's PR (R34 A5), and the
    // quick check; neither for a review Job: nothing was changed to check.
    let check_tail = if review {
        None
    } else {
        let (lead, pr_base) = env.store.read(|conn| lead_branch(conn, &plan, &attempt.job))?;
        let base_ref = git::tamper::base_ref(&root, &pr_base)?;
        git::tamper_check(&root, &lead, &base_ref, &base, &sha)?;
        quick_check(&env, &plan, &id, &root, &lead)?
    };

    // The Worker is the only one who moves its HEAD, and it is waiting for us: but the
    // compare-and-swap of §6.1 is on the HEAD that was checked.
    if git::run::rev_parse(&root, "HEAD")? != sha {
        return Err(CpdError::new(ErrorCode::CasLost, "HEAD moved while the quick check ran: submit again.")
            .with("attempt", id.to_string()));
    }

    let outcome = env.store.write(COMMAND, |tx| {
        let now = row::load(tx, &plan, &id)?;
        if now.status != "running" {
            return match already(&now, Some(&sha)) {
                Some(_) => Ok(true),
                None => Err(not_running(&now)),
            };
        }
        let moved = tx.execute(
            "UPDATE attempts SET status = 'merging', submitted_sha = ?3, check_tail = ?4,
                    submitted_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND id = ?2 AND status = 'running'",
            rusqlite::params![plan.as_str(), id.to_string(), sha, check_tail],
        )?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {id} changed meanwhile: submit again."))
                .with("attempt", id.to_string()));
        }
        let text = format!(
            "Attempt {id} of Job {} was submitted{}; the driver merges it. Next: cpd attempt show {id} --plan {plan}",
            now.job,
            if review { " (a review Job: nothing to check)" } else { " and passed the quick check" }
        );
        let job = crate::ids::JobKey::parse(&now.job).map_err(|_| CpdError::bad_row(format!("job key {:?}", now.job)))?;
        append(
            tx,
            NewEvent::attempt_status(text, "running", "merging")
                .plan(plan.clone())
                .job(job)
                .attempt(id.clone()),
        )?;
        Ok(false)
    })?;
    Ok(done(&id, &sha, outcome))
}

/// A review Job's preconditions (B2, contract §7.3 precondition 3): no commit
/// ahead of `base` (`review_commits`), and a non-empty `run/<plan>/reviews/<job>.md` (`review_missing`).
fn require_review(env: &Env, plan: &PlanId, id: &AttemptId, job: &str, base: &str, committed: bool) -> Result<(), CpdError> {
    if committed {
        return Err(CpdError::new(
            ErrorCode::ReviewCommits,
            "A review Job commits nothing: undo your commits (git reset --hard to the base commit), then submit again.",
        )
        .with("attempt", id.to_string())
        .with("base_sha", base));
    }
    let findings = crate::job::findings_path(&env.home, plan, job);
    if !std::fs::metadata(&findings).is_ok_and(|m| m.is_file() && m.len() > 0) {
        return Err(CpdError::new(
            ErrorCode::ReviewMissing,
            format!("Write your findings to {} (not empty), then submit again.", findings.display()),
        )
        .with("attempt", id.to_string())
        .with("path", findings.display().to_string()));
    }
    Ok(())
}

/// The Lead Branch and the base of the PR that holds Job `job` of `plan`.
fn lead_branch(conn: &rusqlite::Connection, plan: &PlanId, job: &str) -> Result<(String, String), CpdError> {
    Ok(conn.query_row(
        "SELECT r.lead_branch, r.base FROM jobs j JOIN prs r ON r.plan = j.plan AND r.key = j.pr WHERE j.plan = ?1 AND j.key = ?2",
        [plan.as_str(), job],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// The success output; `already` marks a rerun that found the effect in place.
fn done(id: &AttemptId, sha: &str, already: bool) -> Output {
    let mut out = json!({"attempt": id, "submitted": true, "sha": sha, "next": NEXT});
    if already {
        out["already"] = json!(true);
    }
    Output::success(out)
}

/// The submitted sha when the Attempt is `merging` or `ended` and was submitted at `head`.
fn already(attempt: &Attempt, head: Option<&str>) -> Option<String> {
    let sha = attempt.submitted_sha.as_deref()?;
    (matches!(attempt.status.as_str(), "merging" | "ended") && head == Some(sha)).then(|| sha.to_owned())
}

fn not_running(attempt: &Attempt) -> CpdError {
    CpdError::new(
        ErrorCode::AttemptNotRunning,
        format!("Attempt {} is {}, not running: stop, there is nothing to submit.", attempt.id, attempt.status),
    )
    .with("attempt", attempt.id.as_str())
    .with("status", attempt.status.as_str())
    .with("outcome", attempt.outcome.clone())
    .with("cause", attempt.cause.clone())
}

/// `git rev-parse HEAD` of the worktree; `None` when it is gone or has no HEAD.
fn head(worktree: &Path) -> Result<Option<String>, CpdError> {
    let out = git::run::run(worktree, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    Ok((out.code == 0).then(|| out.stdout.trim().to_owned()))
}

/// The worktree's real path, when the caller's cwd is inside it and it is on the Attempt branch;
/// else `wrong_worktree`.
fn require_inside(attempt: &Attempt, worktree: &Path) -> Result<PathBuf, CpdError> {
    let wrong = |why: String| {
        CpdError::new(
            ErrorCode::WrongWorktree,
            format!("{why}: run cpd attempt submit from the Attempt's worktree {}.", worktree.display()),
        )
        .with("attempt", attempt.id.as_str())
        .with("worktree", attempt.worktree.as_str())
        .with("branch", attempt.branch.as_str())
    };
    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| wrong(format!("The current directory is unreadable ({e})")))?;
    let root = worktree.canonicalize().map_err(|_| wrong("The Attempt's worktree does not exist".to_owned()))?;
    if !cwd.starts_with(&root) {
        return Err(wrong(format!("The current directory {} is outside the worktree", cwd.display())));
    }
    match git::run::current_branch(&root)? {
        Some(branch) if branch == attempt.branch => Ok(root),
        found => Err(wrong(format!("The worktree is on {}, not on {}", found.as_deref().unwrap_or("no branch"), attempt.branch))),
    }
}

/// The quick check of §7.3: `pre-commit` in the worktree. Its last 60 lines for `check_tail`;
/// `None` when nothing ran. Refuses `check_failed` (exit 3) or `check_error` (exit 1).
fn quick_check(env: &Env, plan: &PlanId, id: &AttemptId, root: &Path, lead: &str) -> Result<Option<String>, CpdError> {
    let log = env.home.hook_log(plan, id, Hook::PreCommit);
    let timeout = Duration::from_secs(u64::from(env.config.timeouts.gate_minutes) * 60);
    let operation = Operation { branch: lead };
    let result = run_hook(Hook::PreCommit, root, Some(&operation), timeout, &log)?;
    let refuse = |code: ErrorCode, message: String| {
        let mut e = CpdError::new(code, message).with("attempt", id.to_string()).with("tail", result.tail.clone());
        if result.tail.is_some() {
            e = e.with("log", log.display().to_string());
        }
        e
    };
    match &result.outcome {
        HookOutcome::Passed => Ok(result.tail),
        HookOutcome::Failed { exit } => Err(refuse(
            ErrorCode::CheckFailed,
            format!("The quick check failed (exit {exit}): fix it, commit, and submit again. The tail is in details.tail."),
        )
        .with("exit", *exit)),
        HookOutcome::Error(why) => Err(refuse(
            ErrorCode::CheckError,
            format!("The quick check could not be judged: {why}. Ask the Lead: cpd ask {id} \"<question>\"."),
        )
        .with("reason", why.to_string())),
    }
}
