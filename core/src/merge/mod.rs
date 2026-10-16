//! The merge (contract §7.4): driver step A8 under the per-repo lock, and the
//! recovery R of an interrupted merge. No command of its own: the driver
//! calls [`merge`] at A8; engine E2, `pr open` and `pr rebase` take the
//! per-repo lock ([`RepoLock`]) and call [`recover`].
//!
//! No Gate runs here (R19): a merge is checked by the quick check at submit,
//! the tamper check (step 4) and a clean rebase (step 5).

mod ledger;
mod lock;
mod recover;

use serde_json::Value;

use crate::config::{Catalog, Config};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, run::run};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::store::Store;

pub use ledger::{Attempt, End, Failure, Ladder, end_merging, load};
pub use lock::{DriverLock, RepoLock, take_free_driver_lock};
pub use recover::{Recovered, recover};

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
    /// What R did for other Attempts first.
    pub recovered: Vec<Recovered>,
}

/// Driver step A8 (§7.4) for Attempt `attempt` of `plan`, which must be
/// `merging`: takes the per-repo lock (waiting without a limit), runs R for
/// the repo's orphaned merges, then steps 1–10, and ends the Attempt in one
/// transaction (§3.4) before releasing the lock.
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
    let lock = RepoLock::wait(ctx.home, &target.repo)?;
    let recovered = recover::recover_except(store, ctx, &lock, Some((plan, attempt)))?;
    let review = store.read(|conn| crate::job::find(&crate::plan::jobs::load(conn, plan)?, plan, &target.job).map(|j| j.review))?;
    let (end, stale, push) = if review { review_steps(store, &target)? } else { steps(store, &target)? };
    let metrics = (ctx.metrics)(&target);
    let ladder = Ladder { catalog: ctx.catalog, retry_cap: ctx.config.retry_cap };
    // Step 10, and every other end, while the lock is held.
    store.write(ctx.source, |tx| {
        ledger::end_merging(tx, &target, &end, metrics.as_ref(), &ladder)?;
        if let Some(e) = &stale {
            ledger::append_error(tx, &target, ctx.source, e, recover::stale_text(&target, e))?;
        }
        if let Some(e) = &push {
            ledger::append_error(tx, &target, ctx.source, e, recover::push_text(&target, e))?;
        }
        Ok(())
    })?;
    drop(lock);
    Ok(Merge { end, recovered })
}

/// What [`steps`] returns: how the Attempt ends, step 9's `lead_worktree_stale`
/// error and step 8's `push_failed` error, when they happened.
type Stepped = (End, Option<CpdError>, Option<CpdError>);

/// A review Job's merge (B2, §7.4): step 1 (cancel check), step 2 (snapshot `old`)
/// and step 7 (cancel check again); no tamper check, rebase, fast-forward, push
/// or worktree sync, because the Attempt has no commits and the Lead Branch does
/// not move; then the Attempt ends `merged` with `merged_sha = old`.
fn review_steps(store: &mut Store, target: &Attempt) -> Result<Stepped, CpdError> {
    if store.read(|conn| ledger::cancel_request(conn, target))?.is_some() {
        return Ok((End::Cancelled, None, None));
    }
    let old = git::run::rev_parse(&target.repo, &format!("refs/heads/{}", target.lead_branch))?;
    if store.read(|conn| ledger::cancel_request(conn, target))?.is_some() {
        return Ok((End::Cancelled, None, None));
    }
    Ok((End::Merged { sha: old }, None, None))
}

/// Steps 1–9, with step 8's push.
fn steps(store: &mut Store, target: &Attempt) -> Result<Stepped, CpdError> {
    let repo = &target.repo;
    let lead = &target.lead_worktree;
    let lead_ref = format!("refs/heads/{}", target.lead_branch);
    let mut retries = 0;
    loop {
        // 1. Cancel check.
        if store.read(|conn| ledger::cancel_request(conn, target))?.is_some() {
            return Ok((End::Cancelled, None, None));
        }
        // 2. Snapshot.
        let old = git::run::rev_parse(repo, &lead_ref)?;
        // 3. Lead worktree clean, before any work in the Attempt worktree.
        let status = git::run::ok(lead, &["status", "--porcelain"])?;
        if !status.trim().is_empty() {
            let detail = format!(
                "The Lead worktree {} has uncommitted changes: {}",
                lead.display(),
                status.lines().take(10).map(str::trim).collect::<Vec<_>>().join(", ")
            );
            return Ok((End::Failed { cause: Failure::DirtyTarget, detail }, None, None));
        }
        // 4. Tamper check of old...<attempt branch>.
        let touched = git::touched_protected(repo, &old, &old, &target.branch)?;
        if !touched.is_empty() {
            let detail = format!("The diff touches protected paths: {}.", touched.join(", "));
            return Ok((End::Failed { cause: Failure::Tampered, detail }, None, None));
        }
        // 5. Rebase in the Attempt worktree, never in the Lead worktree.
        if let Some(files) = rebase(&target.worktree, &old)? {
            let detail = format!("The rebase onto the Lead Branch conflicted in: {}.", files.join(", "));
            return Ok((End::Failed { cause: Failure::MergeConflict, detail }, None, None));
        }
        // 6. Removed by R19: no Gate at the merge.
        // 7. Cancel check again: the last safe point.
        if store.read(|conn| ledger::cancel_request(conn, target))?.is_some() {
            return Ok((End::Cancelled, None, None));
        }
        // 8. Fast-forward with a compare-and-swap.
        let new = git::run::rev_parse(&target.worktree, "HEAD")?;
        git::run::ok(repo, &["merge-base", "--is-ancestor", &old, &new])?;
        let message = format!("cpd: merge {}/{}", target.plan, target.id);
        let out = run(repo, &["update-ref", "-m", &message, &lead_ref, &new, &old])?;
        if out.code == 0 {
            // Push (R35): the merge stands whatever happens here.
            let push = git::push_lead_branch(lead, &target.lead_branch, false).err();
            // 9. Sync the Lead worktree; the merge stands whatever happens here.
            let sync = run(lead, &["read-tree", "-m", "-u", &old, &new])?;
            let stale = (sync.code != 0).then(|| {
                let fix = format!(
                    "Move away the files in the way, then run: git -C {} read-tree -m -u {old} {new}",
                    lead.display()
                );
                recover::stale_error(lead, &sync.stderr, fix)
            });
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
                "{} kept moving outside cpd during the merge; the fast-forward lost {} times.",
                target.lead_branch,
                CAS_RETRIES + 1
            );
            return Ok((End::Failed { cause: Failure::CasLost, detail }, None, None));
        }
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
