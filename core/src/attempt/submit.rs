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
    let mut env = Env::open()?;
    submit_in(&mut env, args)
}

/// [`submit`] on an open `env`: what a test hands in.
pub(super) fn submit_in(env: &mut Env, args: SubmitArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let id = args.attempt;
    let attempt = env.store.read(|conn| row::load(conn, &plan, &id))?;
    let kind: String = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT kind FROM jobs WHERE plan = ?1 AND key = ?2",
            [plan.as_str(), attempt.job.as_str()],
            |r| r.get(0),
        )?)
    })?;
    if crate::plan::jobs::is_explore(&kind) {
        return submit_explore(env, &plan, &attempt, args.summary_file.as_deref());
    }
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
    // 4. At least one commit ahead of base_sha.
    let base = attempt.base_sha.clone().ok_or_else(|| {
        CpdError::bad_row(format!("Attempt {plan}/{id} is running with no base_sha"))
    })?;
    let ahead = git::run::ok(&root, &["rev-list", "--count", &format!("{base}..HEAD")])?;
    let job = env.store.read(|conn| {
        crate::plan::jobs::load(conn, &plan)?
            .into_iter()
            .find(|j| j.key == attempt.job)
            .ok_or_else(|| {
                CpdError::bad_row(format!("Attempt {plan}/{id} has no Job {}", attempt.job))
            })
    })?;
    // An `open` or `update` Job needs no commit: the base may not have moved (§7.3 precondition 3, §7.7).
    let cpd_job = matches!(job.kind.as_str(), "open" | "update");
    if ahead.trim() == "0" && !cpd_job {
        return Err(CpdError::new(
            ErrorCode::NoCommits,
            "There is no commit on top of the base: commit your work, then submit again.",
        )
        .with("attempt", id.to_string())
        .with("base_sha", base));
    }
    let sha = git::run::rev_parse(&root, "HEAD")?;
    // 5 and 6. The checks, in ONE `checks` span (contract §7.7): the Worker's clocks stop from the first check to the end of the
    // merge. Every way out ends the span once, before the error returns.
    let span = span::open(env, &plan, &id, &job.key)?;
    let checked = run_checks(
        env,
        &plan,
        &id,
        &job,
        &attempt.job,
        &attempt.branch,
        &root,
        &base,
        &sha,
    );
    // The Worker is the only one who moves its HEAD, and it is waiting for us: but the
    // compare-and-swap of §6.1 is on the HEAD that was checked.
    let checked = checked.and_then(|files| match git::run::rev_parse(&root, "HEAD") {
        Ok(now) if now == sha => Ok(files),
        Ok(_) => Err(CpdError::new(
            ErrorCode::CasLost,
            "HEAD moved while the checks ran: submit again.",
        )
        .with("attempt", id.to_string())),
        Err(e) => Err(e),
    });
    let actual_files = match checked {
        Ok(files) => files,
        Err(e) => {
            // The refusal is what returns; a failed `end` is E2's to close (`died: true`).
            let _ = span.end(env);
            return Err(e);
        }
    };
    let actual_files = serde_json::to_string(&actual_files)
        .map_err(|e| CpdError::bad_row(format!("actual_files: {e}")))?;

    let written = env.store.write(COMMAND, |tx| {
        let now = row::load(tx, &plan, &id)?;
        if now.status != "running" {
            return match already(&now, Some(&sha)) {
                Some(_) => Ok(true),
                None => Err(not_running(&now)),
            };
        }
        // The span ends in this transaction, right before the Attempt is `merging`.
        span.end_in(tx)?;
        let moved = tx.execute(
            "UPDATE attempts SET status = 'merging', submitted_sha = ?3, actual_files = ?4,
                    submitted_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND id = ?2 AND status = 'running'",
            rusqlite::params![plan.as_str(), id.to_string(), sha, actual_files],
        )?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {id} changed meanwhile: submit again."))
                .with("attempt", id.to_string()));
        }
        let text = format!(
            "Attempt {id} of Job {} was submitted; the driver merges it. Next: cpd attempt show {id} --plan {plan}",
            now.job
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
            let _ = span.end(env);
            return Err(e);
        }
    };
    Ok(done(&id, &sha, outcome))
}

/// `attempt submit` of an explore Attempt (contract §7.7 "Explore kinds at submit"): three refusals, no check, no git; on a pass the
/// Attempt, the Job and the `job-merged` Event end in one transaction. The Worker is stopped by the driver's A9, never here.
fn submit_explore(
    env: &mut Env,
    plan: &PlanId,
    attempt: &Attempt,
    summary_file: Option<&Path>,
) -> Result<Output, CpdError> {
    use sha2::{Digest, Sha256};

    let id = AttemptId::parse(&attempt.id)
        .map_err(|_| CpdError::bad_row(format!("attempt id {:?}", attempt.id)))?;
    if attempt.status != "running" {
        let accepted = attempt.status == "ended" && attempt.outcome.as_deref() == Some("merged");
        return match attempt.submitted_sha.as_deref().filter(|_| accepted) {
            Some(sha) => Ok(done(&id, sha, true)),
            None => Err(not_running(attempt)),
        };
    }
    let artefact = crate::driver::explore_dir(&env.home, plan).join(format!("{}.md", attempt.job));
    let body = match std::fs::read(&artefact) {
        Ok(body) if !body.is_empty() => body,
        _ => {
            return Err(CpdError::new(
                ErrorCode::ArtefactMissing,
                format!("There is no non-empty artefact at {}: write your findings there, then submit again.", artefact.display()),
            )
            .with("attempt", attempt.id.as_str())
            .with("artefact", artefact.display().to_string()));
        }
    };
    let missing = |why: &str| {
        CpdError::new(
            ErrorCode::SummaryMissing,
            format!(
                "{why}: pass --summary-file <path> with a non-empty summary, then submit again."
            ),
        )
        .with("attempt", attempt.id.as_str())
    };
    let path = summary_file.ok_or_else(|| missing("No --summary-file was given"))?;
    let summary = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => text,
        Ok(_) => {
            return Err(missing(&format!(
                "The summary file {} is empty",
                path.display()
            )));
        }
        Err(e) => {
            return Err(missing(&format!(
                "The summary file {} cannot be read ({e})",
                path.display()
            )));
        }
    };
    let length = summary.chars().count();
    if length > crate::spec::SUMMARY_CAP {
        return Err(CpdError::new(
            ErrorCode::SummaryTooLong,
            format!(
                "The summary has {length} characters, more than {}: shorten it, then submit again.",
                crate::spec::SUMMARY_CAP
            ),
        )
        .with("attempt", attempt.id.as_str())
        .with("length", length as u64)
        .with("cap", crate::spec::SUMMARY_CAP as u64));
    }
    let sha: String = Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let written = env.store.write(COMMAND, |tx| {
        let now = crate::attempt::ledger::load(tx, plan, &id)?;
        crate::attempt::ledger::end_explore(tx, &now, &sha, &summary)
    });
    match written {
        Ok(()) => Ok(done(&id, &sha, false)),
        Err(e) if e.code() == ErrorCode::CasLost => {
            // Rolled back: another submit or an end came first.
            let now = env.store.read(|conn| row::load(conn, plan, &id))?;
            Err(not_running(&now))
        }
        Err(e) => Err(e),
    }
}

/// The checks of a Job, in order: the tamper check, the guard of the `test` Jobs' files and the kind's check
/// (`[test-commands]`). The quick check and the Gates run at the merge (§7.4 step 6, hs-d679).
#[allow(clippy::too_many_arguments)]
fn run_checks(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &crate::plan::jobs::Job,
    key: &str,
    branch: &str,
    root: &Path,
    base: &str,
    sha: &str,
) -> Result<Vec<String>, CpdError> {
    // A stacked Workflow's check reads the root base too (§7.5), so one more ref than an unstacked Workflow's.
    let (lead, base_refs) = env.store.read(|conn| {
        let (lead, repo, workflow_base) = lead_branch(conn, plan, key)?;
        Ok((lead, crate::pr::start::tamper_bases(conn, root, &repo, &workflow_base)?))
    })?;
    // A linear Attempt runs on the Lead Branch, so the Lead Branch diff `lead...sha` is empty: its tamper and
    // red-test diffs compare against the Attempt's `base_sha` instead (hs-d716), as the merge does (hs-d680).
    let compare = if git::worktree::is_linear(branch, &lead) {
        base
    } else {
        &lead
    };
    let policy = env
        .store
        .read(|conn| crate::pr::policy_of_job(conn, &env.home, plan, key))?;
    let base_refs: Vec<&str> = base_refs.iter().map(String::as_str).collect();
    git::tamper::check_over(
        root,
        compare,
        &policy.protected.paths,
        &base_refs,
        base,
        sha,
    )?;
    // The Job's Spec `touches` (§7.5, hs-n641): the same diff may not change a path outside them, unless a base merge
    // brought it, the repo's `[generated] paths` match it, or the Lead approved it (`job approve-touches`, read now so
    // an approval reaches a running Attempt).
    let touches = crate::job::guard::touches(job);
    let approved = crate::job::guard::approved_touches(job);
    let check = git::tamper::Touches {
        kind: &job.kind,
        touches: &touches,
        generated: &policy.generated.paths,
        approved: &approved,
    };
    let outside = git::tamper::touched_outside(root, compare, &check, &base_refs, base, sha)?;
    if !outside.is_empty() {
        return Err(CpdError::new(
            ErrorCode::TouchesViolation,
            format!(
                "The diff changes paths outside the Job's touches: {}. Revert them, or ask the Lead, who approves a path with `cpd job approve-touches`.",
                outside.join(", ")
            ),
        )
        .with("paths", json!(outside))
        .with("touches", json!(touches))
        .with("generated", json!(policy.generated.paths)));
    }
    // The red tests of the `test` Jobs an `impl` Job needs are guarded the same way (§7.5, §7.7).
    kind::guard_tests(env, plan, job, root, compare, base, sha)?;
    // The kind's check (§7.3 precondition 6, §7.7): a `test` or `impl` Job's `[test-commands]`. The test Job's diff
    // is the Attempt's base for every Attempt (a parallel Attempt's `base_sha` is its branch point too).
    if kind::has_checks(job) {
        kind::check(env, plan, id, job, &policy, root, &lead, base, sha)?;
    }
    // The written files (§7.3): the paths of `base...sha` that differ from every comparison ref. A linear Attempt's
    // branch is the Lead Branch, so the Lead tip is not a comparison ref.
    let mut compare_with: Vec<&str> = base_refs.clone();
    if !git::worktree::is_linear(branch, &lead) {
        compare_with.push(&lead);
    }
    git::tamper::written_paths(root, base, sha, &compare_with)
}

/// The Lead Branch and the base of the Workflow that holds Job `job` of `plan`.
fn lead_branch(conn: &rusqlite::Connection, plan: &PlanId, job: &str) -> Result<(String, String, String), CpdError> {
    Ok(conn.query_row(
        "SELECT r.lead_branch, r.repo, r.base FROM jobs j JOIN workflows r ON r.plan = j.plan AND r.key = j.workflow WHERE j.plan = ?1 AND j.key = ?2",
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
    (matches!(attempt.status.as_str(), "merging" | "ended") && head == Some(sha))
        .then(|| sha.to_owned())
}

fn not_running(attempt: &Attempt) -> CpdError {
    CpdError::new(
        ErrorCode::AttemptNotRunning,
        format!(
            "Attempt {} is {}, not running: stop, there is nothing to submit.",
            attempt.id, attempt.status
        ),
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
pub(super) fn require_inside(attempt: &Attempt, worktree: &Path) -> Result<PathBuf, CpdError> {
    let wrong = |why: String| {
        CpdError::new(
            ErrorCode::WrongWorktree,
            format!(
                "{why}: run cpd attempt submit from the Attempt's worktree {}.",
                worktree.display()
            ),
        )
        .with("attempt", attempt.id.as_str())
        .with("worktree", attempt.worktree.as_str())
        .with("branch", attempt.branch.as_str())
    };
    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| wrong(format!("The current directory is unreadable ({e})")))?;
    let root = worktree
        .canonicalize()
        .map_err(|_| wrong("The Attempt's worktree does not exist".to_owned()))?;
    if !cwd.starts_with(&root) {
        return Err(wrong(format!(
            "The current directory {} is outside the worktree",
            cwd.display()
        )));
    }
    match git::run::current_branch(&root)? {
        Some(branch) if branch == attempt.branch => Ok(root),
        found => Err(wrong(format!(
            "The worktree is on {}, not on {}",
            found.as_deref().unwrap_or("no branch"),
            attempt.branch
        ))),
    }
}
