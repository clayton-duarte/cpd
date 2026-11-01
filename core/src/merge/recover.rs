//! R: recovering an interrupted merge (contract §7.4). The ref moves at step
//! 8 but the Ledger records the merge only at step 10; an Attempt in
//! `merging` whose driver is dead is ended only here. R takes no repo lock
//! (hs-d243): it works on one Attempt, under that Attempt's driver lock, and
//! ends it with the compare-and-swap on `status = 'merging'`.

use super::Ctx;
use crate::attempt::ledger::{self, Attempt, Cause, End, Ladder, Stage};
use crate::driver::lock::{self, probe};
use crate::driver::runner;
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, run::run};
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, PlanId};
use crate::ports::Ports;
use crate::store::Store;

/// What R did for one orphaned Attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovered {
    pub plan: PlanId,
    pub attempt: AttemptId,
    /// How it ended; `None` when git failed and nothing changed (R4).
    pub end: Option<End>,
}

/// R for Attempt `id` of `plan`: probes its driver lock without waiting (a
/// probe never creates the file, §3.4) and re-reads it. `None` when the lock
/// is held (a live driver, or another R, finishes it) or the Attempt is no
/// longer `merging`; else what R did. Callers: engine E2, for each orphaned
/// Attempt (§3.7), and `pr rebase`'s guard U (§7.6).
pub fn recover_attempt(
    store: &mut Store,
    ctx: &Ctx<'_>,
    plan: &PlanId,
    id: &AttemptId,
) -> Result<Option<Recovered>, CpdError> {
    let Some(_driver) = probe(ctx.home, plan, id)? else {
        return Ok(None);
    };
    let attempt = store.read(|conn| ledger::load(conn, plan, id))?;
    if attempt.status != "merging" {
        return Ok(None);
    }
    let end = recover_one(store, ctx, &attempt)?;
    Ok(Some(Recovered {
        plan: plan.clone(),
        attempt: id.clone(),
        end,
    }))
}

/// R1–R4 for one Attempt whose driver is dead. Once the end is committed, the Attempt's driver
/// lock file is removed (G13).
fn recover_one(
    store: &mut Store,
    ctx: &Ctx<'_>,
    attempt: &Attempt,
) -> Result<Option<End>, CpdError> {
    let ladder = Ladder {
        catalog: ctx.catalog,
        retry_cap: ctx.config.retry_cap,
    };
    let lead_ref = format!("refs/heads/{}", attempt.lead_branch);
    // A linear Attempt's branch is the Lead Branch (hs-d680, A5): the commit to test is its `submitted_sha`, and
    // R2 records that as `merged_sha`.
    let check = if git::worktree::is_linear(&attempt.branch, &attempt.lead_branch) {
        let submitted = attempt.submitted_sha.clone().ok_or_else(|| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("Attempt {} is linear but has no submitted_sha.", attempt.id),
            )
        })?;
        let out = run(
            &attempt.repo,
            &["merge-base", "--is-ancestor", &submitted, &lead_ref],
        )?;
        match out.code {
            0 => Ok(Some(submitted)),
            1 => Ok(None),
            code => Err(CpdError::new(
                ErrorCode::GitFailed,
                format!("git merge-base --is-ancestor exited {code}."),
            )
            .with("exit", code)
            .with("stderr", git::tail(&out.stderr, git::TAIL_LINES))),
        }
    } else {
        git::run::rev_parse(&attempt.repo, &format!("refs/heads/{}", attempt.branch)).and_then(
            |tip| {
                let out = run(
                    &attempt.repo,
                    &["merge-base", "--is-ancestor", &tip, &lead_ref],
                )?;
                match out.code {
                    0 => Ok(Some(tip)),
                    1 => Ok(None),
                    code => Err(CpdError::new(
                        ErrorCode::GitFailed,
                        format!("git merge-base --is-ancestor exited {code}."),
                    )
                    .with("exit", code)
                    .with("stderr", git::tail(&out.stderr, git::TAIL_LINES))),
                }
            },
        )
    };
    match check {
        // R2: the merge happened.
        Ok(Some(tip)) => {
            // R2: rule S, its stop reported as step 9's is.
            let stale = git::sync_lead(&attempt.lead_worktree, &attempt.lead_branch).err();
            let end = End::Merged { sha: tip.clone() };
            let metrics = (ctx.metrics)(attempt);
            store.write(ctx.source, |tx| {
                ledger::end(tx, attempt, &end, metrics.as_ref(), &ladder, Stage::Merge)?;
                if let Some(e) = &stale {
                    ledger::append_error(
                        tx,
                        attempt,
                        ctx.source,
                        e,
                        stale_text(attempt, e),
                        ctx.config.watch.error_repeat(),
                    )?;
                }
                Ok(())
            })?;
            lock::remove(ctx.home, &attempt.plan, &attempt.id)?;
            // R35: push as step 8 does, by rule P with `tip`; a failure does not undo the merge.
            if let Some(e) = super::publish(attempt, &tip) {
                store.write(ctx.source, |tx| {
                    ledger::append_error(
                        tx,
                        attempt,
                        ctx.source,
                        &e,
                        push_text(attempt, &e),
                        ctx.config.watch.error_repeat(),
                    )
                })?;
            }
            rest_of_a9(store, ctx, attempt)?;
            Ok(Some(end))
        }
        // R3: no merge.
        Ok(None) => {
            let cancel =
                store.read(|conn| ledger::cancel_request(conn, &attempt.plan, &attempt.job))?;
            let end = match cancel {
                Some(_) => End::Cancelled,
                None => End::Failed {
                    cause: Cause::Crashed,
                    detail: "The driver died during the merge, before the Lead Branch moved."
                        .to_owned(),
                },
            };
            let metrics = (ctx.metrics)(attempt);
            store.write(ctx.source, |tx| {
                ledger::end(tx, attempt, &end, metrics.as_ref(), &ladder, Stage::Merge)
            })?;
            // A9: the Attempt is ended, its worktree is kept, its window goes; the driver lock file goes
            // too, whether or not the kill worked (each item best effort).
            let killed = kill_worker(store, ctx, attempt);
            lock::remove(ctx.home, &attempt.plan, &attempt.id)?;
            killed?;
            Ok(Some(end))
        }
        // R4: git failed; nothing changes, reported once per Attempt.
        Err(e) => {
            let error = CpdError::new(
                ErrorCode::MergeCheckFailed,
                format!(
                    "Cannot tell whether Attempt {}'s commits are on {}: git failed.",
                    attempt.id, attempt.lead_branch
                ),
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
                store.write(ctx.source, |tx| {
                    ledger::append_error(
                        tx,
                        attempt,
                        ctx.source,
                        &error,
                        text,
                        ctx.config.watch.error_repeat(),
                    )
                })?;
            }
            Ok(None)
        }
    }
}

/// The `error` Event's text for a stale Lead worktree.
pub(super) fn stale_text(attempt: &Attempt, error: &CpdError) -> String {
    let fix = error
        .details()
        .get("fix")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
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
fn rest_of_a9(store: &mut Store, ctx: &Ctx<'_>, attempt: &Attempt) -> Result<(), CpdError> {
    // Best effort per item: a window that cannot be killed does not keep the worktree.
    let killed = kill_worker(store, ctx, attempt);
    // A linear Attempt is the workflow worktree and the Lead Branch: keep both (hs-d680).
    if git::worktree::is_linear(&attempt.branch, &attempt.lead_branch) {
        return killed;
    }
    let removed =
        git::remove_merged_attempt(&attempt.lead_worktree, &attempt.worktree, &attempt.branch);
    killed.and(removed)
}

/// A9's stop of the Attempt's Worker, by its stored runner (the seam's [`runner::kill_orphan`]: R's driver is dead).
fn kill_worker(store: &mut Store, ctx: &Ctx<'_>, attempt: &Attempt) -> Result<(), CpdError> {
    let tmux = Tmux::new(ctx.config.tmux.socket.as_str());
    runner::kill_orphan(
        &Ports::current(),
        ctx.config,
        ctx.home,
        &tmux,
        store,
        &attempt.plan,
        &attempt.id,
    )
}
