//! The merge (contract §7.4): driver step A8, and the recovery R of an
//! interrupted merge, both without a repo lock (hs-d243). No command of its
//! own: the driver calls [`merge`] at A8; engine E2 calls [`recover_attempt`]
//! for each orphaned Attempt.
//!
//! No Gate runs here (R19): a merge is checked by the quick check at submit,
//! the tamper check (step 4) and a clean rebase (step 5).

mod recover;

use serde_json::Value;

use crate::attempt::ledger::{self, Stage};
use crate::config::{Catalog, Config};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, run::run};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::store::Store;

// The merge's causes are the shared `Cause`; `Failure` is the name its callers know.
pub use crate::attempt::ledger::{Attempt, Cause as Failure, End, Ladder, load};
pub use crate::driver::lock::{Free as DriverLock, take_free_driver_lock};
pub use recover::{Recovered, recover_attempt};

/// How many times step 8 goes back to step 2 when the Lead Branch moved,
/// before the merge ends `cas_lost` (§7.4 step 8).
pub const CAS_RETRIES: u32 = 3;

/// What the merge and R need from their caller.
pub struct Ctx<'a> {
    pub home: &'a CpdHome,
    /// `retry_cap` (the ladder) and `tmux` (R kills a recovered Attempt's window).
    pub config: &'a Config,
    /// pi's catalog, for the ladder's next Tier (§9).
    pub catalog: &'a Catalog,
    /// The caller, as `txns.command` and the `source` of `error` Events:
    /// `attempt run`, `watch`, `pr open` or `pr rebase`.
    pub source: &'a str,
    /// A9's `metrics` of an Attempt, recorded in the transaction that ends it.
    pub metrics: &'a dyn Fn(&Attempt) -> Option<Value>,
}

/// The result of [`merge`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge {
    /// How the Attempt ended; recorded in the Ledger.
    pub end: End,
}

/// Driver step A8 (§7.4) for Attempt `attempt` of `plan`, which must be
/// `merging`: steps 1–10, ending the Attempt in one transaction (§3.4). It
/// takes no lock and runs no R first (hs-d243): the compare-and-swap of step
/// 8, rule P and rule S keep concurrent merges of one PR apart.
///
/// An `Err` is an unexpected failure (git, the store) and leaves the Attempt
/// `merging`; the driver then ends it as commands.md §6.4 says. After an
/// `Err` that happened after the compare-and-swap, R finds the merge.
pub fn merge(store: &mut Store, ctx: &Ctx<'_>, plan: &PlanId, attempt: &AttemptId) -> Result<Merge, CpdError> {
    let target = store.read(|conn| ledger::load(conn, plan, attempt))?;
    if target.status != "merging" {
        return Err(CpdError::new(
            ErrorCode::IllegalTransition,
            format!("Attempt {attempt} is {}, not merging: there is nothing to merge.", target.status),
        )
        .with("attempt", attempt.to_string())
        .with("status", target.status));
    }
    let review = store.read(|conn| crate::job::find(&crate::plan::jobs::load(conn, plan)?, plan, &target.job).map(|j| j.review))?;
    let (end, stale, push) = if review { review_steps(store, &target)? } else { steps(store, &target)? };
    let metrics = (ctx.metrics)(&target);
    let ladder = Ladder { catalog: ctx.catalog, retry_cap: ctx.config.retry_cap };
    // Step 10, and every other end.
    store.write(ctx.source, |tx| {
        ledger::end(tx, &target, &end, metrics.as_ref(), &ladder, Stage::Merge)?;
        if let Some(e) = &stale {
            ledger::append_error(tx, &target, ctx.source, e, recover::stale_text(&target, e))?;
        }
        if let Some(e) = &push {
            ledger::append_error(tx, &target, ctx.source, e, recover::push_text(&target, e))?;
        }
        Ok(())
    })?;
    Ok(Merge { end })
}

/// What [`steps`] returns: how the Attempt ends, step 9's `lead_worktree_stale`
/// error and step 8's `push_failed` error, when they happened.
type Stepped = (End, Option<CpdError>, Option<CpdError>);

/// A review Job's merge (B2, §7.4): step 1 (cancel check), step 2 (snapshot `old`)
/// and step 7 (cancel check again); no tamper check, rebase, fast-forward, push
/// or worktree sync, because the Attempt has no commits and the Lead Branch does
/// not move; then the Attempt ends `merged` with `merged_sha = old`.
fn review_steps(store: &mut Store, target: &Attempt) -> Result<Stepped, CpdError> {
    if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
        return Ok((End::Cancelled, None, None));
    }
    let old = git::run::rev_parse(&target.repo, &format!("refs/heads/{}", target.lead_branch))?;
    if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
        return Ok((End::Cancelled, None, None));
    }
    Ok((End::Merged { sha: old }, None, None))
}

/// Steps 1–9, with step 8's push.
fn steps(store: &mut Store, target: &Attempt) -> Result<Stepped, CpdError> {
    let repo = &target.repo;
    let lead = &target.lead_worktree;
    let lead_ref = format!("refs/heads/{}", target.lead_branch);
    // 0. Branch check (engine-W1): the Attempt branch is still what `attempt submit` checked.
    let submitted = target.submitted_sha.as_deref().ok_or_else(|| {
        CpdError::new(ErrorCode::StoreFailed, format!("Attempt {} is merging with no submitted_sha.", target.id))
    })?;
    let tip = git::run::rev_parse(repo, &format!("refs/heads/{}", target.branch))?;
    if tip != submitted {
        let detail = format!("the Attempt branch moved after submit ({submitted} -> {tip})");
        return Ok((End::Failed { cause: Failure::Tampered, detail }, None, None));
    }
    let mut retries = 0;
    loop {
        // 1. Cancel check.
        if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            return Ok((End::Cancelled, None, None));
        }
        // 2. Snapshot.
        let old = git::run::rev_parse(repo, &lead_ref)?;
        // 3. Removed by hs-d243: the merge never works in the Lead worktree, and rule S
        // (step 9) neither overwrites local changes nor fails the Job for them.
        // 4. Tamper check of old...<attempt branch>.
        // A stacked PR's base is its base PR's Lead Branch, and its check reads the root base too (§7.5).
        let base_refs: Vec<String> = store.read(|conn| {
            let (pr_repo, pr_base): (String, String) = conn.query_row(
                "SELECT r.repo, r.base FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
                   JOIN prs r ON r.plan = j.plan AND r.key = j.pr WHERE a.plan = ?1 AND a.id = ?2",
                [target.plan.as_str(), target.id.to_string().as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            crate::pr::start::tamper_bases(conn, repo, &pr_repo, &pr_base)
        })?;
        let base_refs: Vec<&str> = base_refs.iter().map(String::as_str).collect();
        let touched = git::tamper::touched_protected_over(repo, &old, &base_refs, &old, &target.branch)?;
        if !touched.is_empty() {
            let detail = format!("The diff touches protected paths: {}.", touched.join(", "));
            return Ok((End::Failed { cause: Failure::Tampered, detail }, None, None));
        }
        // 5. Rebase in the Attempt worktree, never in the Lead worktree; skipped when the
        // Attempt already contains `old` (hs-d139), so a merge of the base survives.
        if !contains(&target.worktree, &old)? {
            if let Some(files) = rebase(&target.worktree, &old)? {
                let detail = format!("The rebase onto the Lead Branch conflicted in: {}.", files.join(", "));
                return Ok((End::Failed { cause: Failure::MergeConflict, detail }, None, None));
            }
        }
        // 6. Removed by R19: no Gate at the merge.
        // 7. Cancel check again: the last safe point.
        if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            return Ok((End::Cancelled, None, None));
        }
        // 8. Fast-forward with a compare-and-swap.
        let new = git::run::rev_parse(&target.worktree, "HEAD")?;
        git::run::ok(repo, &["merge-base", "--is-ancestor", &old, &new])?;
        let message = format!("cpd: merge {}/{}", target.plan, target.id);
        let out = run(repo, &["update-ref", "-m", &message, &lead_ref, &new, &old])?;
        if out.code == 0 {
            // Push (R35) by rule P: the merge stands whatever happens here.
            let push = push(lead, &target.lead_branch, &new);
            // 9. Rule S on the Lead worktree; the merge stands whatever happens here.
            let stale = git::sync_lead(lead, &target.lead_branch).err();
            return Ok((End::Merged { sha: new }, stale, push));
        }
        if git::run::rev_parse(repo, &lead_ref)? == old {
            return Err(CpdError::new(ErrorCode::GitFailed, "git update-ref failed, and the Lead Branch did not move.")
                .with("exit", out.code)
                .with("stderr", git::tail(&out.stderr, git::TAIL_LINES)));
        }
        // The ref moved: back to step 2.
        retries += 1;
        if retries > CAS_RETRIES {
            let detail = format!(
                "{} kept moving during the merge (other merges, a pr rebase, or someone outside cpd); the fast-forward lost {} times.",
                target.lead_branch,
                CAS_RETRIES + 1
            );
            return Ok((End::Failed { cause: Failure::CasLost, detail }, None, None));
        }
    }
}

/// Rule P (§7.4) for `sha` on the Lead Branch `branch`, from the Lead worktree
/// `lead`, as step 8 and R2 push: `None` when it pushed, when the remote
/// already contains `sha`, or when the repo has no `origin` (A3); else the
/// `push_failed` error to append. A push never fails the merge (R35).
pub(super) fn push(lead: &std::path::Path, branch: &str, sha: &str) -> Option<CpdError> {
    let failed = |stderr: &str, exit: Option<i64>, why: String| {
        let e = CpdError::new(ErrorCode::PushFailed, format!("git push of {branch} to origin {why}"))
            .with("branch", branch)
            .with("sha", sha)
            .with("stderr", git::tail(stderr, git::TAIL_LINES));
        match exit {
            Some(code) => e.with("exit", code),
            None => e,
        }
    };
    match git::run::has_origin(lead).and_then(|origin| if origin { git::push_commit(lead, branch, sha).map(Some) } else { Ok(None) }) {
        Ok(None | Some(git::Push::Pushed { .. } | git::Push::Contained { .. })) => None,
        Ok(Some(git::Push::Rejected { stderr })) => Some(failed(&stderr, None, "was rejected: not a fast-forward.".to_owned())),
        Err(e) => {
            let detail = |key: &str| e.details().get(key).cloned();
            let stderr = detail("stderr").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
            let exit = detail("exit").and_then(|v| v.as_i64());
            Some(failed(&stderr, exit, format!("failed: {}", e.message())).with("git", e.to_json()))
        }
    }
}

/// Whether `commit` is an ancestor of `worktree`'s HEAD: exit 0 yes, 1 no, else `git_failed`.
fn contains(worktree: &std::path::Path, commit: &str) -> Result<bool, CpdError> {
    let out = run(worktree, &["merge-base", "--is-ancestor", commit, "HEAD"])?;
    match out.code {
        0 => Ok(true),
        1 => Ok(false),
        code => Err(CpdError::new(ErrorCode::GitFailed, "git merge-base --is-ancestor failed.")
            .with("exit", code)
            .with("stderr", git::tail(&out.stderr, git::TAIL_LINES))),
    }
}

/// `git rebase <onto>` in `worktree`. `Some(files)` on a conflict (after
/// `git rebase --abort`); `None` when it rebased.
fn rebase(worktree: &std::path::Path, onto: &str) -> Result<Option<Vec<String>>, CpdError> {
    let out = run(worktree, &["rebase", onto])?;
    if out.code == 0 {
        return Ok(None);
    }
    let conflicts = git::run::ok(worktree, &["diff", "--name-only", "--diff-filter=U"])?;
    let files: Vec<String> = conflicts.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
    // Best effort when no rebase is in progress; a failure to abort is reported below.
    let abort = run(worktree, &["rebase", "--abort"])?;
    if files.is_empty() {
        return Err(CpdError::new(ErrorCode::GitFailed, format!("git rebase exited {} without a conflict.", out.code))
            .with("exit", out.code)
            .with("stderr", git::tail(&out.stderr, git::TAIL_LINES)));
    }
    if abort.code != 0 {
        return Err(CpdError::new(ErrorCode::GitFailed, "git rebase --abort failed after a conflict.")
            .with("exit", abort.code)
            .with("stderr", git::tail(&abort.stderr, git::TAIL_LINES)));
    }
    Ok(Some(files))
}
