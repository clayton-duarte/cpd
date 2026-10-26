//! The merge (contract §7.4): driver step A8, and the recovery R of an
//! interrupted merge, both without a repo lock (hs-d243). No command of its
//! own: the driver calls [`merge`] at A8; engine E2 calls [`recover_attempt`]
//! for each orphaned Attempt.
//!
//! The Gates run here (contract §7.4 step 6, hs-d679): `gate.merge` (the repo's
//! `pre-commit`) at every non-review Job merge, and `gate.pr` (the repo's
//! `pre-merge`) at an `open` or `update` Job merge, in the Attempt worktree at
//! HEAD, while the Attempt is still `merging` (the tree shows it yellow).

mod recover;

#[cfg(test)]
mod linear_tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::attempt::ledger::{self, Stage};
use crate::config::{Catalog, Config};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, run::run, HookOutcome, Pipeline};
use crate::home::{CpdHome, Hook};
use crate::policy::Gate;
use crate::ids::{AttemptId, PlanId};
use crate::ports::Ports;
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
    let Stepped { end, stale, push, pulled } = if review { review_steps(store, &target)? } else { steps(store, ctx.home, ctx.config, &target)? };
    let metrics = (ctx.metrics)(&target);
    let ladder = Ladder { catalog: ctx.catalog, retry_cap: ctx.config.retry_cap };
    let pr = crate::ids::PrKey::parse(&target.pr).map_err(|_| CpdError::bad_row(format!("Job {} has the PR key {:?}", target.job, target.pr)))?;
    // Step 10, and every other end. An `open` Job's merge writes the PR's rows in the same transaction (§7.7).
    store.write(ctx.source, |tx| {
        let pull = pulled.as_ref().map(|p| ledger::Pull { number: p.number(), url: p.url() });
        ledger::end_opened(tx, &target, &end, metrics.as_ref(), &ladder, Stage::Merge, pull)?;
        if let Some(pulled) = &pulled {
            crate::pr::record_pull(tx, plan, &pr, pulled)?;
        }
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

/// What [`steps`] returns: how the Attempt ends, step 9's `lead_worktree_stale` error, step 8's `push_failed` error (a Job that is not an
/// `open` or `update` Job stays merged with it), and the pull request an `open` Job's merge created or adopted (step 8b), whose rows step 10
/// writes.
struct Stepped {
    end: End,
    stale: Option<CpdError>,
    push: Option<CpdError>,
    pulled: Option<crate::pr::Pulled>,
}

impl Stepped {
    fn ends(end: End) -> Stepped {
        Stepped { end, stale: None, push: None, pulled: None }
    }
}

/// An `open` or `update` Job's merge differs from the others' (§7.4): no rebase, and the push is part of the Job.
fn is_cpd_kind(target: &Attempt) -> bool {
    matches!(target.kind.as_str(), "open" | "update")
}

/// A review Job's merge (B2, §7.4): step 1 (cancel check), step 2 (snapshot `old`)
/// and step 7 (cancel check again); no tamper check, rebase, fast-forward, push
/// or worktree sync, because the Attempt has no commits and the Lead Branch does
/// not move; then the Attempt ends `merged` with `merged_sha = old`.
fn review_steps(store: &mut Store, target: &Attempt) -> Result<Stepped, CpdError> {
    if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
        return Ok(Stepped::ends(End::Cancelled));
    }
    let old = git::run::rev_parse(&target.repo, &format!("refs/heads/{}", target.lead_branch))?;
    if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
        return Ok(Stepped::ends(End::Cancelled));
    }
    Ok(Stepped::ends(End::Merged { sha: old }))
}

/// Steps 1–9, with step 8's push; for an `open` or `update` Job the steps of §7.4 "An `open` or `update` Job's merge".
fn steps(store: &mut Store, home: &CpdHome, config: &Config, target: &Attempt) -> Result<Stepped, CpdError> {
    let repo = &target.repo;
    let lead = &target.lead_worktree;
    let lead_ref = format!("refs/heads/{}", target.lead_branch);
    let cpd_kind = is_cpd_kind(target);
    // A linear Attempt's branch is the PR's Lead Branch (hs-d680, A5): its merge skips the rebase and fast-forward.
    let linear = git::worktree::is_linear(&target.branch, &target.lead_branch);
    // 0. Branch check (engine-W1): the Attempt branch is still what `attempt submit` checked. A linear Attempt's
    // branch is the Lead Branch; only a rewrite that drops the submitted commit is tampering.
    let submitted = target.submitted_sha.as_deref().ok_or_else(|| {
        CpdError::new(ErrorCode::StoreFailed, format!("Attempt {} is merging with no submitted_sha.", target.id))
    })?;
    if linear {
        let out = run(repo, &["merge-base", "--is-ancestor", submitted, &lead_ref])?;
        match out.code {
            0 => {}
            1 => {
                let detail = format!("the Lead Branch {} no longer contains the submitted commit ({submitted})", target.lead_branch);
                return Ok(Stepped::ends(End::Failed { cause: Failure::Tampered, detail }));
            }
            code => {
                return Err(CpdError::new(ErrorCode::GitFailed, "git merge-base --is-ancestor failed.")
                    .with("exit", code)
                    .with("stderr", git::tail(&out.stderr, git::TAIL_LINES)));
            }
        }
    } else {
        let tip = git::run::rev_parse(repo, &format!("refs/heads/{}", target.branch))?;
        if tip != submitted {
            let detail = format!("the Attempt branch moved after submit ({submitted} -> {tip})");
            return Ok(Stepped::ends(End::Failed { cause: Failure::Tampered, detail }));
        }
    }
    // A linear Attempt's `base_sha` is the base of its tamper diff and red-test guard: `old...branch` is empty
    // because `old` is already the Lead tip. Read here; absent is a corrupt row.
    let base_sha = if linear {
        store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT base_sha FROM attempts WHERE plan = ?1 AND id = ?2",
                    [target.plan.as_str(), &target.id.to_string()],
                    |r| r.get::<_, Option<String>>(0),
                )?)
            })?
            .ok_or_else(|| CpdError::new(ErrorCode::StoreFailed, format!("Attempt {} is linear but has no base_sha.", target.id)))?
    } else {
        String::new()
    };
    let mut retries = 0;
    loop {
        // 1. Cancel check.
        if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            return Ok(Stepped::ends(End::Cancelled));
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
        let policy = store.read(|conn| crate::pr::policy_of_job(conn, home, &target.plan, target.job.as_str()))?;
        // A linear Attempt diffs `base_sha...submitted_sha`; every other Attempt `old...branch`.
        let (lead_compare, diff_base, diff_head): (&str, &str, &str) =
            if linear { (base_sha.as_str(), base_sha.as_str(), submitted) } else { (&old, &old, &target.branch) };
        let touched = git::tamper::touched_protected_over(repo, lead_compare, &policy.protected.paths, &base_refs, diff_base, diff_head)?;
        if !touched.is_empty() {
            let detail = format!("The diff touches protected paths: {}.", touched.join(", "));
            return Ok(Stepped::ends(End::Failed { cause: Failure::Tampered, detail }));
        }
        // The red tests (§7.5, §7.7): an `impl` Job's diff does not change a file a `test` Job it needs guards, unless the Lead approved
        // the path (read now, so an approval reaches an Attempt already merging).
        if target.kind == "impl" {
            let (all, job) = store.read(|conn| {
                let all = crate::plan::jobs::load(conn, &target.plan)?;
                let job = crate::job::find(&all, &target.plan, &target.job)?.clone();
                Ok((all, job))
            })?;
            let guard = crate::job::guard::of(&all, &job);
            let changed = git::tamper::touched_guarded(repo, lead_compare, diff_base, diff_head, &guard.files, &guard.approved)?;
            if !changed.is_empty() {
                let detail = format!("The diff changes red tests of {}: {}.", guard.jobs.join(", "), changed.join(", "));
                return Ok(Stepped::ends(End::Failed { cause: Failure::Tampered, detail }));
            }
        }
        // 5. Rebase in the Attempt worktree, never in the Lead worktree; skipped when the
        // Attempt already contains `old` (hs-d139), so a merge of the base survives. An `open` or
        // `update` Job never rebases: the Gate ran on the submitted HEAD, and a rebase would drop its
        // merge commit and replay the base's commits, which then conflict.
        // A linear Attempt is already the Lead Branch tip: it is contained by definition and never rebases (hs-d680).
        let contained = if linear { true } else { contains(&target.worktree, &old)? };
        if cpd_kind && !contained {
            let detail = format!(
                "lead_branch_moved: {} moved since the submit ({old} is not in the Attempt's HEAD), and an {} Job is never rebased: the next Attempt starts from the newer tip.",
                target.lead_branch, target.kind
            );
            return Ok(Stepped::ends(End::Failed { cause: Failure::MergeConflict, detail }));
        }
        if !contained {
            if let Some(files) = rebase(&target.worktree, &old)? {
                let detail = format!("The rebase onto the Lead Branch conflicted in: {}.", files.join(", "));
                return Ok(Stepped::ends(End::Failed { cause: Failure::MergeConflict, detail }));
            }
        }
        // 6. The Gates (hs-d679), while the Attempt is still `merging`. An `open` or `update` Job's base check
        // runs first, so nothing is pushed or opened when it refuses.
        if cpd_kind {
            let (pr, _, _) = pr_of_job(store, target)?;
            let again = format!("cpd attempt submit {}", target.id);
            match store.read(|conn| crate::pr::base_check(conn, &target.plan, repo, &target.worktree, &pr, submitted, &again)) {
                Ok(()) => {}
                Err(e) if e.code() == ErrorCode::BaseConflict => {
                    return Ok(Stepped::ends(End::Failed { cause: Failure::MergeConflict, detail: e.message().to_owned() }));
                }
                Err(e) => return Err(e),
            }
        }
        if policy.gate.merge == Gate::Local {
            if let Some(end) = run_gate(home, config, target, Hook::PreCommit, &policy.hooks.pre_commit)? {
                return Ok(Stepped::ends(end));
            }
        }
        if cpd_kind && policy.gate.pr == Gate::Local {
            if let Some(end) = run_gate(home, config, target, Hook::PreMerge, &policy.hooks.pre_merge)? {
                return Ok(Stepped::ends(end));
            }
        }
        // An `open` Job whose pull request was merged on GitHub and never recorded adopts it (§7.7 submit step 1, §7.4 step 8b): it has no
        // new commits to push, so steps 5 and 8 are skipped and step 10 writes the rows of `pr open` step 0.
        if target.kind == "open" && pull_request_is_merged(store, target)? {
            let pulled = open_pull_request(store, target)?;
            return Ok(Stepped { end: End::Merged { sha: old }, stale: None, push: None, pulled: Some(pulled) });
        }
        // 7. Cancel check again: the last safe point.
        if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            return Ok(Stepped::ends(End::Cancelled));
        }
        // 8. Fast-forward with a compare-and-swap. A linear Attempt is already on the Lead Branch: no rebase and no
        // update-ref; rule P's push and rule S still run, and step 10 records `submitted_sha` as `merged_sha` (hs-d680).
        if linear {
            let new = git::run::rev_parse(&target.worktree, "HEAD")?;
            let push = push(lead, &target.lead_branch, &new);
            let stale = git::sync_lead(lead, &target.lead_branch).err();
            return finish_merge(store, target, submitted.to_owned(), cpd_kind, push, stale);
        }
        let new = git::run::rev_parse(&target.worktree, "HEAD")?;
        git::run::ok(repo, &["merge-base", "--is-ancestor", &old, &new])?;
        let message = format!("cpd: merge {}/{}", target.plan, target.id);
        let out = run(repo, &["update-ref", "-m", &message, &lead_ref, &new, &old])?;
        if out.code == 0 {
            // Push (R35) by rule P: the merge stands whatever happens here.
            let push = publish(target, &new);
            // 9. Rule S on the Lead worktree; the merge stands whatever happens here.
            let stale = git::sync_lead(lead, &target.lead_branch).err();
            return finish_merge(store, target, new, cpd_kind, push, stale);
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
            return Ok(Stepped::ends(End::Failed { cause: Failure::CasLost, detail }));
        }
    }
}

/// Step 8's tail once the Lead Branch holds the merge: rule P (`push`) and rule S (`stale`) have run. A cpd kind
/// (`open`/`update`) Job fails `crashed` on a failed push and opens its pull request (step 8b); every other Job
/// merges. `sha` is recorded as `merged_sha`.
fn finish_merge(
    store: &mut Store,
    target: &Attempt,
    sha: String,
    cpd_kind: bool,
    push: Option<CpdError>,
    stale: Option<CpdError>,
) -> Result<Stepped, CpdError> {
    if !cpd_kind {
        return Ok(Stepped { end: End::Merged { sha }, stale, push, pulled: None });
    }
    // An `open` or `update` Job's push is part of the Job: one that fails ends the Attempt `failed` / `crashed` after the
    // fast-forward (the next Attempt finds the Lead Branch already holding the merge, and its submit runs the Gate again).
    if let Some(e) = push {
        let detail = format!("push_failed: {}", e.message());
        return Ok(Stepped { end: End::Failed { cause: Failure::Crashed, detail }, stale, push: None, pulled: None });
    }
    // 8b. An `open` Job's pull request: created or adopted, never a draft; a `gh` failure ends the Attempt `crashed`.
    if target.kind == "open" {
        match open_pull_request(store, target) {
            Ok(pulled) => return Ok(Stepped { end: End::Merged { sha }, stale, push: None, pulled: Some(pulled) }),
            Err(e) if matches!(e.code(), ErrorCode::GhFailed | ErrorCode::ToolTimeout | ErrorCode::PrClosed) => {
                let detail = format!("{}: {}", if e.code() == ErrorCode::PrClosed { "pr_closed" } else { "gh_failed" }, e.message());
                return Ok(Stepped { end: End::Failed { cause: Failure::Crashed, detail }, stale, push: None, pulled: None });
            }
            Err(e) => return Err(e),
        }
    }
    Ok(Stepped { end: End::Merged { sha }, stale, push: None, pulled: None })
}

/// The three shapes of the `detail` an Attempt ends with after the Gate (§7.4 step 6): [`gate_detail`] writes them,
/// [`parse_gate_detail`] reads them back, so the writer and the reader are one module.
#[derive(Clone, Copy)]
enum GateEnd<'a> {
    /// The Gate exited non-zero (`gate_failed`).
    Failed { exit: i32, tail: &'a str },
    /// The Gate was killed or timed out (`gate_error`).
    Unjudged { why: &'a str, tail: &'a str },
    /// The Gate could not start (`gate_error`).
    CouldNotRun { message: &'a str },
}

const FAILED: &str = " failed (exit ";
const UNJUDGED: &str = " could not be judged (";
const COULD_NOT_RUN: &str = " could not run: ";
const LOG: &str = ". log: ";
const TAIL: &str = "\ntail:\n";

fn gate_detail(hook: Hook, log: &Path, end: GateEnd<'_>) -> String {
    let (hook, log) = (hook.as_str(), log.display());
    match end {
        GateEnd::Failed { exit, tail } => format!("{hook}{FAILED}{exit}){LOG}{log}{TAIL}{tail}"),
        GateEnd::Unjudged { why, tail } => format!("{hook}{UNJUDGED}{why}){LOG}{log}{TAIL}{tail}"),
        GateEnd::CouldNotRun { message } => format!("{hook}{COULD_NOT_RUN}{message}{LOG}{log}"),
    }
}

/// The hook and the log of the Gate a `detail` written by [`gate_detail`] names; `None` for a detail of another shape.
pub fn parse_gate_detail(detail: &str) -> Option<(Hook, PathBuf)> {
    let (hook, rest) = [Hook::PreCommit, Hook::PreMerge].into_iter().find_map(|h| Some((h, detail.strip_prefix(h.as_str())?)))?;
    let log = if let Some(rest) = rest.strip_prefix(FAILED) {
        let (exit, after) = rest.split_once(')')?;
        exit.parse::<i32>().ok()?;
        let (after, _) = after.split_once(TAIL)?;
        after.strip_prefix(LOG)?
    } else if let Some(rest) = rest.strip_prefix(UNJUDGED) {
        let (head, _) = rest.split_once(TAIL)?;
        &head[head.rfind(&format!("){LOG}"))? + 1 + LOG.len()..]
    } else if let Some(rest) = rest.strip_prefix(COULD_NOT_RUN) {
        &rest[rest.rfind(LOG)? + LOG.len()..]
    } else {
        return None;
    };
    (!log.is_empty() && !log.contains('\n')).then(|| (hook, PathBuf::from(log)))
}

/// Runs the Gate `hook` for `target` in its Attempt worktree at HEAD (§7.4 step 6, hs-d679). `None` when it passed or has no
/// commands; `Some(end)` when it failed (`gate_failed`) or could not be judged (`gate_error`), carrying the hook tail and log.
fn run_gate(home: &CpdHome, config: &Config, target: &Attempt, hook: Hook, pipeline: &Pipeline) -> Result<Option<End>, CpdError> {
    if pipeline.is_empty() {
        return Ok(None);
    }
    let log = home.hook_log(&target.plan, &target.id, hook);
    let timeout = Duration::from_secs(u64::from(config.timeouts.gate_minutes) * 60);
    let operation = git::Operation { branch: &target.lead_branch };
    match git::run_hook(hook, pipeline, &target.worktree, Some(&operation), timeout, &log) {
        Ok(result) => {
            let tail = result.tail.unwrap_or_default();
            match result.outcome {
                HookOutcome::Passed => Ok(None),
                HookOutcome::Failed { exit } => Ok(Some(End::Failed {
                    cause: Failure::GateFailed,
                    detail: gate_detail(hook, &log, GateEnd::Failed { exit, tail: &tail }),
                })),
                HookOutcome::Error(why) => Ok(Some(End::Failed {
                    cause: Failure::GateError,
                    detail: gate_detail(hook, &log, GateEnd::Unjudged { why: &why.to_string(), tail: &tail }),
                })),
            }
        }
        Err(e) => Ok(Some(End::Failed {
            cause: Failure::GateError,
            detail: gate_detail(hook, &log, GateEnd::CouldNotRun { message: e.message() }),
        })),
    }
}

/// The PR of the Attempt's Job, its stored title and the Jobs of the PR the pull request's body lists (§7.6 `pr open` step 3), read once.
fn pr_of_job(store: &mut Store, target: &Attempt) -> Result<(crate::plan::PrRow, String, Vec<crate::plan::jobs::Job>), CpdError> {
    store.read(|conn| {
        let prs = crate::plan::fingerprint::load_prs(conn, &target.plan)?;
        let pr = prs
            .into_iter()
            .find(|p| p.key.as_str() == target.pr)
            .ok_or_else(|| CpdError::bad_row(format!("Plan {} has no PR {}", target.plan, target.pr)))?;
        let title = pr.title.clone().ok_or_else(|| {
            CpdError::new(
                ErrorCode::PrTitleRequired,
                format!("PR {} of Plan {} has no title: run cpd pr edit {} --title \"<text>\" (1-72 characters).", pr.key, target.plan, pr.key),
            )
            .with("plan", target.plan.as_str())
            .with("pr", pr.key.as_str())
        })?;
        let jobs = crate::plan::jobs::load(conn, &target.plan)?
            .into_iter()
            .filter(|j| j.pr == target.pr && !matches!(j.kind.as_str(), "open" | "update"))
            .collect();
        Ok((pr, title, jobs))
    })
}

/// Whether the newest pull request of the PR's Lead Branch is already merged on GitHub: the `open` Job adopts it (§7.7 submit step 1).
fn pull_request_is_merged(store: &mut Store, target: &Attempt) -> Result<bool, CpdError> {
    let (pr, _, _) = pr_of_job(store, target)?;
    crate::pr::is_merged_on_github(&target.lead_worktree, pr.remote_branch())
}

/// Step 8b (§7.4): the pull request of the Lead Branch, created or adopted, as `pr open` steps 0 and 3 do it (never `--draft`).
fn open_pull_request(store: &mut Store, target: &Attempt) -> Result<crate::pr::Pulled, CpdError> {
    let (pr, title, jobs) = pr_of_job(store, target)?;
    crate::pr::pull_request(&target.lead_worktree, &pr, &title, &jobs)
}

/// Step 8's and R2's push of `sha`: to the PR's remote branch, and skipped for an adopted PR unless the Job is an `open` or
/// `update` one, so that a work Job's commit never reaches a live pull request (§7.4).
pub(super) fn publish(target: &Attempt, sha: &str) -> Option<CpdError> {
    if target.adopted && !is_cpd_kind(target) {
        return None;
    }
    push(&target.lead_worktree, &target.remote_branch, sha)
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
    rebase_with(&*Ports::current().runner, worktree, onto)
}

/// [`rebase`] on `runner` (a test passes a `FakeRunner`).
pub(crate) fn rebase_with(runner: &dyn crate::host::runner::Runner, worktree: &std::path::Path, onto: &str) -> Result<Option<Vec<String>>, CpdError> {
    let out = git::run::run_with(runner, worktree, &["rebase", onto])?;
    if out.code == 0 {
        return Ok(None);
    }
    let conflicts = git::run::ok_with(runner, worktree, &["diff", "--name-only", "--diff-filter=U"])?;
    let files: Vec<String> = conflicts.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
    // Best effort when no rebase is in progress; a failure to abort is reported below.
    let abort = git::run::run_with(runner, worktree, &["rebase", "--abort"])?;
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

#[cfg(test)]
mod gate_detail_tests {
    use super::*;

    #[test]
    fn the_three_forms_round_trip() {
        let log = Path::new("/h/run/pay/logs/api-a1.pre-commit.log");
        let forms = [
            GateEnd::Failed { exit: 3, tail: "red\nmore" },
            GateEnd::Unjudged { why: "killed. log: x (really)", tail: "t" },
            GateEnd::CouldNotRun { message: "no such file. log: y" },
        ];
        for end in forms {
            for hook in [Hook::PreCommit, Hook::PreMerge] {
                let detail = gate_detail(hook, log, end);
                assert_eq!(parse_gate_detail(&detail), Some((hook, log.to_path_buf())), "{detail}");
            }
        }
    }

    #[test]
    fn another_shape_is_not_a_gate_detail() {
        for detail in [
            "",
            "The rebase onto the Lead Branch conflicted in: a.txt.",
            "pre-start failed (exit 1). log: /l\ntail:\nx",
            "pre-commit failed (exit x). log: /l\ntail:\nx",
            "pre-commit failed (exit 1). log: \ntail:\nx",
            "pre-commit failed (exit 1). log: /l",
            "pre-commit exploded. log: /l",
        ] {
            assert_eq!(parse_gate_detail(detail), None, "{detail:?}");
        }
    }
}
