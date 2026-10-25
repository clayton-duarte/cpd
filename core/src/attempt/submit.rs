//! `attempt submit` (commands.md §6.1, contract §7.3): the Worker's preconditions,
//! the `checks` span (the tamper check, the red-test guard and the kind's tests), and the one transaction that moves the Attempt to `merging`.
//! The quick check and the Gates run at the merge (§7.4 step 6), not here (hs-d679).

use std::path::{Path, PathBuf};

use serde_json::json;

use super::row::{self, Attempt};
use super::{Env, SubmitArgs, kind, span};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git;
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
    let job = env.store.read(|conn| {
        crate::plan::jobs::load(conn, &plan)?
            .into_iter()
            .find(|j| j.key == attempt.job)
            .ok_or_else(|| CpdError::bad_row(format!("Attempt {plan}/{id} has no Job {}", attempt.job)))
    })?;
    let review = job.review;
    // An `open` or `update` Job needs no commit: the base may not have moved (§7.3 precondition 3, §7.7).
    let cpd_job = matches!(job.kind.as_str(), "open" | "update");
    if review {
        require_review(&env, &plan, &id, &attempt.job, &base, ahead.trim() != "0")?;
    } else if ahead.trim() == "0" && !cpd_job {
        return Err(CpdError::new(ErrorCode::NoCommits, "There is no commit on top of the base: commit your work, then submit again.")
            .with("attempt", id.to_string())
            .with("base_sha", base));
    }
    let sha = git::run::rev_parse(&root, "HEAD")?;
    // 5 and 6. The checks, in ONE `checks` span (contract §7.7): the Worker's clocks stop from the first check to the end of the
    // merge. A review Job has no checks and no span. Every way out ends the span once, before the error returns.
    let span = if review { None } else { Some(span::open(&mut env, &plan, &id, &job.key)?) };
    let checked = if review { Ok(()) } else { run_checks(&mut env, &plan, &id, &job, &attempt.job, &root, &base, &sha) };
    // The Worker is the only one who moves its HEAD, and it is waiting for us: but the
    // compare-and-swap of §6.1 is on the HEAD that was checked.
    let checked = checked.and_then(|()| match git::run::rev_parse(&root, "HEAD") {
        Ok(now) if now == sha => Ok(()),
        Ok(_) => Err(CpdError::new(ErrorCode::CasLost, "HEAD moved while the checks ran: submit again.").with("attempt", id.to_string())),
        Err(e) => Err(e),
    });
    match checked {
        Ok(()) => {}
        Err(e) => {
            if let Some(span) = span {
                // The refusal is what returns; a failed `end` is E2's to close (`died: true`).
                let _ = span.end(&mut env);
            }
            return Err(e);
        }
    };

    let written = env.store.write(COMMAND, |tx| {
        let now = row::load(tx, &plan, &id)?;
        if now.status != "running" {
            return match already(&now, Some(&sha)) {
                Some(_) => Ok(true),
                None => Err(not_running(&now)),
            };
        }
        // The span ends in this transaction, right before the Attempt is `merging`.
        if let Some(span) = &span {
            span.end_in(tx)?;
        }
        let moved = tx.execute(
            "UPDATE attempts SET status = 'merging', submitted_sha = ?3,
                    submitted_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND id = ?2 AND status = 'running'",
            rusqlite::params![plan.as_str(), id.to_string(), sha],
        )?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {id} changed meanwhile: submit again."))
                .with("attempt", id.to_string()));
        }
        let text = format!(
            "Attempt {id} of Job {} was submitted{}; the driver merges it. Next: cpd attempt show {id} --plan {plan}",
            now.job,
            if review { " (a review Job: nothing to check)" } else { "" }
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
    });
    // Rolled back: the span is still open, so it ends in a transaction of its own; committed: the lock goes with `span`.
    let outcome = match written {
        Ok(outcome) => outcome,
        Err(e) => {
            if let Some(span) = span {
                let _ = span.end(&mut env);
            }
            return Err(e);
        }
    };
    Ok(done(&id, &sha, outcome))
}

/// The checks of a non-review Job, in order: the tamper check, the guard of the `test` Jobs' files and the kind's check
/// (`[test-commands]`). The quick check and the Gates run at the merge (§7.4 step 6, hs-d679).
#[allow(clippy::too_many_arguments)]
fn run_checks(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &crate::plan::jobs::Job,
    key: &str,
    root: &Path,
    base: &str,
    sha: &str,
) -> Result<(), CpdError> {
    // A stacked PR's check reads the root base too (§7.5), so one more ref than an unstacked PR's.
    let (lead, base_refs) = env.store.read(|conn| {
        let (lead, repo, pr_base) = lead_branch(conn, plan, key)?;
        Ok((lead, crate::pr::start::tamper_bases(conn, root, &repo, &pr_base)?))
    })?;
    let policy = env.store.read(|conn| crate::pr::policy_of_job(conn, &env.home, &env.config.host, plan, key))?;
    git::tamper::check_over(root, &lead, &policy.protected.paths, &base_refs.iter().map(String::as_str).collect::<Vec<_>>(), base, sha)?;
    // The red tests of the `test` Jobs an `impl` Job needs are guarded the same way (§7.5, §7.7).
    kind::guard_tests(env, plan, job, root, &lead, base, sha)?;
    // The kind's check (§7.3 precondition 6, §7.7): a `test` or `impl` Job's `[test-commands]`.
    if kind::has_checks(job) {
        kind::check(env, plan, id, job, &policy, root, &lead, sha)?;
    }
    Ok(())
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
fn lead_branch(conn: &rusqlite::Connection, plan: &PlanId, job: &str) -> Result<(String, String, String), CpdError> {
    Ok(conn.query_row(
        "SELECT r.lead_branch, r.repo, r.base FROM jobs j JOIN prs r ON r.plan = j.plan AND r.key = j.pr WHERE j.plan = ?1 AND j.key = ?2",
        [plan.as_str(), job],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
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
