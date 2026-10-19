//! R: recovering an interrupted merge (contract §7.4). The ref moves at step
//! 8 but the Ledger records the merge only at step 10; an Attempt in
//! `merging` whose driver is dead is ended only here, under the per-repo lock.

use std::path::Path;

use super::Ctx;
use crate::attempt::ledger::{self, Attempt, Cause, End, Ladder, Stage};
use super::lock::RepoLock;
use crate::driver::lock::probe;
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, run::run};
use crate::host::layout::attempt_window;
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, PlanId};
use crate::store::Store;

/// What R did for one orphaned Attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovered {
    pub plan: PlanId,
    pub attempt: AttemptId,
    /// How it ended; `None` when git failed and nothing changed (R4).
    pub end: Option<End>,
}

/// Runs R for every Attempt in `merging` of a Plan on `lock`'s repo whose
/// driver lock is free. Every holder of the per-repo lock calls this right
/// after taking it (`pr open`, `pr rebase`, and E2 after
/// [`RepoLock::try_take`]); the merge calls it itself.
pub fn recover(store: &mut Store, ctx: &Ctx<'_>, lock: &RepoLock) -> Result<Vec<Recovered>, CpdError> {
    recover_except(store, ctx, lock, None)
}

/// [`recover`], skipping the caller's own Attempt (the merge's).
pub(super) fn recover_except(
    store: &mut Store,
    ctx: &Ctx<'_>,
    lock: &RepoLock,
    skip: Option<(&PlanId, &AttemptId)>,
) -> Result<Vec<Recovered>, CpdError> {
    let repo = lock.repo().to_string_lossy().into_owned();
    let candidates = store.read(|conn| ledger::merging_on_repo(conn, &repo))?;
    let mut done = Vec::new();
    for (plan, id) in candidates {
        if skip.is_some_and(|(p, a)| *p == plan && *a == id) {
            continue;
        }
        // Held: its driver is alive and will finish the merge itself.
        let Some(_driver) = probe(ctx.home, &plan, &id)? else { continue };
        let attempt = store.read(|conn| ledger::load(conn, &plan, &id))?;
        // Ended between the query and the lock.
        if attempt.status != "merging" {
            continue;
        }
        let end = recover_one(store, ctx, &attempt)?;
        done.push(Recovered { plan, attempt: id, end });
    }
    Ok(done)
}

/// R1–R4 for one Attempt whose driver is dead.
fn recover_one(store: &mut Store, ctx: &Ctx<'_>, attempt: &Attempt) -> Result<Option<End>, CpdError> {
    let ladder = Ladder { catalog: ctx.catalog, retry_cap: ctx.config.retry_cap };
    let lead_ref = format!("refs/heads/{}", attempt.lead_branch);
    let check = git::run::rev_parse(&attempt.repo, &format!("refs/heads/{}", attempt.branch)).and_then(|tip| {
        let out = run(&attempt.repo, &["merge-base", "--is-ancestor", &tip, &lead_ref])?;
        match out.code {
            0 => Ok(Some(tip)),
            1 => Ok(None),
            code => Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-base --is-ancestor exited {code}."))
                .with("exit", code)
                .with("stderr", git::tail(&out.stderr, git::TAIL_LINES))),
        }
    });
    match check {
        // R2: the merge happened.
        Ok(Some(tip)) => {
            let stale = finish_sync(attempt).err();
            let end = End::Merged { sha: tip };
            let metrics = (ctx.metrics)(attempt);
            store.write(ctx.source, |tx| {
                ledger::end(tx, attempt, &end, metrics.as_ref(), &ladder, Stage::Merge)?;
                if let Some(e) = &stale {
                    ledger::append_error(tx, attempt, ctx.source, e, stale_text(attempt, e))?;
                }
                Ok(())
            })?;
            // R35: push as step 8 does; a failure does not undo the merge.
            if let Err(e) = git::push_lead_branch(&attempt.lead_worktree, &attempt.lead_branch, false) {
                store.write(ctx.source, |tx| ledger::append_error(tx, attempt, ctx.source, &e, push_text(attempt, &e)))?;
            }
            rest_of_a9(ctx, attempt)?;
            Ok(Some(end))
        }
        // R3: no merge.
        Ok(None) => {
            let cancel = store.read(|conn| ledger::cancel_request(conn, &attempt.plan, &attempt.job))?;
            let end = match cancel {
                Some(_) => End::Cancelled,
                None => End::Failed {
                    cause: Cause::Crashed,
                    detail: "The driver died during the merge, before the Lead Branch moved.".to_owned(),
                },
            };
            let metrics = (ctx.metrics)(attempt);
            store.write(ctx.source, |tx| ledger::end(tx, attempt, &end, metrics.as_ref(), &ladder, Stage::Merge))?;
            Ok(Some(end))
        }
        // R4: git failed; nothing changes, reported once per Attempt.
        Err(e) => {
            let error = CpdError::new(
                ErrorCode::MergeCheckFailed,
                format!("Cannot tell whether Attempt {}'s commits are on {}: git failed.", attempt.id, attempt.lead_branch),
            )
            .with("git", e.to_json());
            if !store.read(|conn| ledger::has_error(conn, attempt, ErrorCode::MergeCheckFailed))? {
                let text = format!(
                    "The driver of Attempt {} died during its merge, and cpd cannot tell whether the merge happened: {} It checks again later; nothing changed. See: cpd attempt show {} --plan {}",
                    attempt.id,
                    e.message(),
                    attempt.id,
                    attempt.plan
                );
                store.write(ctx.source, |tx| ledger::append_error(tx, attempt, ctx.source, &error, text))?;
            }
            Ok(None)
        }
    }
}

/// R2's step 9 for a dead driver: the Lead worktree is stale when its index
/// and working tree agree with each other but not with `HEAD`; then it still
/// holds the tree of `old`, which is written out and moved to `HEAD`. Clean:
/// nothing to do. Any other state, or a git failure: `lead_worktree_stale`.
fn finish_sync(attempt: &Attempt) -> Result<(), CpdError> {
    let lead = &attempt.lead_worktree;
    try_finish_sync(lead).map_err(|e| match e.code() {
        ErrorCode::LeadWorktreeStale => e,
        _ => {
            let stderr = e.details().get("stderr").and_then(|v| v.as_str()).unwrap_or(e.message()).to_owned();
            let fix = format!("Check it out at the Lead Branch: git -C {} reset --hard HEAD", lead.display());
            stale_error(lead, &stderr, fix)
        }
    })
}

fn try_finish_sync(lead: &Path) -> Result<(), CpdError> {
    let worktree = run(lead, &["diff", "--quiet"])?.code;
    let index = run(lead, &["diff", "--cached", "--quiet", "HEAD"])?.code;
    match (worktree, index) {
        (0, 0) => Ok(()),
        (0, 1) => {
            let tree = git::run::ok(lead, &["write-tree"])?;
            let out = run(lead, &["read-tree", "-m", "-u", tree.trim(), "HEAD"])?;
            if out.code == 0 {
                return Ok(());
            }
            let fix = format!(
                "Move away the files in the way, then run: git -C {} read-tree -m -u {} HEAD",
                lead.display(),
                tree.trim()
            );
            Err(stale_error(lead, &out.stderr, fix))
        }
        _ => {
            let fix = format!(
                "The Lead worktree has changes of its own: inspect them with git -C {0} status; to keep only the Lead Branch, run: git -C {0} reset --hard HEAD",
                lead.display()
            );
            Err(stale_error(lead, "", fix))
        }
    }
}

/// `lead_worktree_stale`: the merge stands but the Lead worktree was not
/// moved to it (§7.4 step 9).
pub(super) fn stale_error(lead: &Path, stderr: &str, fix: String) -> CpdError {
    CpdError::new(
        ErrorCode::LeadWorktreeStale,
        format!("The Lead Branch moved but the Lead worktree {} was not updated to it.", lead.display()),
    )
    .with("worktree", lead.display().to_string())
    .with("stderr", git::tail(stderr, git::TAIL_LINES))
    .with("fix", fix)
}

/// The `error` Event's text for a stale Lead worktree.
pub(super) fn stale_text(attempt: &Attempt, error: &CpdError) -> String {
    let fix = error.details().get("fix").and_then(|v| v.as_str()).unwrap_or_default();
    format!(
        "Attempt {} is merged, but the Lead worktree {} still holds the previous tree. {fix}. Then: cpd plan show --plan {}",
        attempt.id,
        attempt.lead_worktree.display(),
        attempt.plan
    )
}

/// The `error` Event's text for a failed push of the Lead Branch (R35).
pub(super) fn push_text(attempt: &Attempt, error: &CpdError) -> String {
    format!(
        "Attempt {} is merged, but pushing {} to origin failed: {}. The next merge or cpd pr open pushes it again. See: cpd attempt show {} --plan {}",
        attempt.id,
        attempt.lead_branch,
        error.message(),
        attempt.id,
        attempt.plan
    )
}

/// The rest of A9 after R2: kill the Attempt's tmux window, remove its
/// worktree and branch (§3.4, §7.2).
fn rest_of_a9(ctx: &Ctx<'_>, attempt: &Attempt) -> Result<(), CpdError> {
    let tmux = Tmux::new(ctx.config.tmux.socket.as_str());
    let name = attempt_window(&attempt.plan, &attempt.id);
    let windows: Vec<_> = tmux
        .list_panes()?
        .into_iter()
        .filter(|p| p.session_name == ctx.config.tmux.work_session && p.window_name == name)
        .map(|p| p.window)
        .collect();
    for window in windows {
        tmux.kill_window(&window)?;
    }
    git::remove_merged_attempt(&attempt.lead_worktree, &attempt.worktree, &attempt.branch)
}
