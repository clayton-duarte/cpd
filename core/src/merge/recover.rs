//! R: recovering an interrupted merge (contract §7.4). The ref moves at step
//! 8 but the Ledger records the merge only at step 10; an Attempt in
//! `merging` whose driver is dead is ended only here. R takes no repo lock
//! (hs-d243): it works on one Attempt, under that Attempt's driver lock, and
//! ends it with the compare-and-swap on `status = 'merging'`.

use super::Ctx;
use super::ledger::{self, Attempt, End, Failure, Ladder};
use super::lock::take_free_driver_lock;
use crate::driver::lock;
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

/// R for Attempt `id` of `plan`: takes its driver lock without waiting and
/// re-reads it. `None` when the lock is held (a live driver, or another R,
/// finishes it) or the Attempt is no longer `merging`; else what R did.
/// Callers: engine E2, for each orphaned Attempt (§3.7), and later `pr
/// rebase`'s guard U (§7.6).
pub fn recover_attempt(store: &mut Store, ctx: &Ctx<'_>, plan: &PlanId, id: &AttemptId) -> Result<Option<Recovered>, CpdError> {
    let Some(_driver) = take_free_driver_lock(ctx.home, plan, id)? else { return Ok(None) };
    let attempt = store.read(|conn| ledger::load(conn, plan, id))?;
    if attempt.status != "merging" {
        return Ok(None);
    }
    let end = recover_one(store, ctx, &attempt)?;
    Ok(Some(Recovered { plan: plan.clone(), attempt: id.clone(), end }))
}

/// R1–R4 for one Attempt whose driver is dead. Once the end is committed, the Attempt's driver
/// lock file is removed (G13).
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
            // R2: rule S, its stop reported as step 9's is.
            let stale = git::sync_lead(&attempt.lead_worktree, &attempt.lead_branch).err();
            let end = End::Merged { sha: tip.clone() };
            let metrics = (ctx.metrics)(attempt);
            store.write(ctx.source, |tx| {
                ledger::end_merging(tx, attempt, &end, metrics.as_ref(), &ladder)?;
                if let Some(e) = &stale {
                    ledger::append_error(tx, attempt, ctx.source, e, stale_text(attempt, e))?;
                }
                Ok(())
            })?;
            lock::remove(ctx.home, &attempt.plan, &attempt.id)?;
            // R35: push as step 8 does, by rule P with `tip`; a failure does not undo the merge.
            if let Some(e) = super::push(&attempt.lead_worktree, &attempt.lead_branch, &tip) {
                store.write(ctx.source, |tx| ledger::append_error(tx, attempt, ctx.source, &e, push_text(attempt, &e)))?;
            }
            rest_of_a9(ctx, attempt)?;
            Ok(Some(end))
        }
        // R3: no merge.
        Ok(None) => {
            let cancel = store.read(|conn| ledger::cancel_request(conn, attempt))?;
            let end = match cancel {
                Some(_) => End::Cancelled,
                None => End::Failed {
                    cause: Failure::Crashed,
                    detail: "The driver died during the merge, before the Lead Branch moved.".to_owned(),
                },
            };
            let metrics = (ctx.metrics)(attempt);
            store.write(ctx.source, |tx| ledger::end_merging(tx, attempt, &end, metrics.as_ref(), &ladder))?;
            lock::remove(ctx.home, &attempt.plan, &attempt.id)?;
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
