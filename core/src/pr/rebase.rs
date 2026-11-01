//! `pr rebase` (commands.md §9.3, contract §7.6): the Lead Branch onto the base
//! in a temporary rebase worktree, guard U, a compare-and-swap of the Lead Branch,
//! rule S, the Gate on the rebased commit, and the push with a lease. No lock
//! (hs-d243): merges into the PR go on meanwhile.
//!
//! A conflict is not abandoned (hs-d289): the rebase stays in progress in the Lead
//! worktree, and `--continue` (the rest of the rebase, the Gate, the push) or
//! `--abort` ends it.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use serde_json::json;

use super::row::PlanPrs;
use crate::plan::PrRow;
use super::open::{agent, hold_refused, require_jobs_done};
use super::{Args, Env, RebaseArgs, gate, gate_in, illegal_phase, pr_policy, short};
use crate::config::{self, Catalog};
use crate::policy::Policy;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::{self, Push, RebaseWorktree, TAIL_LINES, tail};
use crate::host::runner::Runner;
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::merge::{self, CAS_RETRIES};
use crate::output::Output;
use crate::plan::Phase;
use crate::ports::Ports;

const COMMAND: &str = "pr rebase";

/// How long guard U waits for a live driver's step 10 (§7.6 step 5).
const GUARD_WAIT: Duration = Duration::from_secs(60);

/// How often guard U reads the store while it waits.
const GUARD_POLL: Duration = Duration::from_millis(200);

/// git's options for a rebase that never opens an editor (`GIT_EDITOR=true`, contract §7.6;
/// `run` has no environment to set, and the config does the same).
const NO_EDITOR: [&str; 4] = ["-c", "core.editor=true", "-c", "sequence.editor=true"];

pub(super) fn rebase(args: RebaseArgs) -> Result<Output, CpdError> {
    match (args.resume, args.abort) {
        (true, _) => resume(args.args),
        (_, true) => abort(args.args),
        _ => onto_base(args.args),
    }
}

fn onto_base(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let pr = require_open(&mut env, &plan, &args)?.clone();
    let key = pr.key.clone();
    env.store.read(|conn| hold_refused(conn, &id, &key, agent()))?;
    env.store.read(|conn| refuse_running(conn, &id, &key, "do the rebase"))?;
    let policy = pr_policy(&mut env, &id, &key)?;
    let lead = pr.lead(&id)?.clone();
    let repo = std::path::Path::new(&pr.repo);
    let branch = pr.lead_branch.as_str();
    let onto = onto_of(&pr);

    // 1. The base, fresh (the ref lock on `origin/<base>` is waited for, §7); for an adopted PR
    // the remote head, which the catch-up rebases onto (pro-d15).
    git::run::fetch(repo, &["origin", onto_branch(&pr)])?;
    // The base is fetched too, for the update Job's merge and the Gate (the object store and `origin/<base>` stay fresh).
    if pr.head_ref.is_some() {
        git::run::fetch(repo, &["origin", &pr.base])?;
    }
    // A rebase stopped by an earlier run: that one is finished or given up first.
    if in_progress(&lead)? {
        return Err(in_progress_error(&id, &key));
    }
    // An adopted PR that already holds origin/<head_ref>: nothing to catch up.
    if pr.head_ref.is_some() && git::run::run(repo, &["merge-base", "--is-ancestor", &onto, &format!("refs/heads/{branch}")])?.code == 0 {
        return Ok(Output::success(json!({"plan": id, "pr": key, "already": true})));
    }

    let mut retries = 0;
    let (old, new, lease, worktree) = loop {
        // 2. The snapshot S.
        let old = git::run::rev_parse(repo, &format!("refs/heads/{branch}"))?;
        // 3. A conflict stops the rebase in the Lead worktree, before anything else changes.
        if let Some(files) = git::merge_conflicts(repo, &onto, &old)? {
            return rebase_in_lead(&mut env, &args, &id, &pr, &policy, files);
        }
        // 4. A temporary rebase worktree at S, rebased there: R'.
        let worktree = RebaseWorktree::add(repo, &id, &key, &old)?;
        let new = match rebase_onto(worktree.path(), &onto) {
            Ok(None) => match git::run::rev_parse(worktree.path(), "HEAD") {
                Ok(new) => new,
                Err(e) => return Err(removing(worktree, e)),
            },
            Ok(Some(files)) => {
                worktree.remove()?;
                return rebase_in_lead(&mut env, &args, &id, &pr, &policy, files);
            }
            Err(e) => return Err(removing(worktree, e)),
        };
        // 5. Guard U: no merge in S that the Ledger has not recorded.
        // The lease X0, what cpd last saw of the remote branch: read after guard U,
        // so that the pushes of the merges in S, which it waited for or ran R for, are seen.
        let lease = match guard_u(&mut env, &id, &key, repo, &old).and_then(|()| last_seen(repo, branch)) {
            Ok(lease) => lease,
            Err(e) => return Err(removing(worktree, e)),
        };
        // 6. The compare-and-swap of the Lead Branch, S to R'.
        let message = format!("cpd: pr rebase {id}/{key}");
        let lead_ref = format!("refs/heads/{branch}");
        let out = match git::run::run(repo, &["update-ref", "-m", &message, &lead_ref, &new, &old]) {
            Ok(out) => out,
            Err(e) => return Err(removing(worktree, e)),
        };
        if out.code == 0 {
            break (old, new, lease, worktree);
        }
        worktree.remove()?;
        if git::run::rev_parse(repo, &lead_ref)? == old {
            return Err(CpdError::new(ErrorCode::GitFailed, "git update-ref failed, and the Lead Branch did not move.")
                .with("exit", out.code)
                .with("stderr", tail(&out.stderr, TAIL_LINES)));
        }
        // The ref moved (a merge landed, or another `pr rebase` won): back to step 2.
        retries += 1;
        if retries > CAS_RETRIES {
            return Err(CpdError::new(
                ErrorCode::CasLost,
                format!("{branch} kept moving during the rebase (merges into PR {key}); the compare-and-swap lost {} times: rerun the command.", CAS_RETRIES + 1),
            )
            .with("plan", id.as_str())
            .with("pr", key.as_str()));
        }
    };

    // 7. Rule S on the Lead worktree; a stop is reported and fails nothing.
    if let Err(stale) = git::sync_lead(&lead, branch)
        && let Err(e) = report_stale(&mut env, &id, &key, &lead, "rebased", &stale) {
            return Err(removing(worktree, e));
        }

    // 8. The Gate on R' in the temporary worktree, `pre-start` then `pre-merge`; it is removed whatever the result.
    let again = format!("pr rebase --pr {key}");
    let gated = gate_in(&env, &policy, &id, &pr, worktree.path(), &new, true, &again);
    let removed = worktree.remove();
    gated?;
    removed?;
    push_rebased(&mut env, &args, &id, &pr, &old, &new, &lease)
}

/// The running Attempts of PR `key`'s Jobs as `(job key, attempt id)`, in job/attempt order.
/// One query, shared by `pr rebase` and `pr push` (hs-d801).
pub(super) fn running_attempts(conn: &rusqlite::Connection, plan: &PlanId, key: &PrKey) -> Result<Vec<(String, String)>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT j.key, a.id FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE a.plan = ?1 AND j.pr = ?2 AND a.status = 'running' ORDER BY j.key, a.id",
    )?;
    let rows = stmt.query_map([plan.as_str(), key.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// A running Attempt of any Job of the PR refuses `attempt_running`: that Worker is steered by the
/// Lead, not by the command. `what` is what the command would have done ("do the rebase", "do the
/// push"). The caller passes a store or an open write transaction, so `pr push` can check it inside
/// the transaction that serialises it with an Attempt start (ADR-0014).
pub(super) fn refuse_running(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &PrKey,
    what: &str,
) -> Result<(), CpdError> {
    let running = running_attempts(conn, plan, key)?;
    if running.is_empty() {
        return Ok(());
    }
    let mut jobs: Vec<String> = running.iter().map(|(j, _)| j.clone()).collect();
    jobs.dedup();
    let attempts: Vec<String> = running.into_iter().map(|(_, a)| a).collect();
    Err(CpdError::new(
        ErrorCode::AttemptRunning,
        format!("Job {} of PR {key} has a running Attempt: steer that Worker to {what} (message it with `cpd session send`) instead.", jobs.join(", ")),
    )
    .with("pr", key.as_str())
    .with("jobs", jobs)
    .with("attempts", attempts))
}

/// `pr rebase --continue` (contract §7.6 C0–C6): the stopped rebase finished, then the Gate and the push.
fn resume(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    // C0.
    let plan = env.plan(&id)?;
    let pr = require_open(&mut env, &plan, &args)?.clone();
    let key = pr.key.clone();
    env.store.read(|conn| hold_refused(conn, &id, &key, agent()))?;
    env.store.read(|conn| refuse_running(conn, &id, &key, "do the rebase"))?;
    let policy = pr_policy(&mut env, &id, &key)?;
    let lead = pr.lead(&id)?.clone();
    let repo = Path::new(&pr.repo);
    let branch = pr.lead_branch.as_str();
    let lead_ref = format!("refs/heads/{branch}");

    // C1.
    if !in_progress(&lead)? {
        return Err(no_rebase_error(&id, &key));
    }
    // C2.
    let files = unresolved(&lead)?;
    if !files.is_empty() {
        let message = format!("{branch} still has conflicts in {} files: resolve them and `git add` them (no commit, no push), then run the command again.", files.len());
        return Err(conflict(&id, &key, &lead, message, files));
    }
    // C3.
    let old = orig_head(&lead)?;
    let tip = git::run::rev_parse(repo, &lead_ref)?;
    if tip != old {
        return Err(moved(&id, &key, &tip));
    }
    guard_u(&mut env, &id, &key, repo, &old)?;
    let lease = last_seen(repo, branch)?;
    // C4.
    let out = git::run::run(&lead, &[NO_EDITOR.as_slice(), &["rebase", "--continue"]].concat())?;
    if out.code != 0 {
        let files = unmerged(&*Ports::current().runner, &lead)?;
        if !files.is_empty() {
            return Err(stopped(&id, &key, &lead, branch, files));
        }
        let tip = git::run::rev_parse(repo, &lead_ref)?;
        if tip != old && !in_progress(&lead)? {
            return Err(moved(&id, &key, &tip));
        }
        return Err(CpdError::new(ErrorCode::GitFailed, "git rebase --continue failed: the rebase stays in progress.")
            .with("exit", out.code)
            .with("stderr", tail(&out.stderr, TAIL_LINES)));
    }
    let new = git::run::rev_parse(&lead, "HEAD")?;
    // C5 and C6: no rule S, the Lead worktree is the one that was rebased.
    gate(&env, &policy, &id, &pr, &lead, &new, &format!("pr rebase --pr {key}"))?;
    push_rebased(&mut env, &args, &id, &pr, &old, &new, &lease)
}

/// `pr rebase --abort`: the stopped rebase given up, the Lead Branch and the Lead worktree as before.
/// It asks no PR status, runs no Gate and pushes nothing.
fn abort(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let plan = env.plan(&id)?;
    let pr = plan.select(&args.pr)?.clone();
    env.store.read(|conn| hold_refused(conn, &id, &pr.key, agent()))?;
    let lead = pr.lead(&id)?.clone();
    let branch = pr.lead_branch.as_str();

    if !in_progress(&lead)? {
        return Err(no_rebase_error(&id, &pr.key));
    }
    git::run::ok(&lead, &["rebase", "--abort"])?;
    let tip = git::run::rev_parse(Path::new(&pr.repo), &format!("refs/heads/{branch}"))?;
    if let Err(stale) = git::sync_lead(&lead, branch) {
        report_stale(&mut env, &id, &pr.key, &lead, "aborted", &stale)?;
    }
    Ok(Output::success(json!({"plan": id, "pr": pr.key, "aborted": true, "tip": tip})))
}

/// The conflict stop (contract §7.6): `git rebase <onto>` in the Lead worktree, left in progress
/// at the first conflicting commit, which is the `rebase_conflict` returned. `listed` is
/// `merge-tree`'s list, for a stop with no unmerged path of its own. A rebase that finishes
/// without stopping (a race: a merge landed after the snapshot) goes on as `--continue` does
/// from guard U, then the Gate and the push.
fn rebase_in_lead(env: &mut Env, args: &Args, id: &PlanId, pr: &PrRow, policy: &Policy, listed: Vec<String>) -> Result<Output, CpdError> {
    let lead = pr.lead(id)?;
    let repo = Path::new(&pr.repo);
    let branch = pr.lead_branch.as_str();
    let onto = onto_of(pr);

    // `git rebase` refuses tracked local changes.
    let dirty = git::run::ok(lead, &["status", "--porcelain", "--untracked-files=no"])?;
    if !dirty.trim().is_empty() {
        let files: Vec<&str> = dirty.lines().filter_map(|l| l.get(3..)).collect();
        return Err(CpdError::new(
            ErrorCode::LeadWorktreeDirty,
            format!("The Lead worktree {} has local changes to tracked files, which a rebase would refuse: nothing was changed. Commit or discard them.", lead.display()),
        )
        .with("plan", id.as_str())
        .with("pr", pr.key.as_str())
        .with("files", files));
    }

    let old = git::run::rev_parse(lead, "HEAD")?;
    let out = git::run::run(lead, &[NO_EDITOR.as_slice(), &["rebase", &onto]].concat())?;
    if out.code != 0 {
        let mut files = unmerged(&*Ports::current().runner, lead)?;
        if files.is_empty() && in_progress(lead)? {
            files = listed;
        }
        if files.is_empty() {
            return Err(CpdError::new(ErrorCode::GitFailed, "git rebase failed without a conflict.")
                .with("exit", out.code)
                .with("stderr", tail(&out.stderr, TAIL_LINES)));
        }
        return Err(stopped(id, &pr.key, lead, branch, files));
    }
    let new = git::run::rev_parse(lead, "HEAD")?;
    guard_u(env, id, &pr.key, repo, &old)?;
    let lease = last_seen(repo, branch)?;
    gate(env, policy, id, pr, lead, &new, &format!("pr rebase --pr {}", pr.key))?;
    push_rebased(env, args, id, pr, &old, &new, &lease)
}

/// Step 9 (and C6): the PR re-read after the Gate, R' pushed with the lease X0, then the merges that
/// landed during the Gate, by rule P.
fn push_rebased(env: &mut Env, args: &Args, id: &PlanId, pr: &PrRow, old: &str, new: &str, lease: &str) -> Result<Output, CpdError> {
    let key = &pr.key;
    let lead = pr.lead(id)?;
    let repo = Path::new(&pr.repo);
    let branch = pr.lead_branch.as_str();
    let lead_ref = format!("refs/heads/{branch}");

    // The PR is read again: merged during the Gate, nothing is pushed.
    let plan = env.plan(id)?;
    require_open(env, &plan, args)?;

    if pr.head_ref.is_some() {
        return push_adopted(id, pr, old, new);
    }

    // R' with the lease X0, then the merges that landed during the Gate, by rule P.
    // A rejection whose remote tip already contains R' is a success that pushed nothing:
    // two `pr rebase` of the PR at once push the same R', and the second can lose the
    // race inside the remote (Escalation nl-pr-a1-q2).
    let lease_arg = format!("--force-with-lease={lead_ref}:{lease}");
    let refspec = format!("{new}:{lead_ref}");
    let push = ["push", "--porcelain", lease_arg.as_str(), "origin", refspec.as_str()];
    let out = git::run::run(lead, &push)?;
    if out.code != 0 && !remote_contains(lead, &lead_ref, new)? {
        let argv: Vec<String> = ["git", "-C", &lead.to_string_lossy()].into_iter().chain(push).map(str::to_owned).collect();
        return Err(CpdError::new(ErrorCode::GitFailed, format!("git push exited {}.", out.code))
            .with("argv", argv)
            .with("exit", out.code)
            .with("stderr", tail(&out.stderr, TAIL_LINES)));
    }
    let tip = git::run::rev_parse(repo, &lead_ref)?;
    if tip != new && git::run::run(repo, &["merge-base", "--is-ancestor", new, &tip])?.code == 0
        && let Push::Rejected { stderr } = git::push_commit(lead, branch, &tip)? {
            return Err(CpdError::new(
                ErrorCode::GitFailed,
                format!("{} was pushed, but the merges after it ({}) were rejected: run cpd pr push --pr {key}.", short(new), short(&tip)),
            )
            .with("plan", id.as_str())
            .with("pr", key.as_str())
            .with("sha", tip.as_str())
            .with("stderr", stderr));
        }

    Ok(Output::success(json!({"plan": id, "pr": key, "rebased": old != new, "old": old, "new": new, "pushed": true})))
}

/// What the rebase goes onto: `origin/<base>`, or `origin/<head_ref>` for an adopted PR (pro-d15).
fn onto_of(pr: &PrRow) -> String {
    format!("origin/{}", onto_branch(pr))
}

fn onto_branch(pr: &PrRow) -> &str {
    pr.head_ref.as_deref().unwrap_or(&pr.base)
}

/// The push of an adopted PR (A8): rule P to `<head_ref>`, plain, no lease and no force; the merges that landed
/// during the Gate go after R' the same way.
fn push_adopted(id: &PlanId, pr: &PrRow, old: &str, new: &str) -> Result<Output, CpdError> {
    let lead = pr.lead(id)?;
    let repo = Path::new(&pr.repo);
    let remote = pr.remote_branch();
    let mut pushed = match git::push_commit(lead, remote, new)? {
        Push::Pushed { new } => new,
        Push::Contained { .. } => false,
        Push::Rejected { stderr } => return Err(rejected(id, pr, new, stderr)),
    };
    let tip = git::run::rev_parse(repo, &format!("refs/heads/{}", pr.lead_branch))?;
    if tip != new && git::run::run(repo, &["merge-base", "--is-ancestor", new, &tip])?.code == 0 {
        match git::push_commit(lead, remote, &tip)? {
            Push::Pushed { new } => pushed |= new,
            Push::Contained { .. } => {}
            Push::Rejected { stderr } => return Err(rejected(id, pr, &tip, stderr)),
        }
    }
    Ok(Output::success(json!({"plan": id, "pr": pr.key, "rebased": old != new, "old": old, "new": new, "pushed": pushed})))
}

fn rejected(id: &PlanId, pr: &PrRow, sha: &str, stderr: String) -> CpdError {
    CpdError::new(
        ErrorCode::PushRejected,
        format!("{} was rebased onto origin/{}, but its push was rejected: the remote head moved again, run cpd pr rebase --pr {}.", short(sha), pr.remote_branch(), pr.key),
    )
    .with("plan", id.as_str())
    .with("pr", pr.key.as_str())
    .with("sha", sha)
    .with("stderr", stderr)
}

/// Whether a rebase is in progress in the Lead worktree (rule S step 0): `rebase-merge` or
/// `rebase-apply` in its git dir.
fn in_progress(lead: &Path) -> Result<bool, CpdError> {
    in_progress_with(&*Ports::current().runner, lead)
}

/// [`in_progress`] on `runner` (a test passes a `FakeRunner`).
pub(crate) fn in_progress_with(runner: &dyn Runner, lead: &Path) -> Result<bool, CpdError> {
    for name in ["rebase-merge", "rebase-apply"] {
        if git_path(runner, lead, name)?.exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn git_path(runner: &dyn Runner, lead: &Path, name: &str) -> Result<std::path::PathBuf, CpdError> {
    let path = git::run::ok_with(runner, lead, &["rev-parse", "--path-format=absolute", "--git-path", name])?;
    Ok(std::path::PathBuf::from(path.trim()))
}

/// The tip the stopped rebase started from, S (`rebase-merge/orig-head`, `rebase-apply/orig-head`).
fn orig_head(lead: &Path) -> Result<String, CpdError> {
    for name in ["rebase-merge/orig-head", "rebase-apply/orig-head"] {
        if let Ok(sha) = std::fs::read_to_string(git_path(&*Ports::current().runner, lead, name)?) {
            return Ok(sha.trim().to_owned());
        }
    }
    Err(CpdError::new(ErrorCode::GitFailed, format!("The rebase in {} has no orig-head: its starting tip is unknown.", lead.display())))
}

/// The paths git has not merged (`git diff --name-only --diff-filter=U`).
fn unmerged(runner: &dyn Runner, lead: &Path) -> Result<Vec<String>, CpdError> {
    let out = git::run::ok_with(runner, lead, &["diff", "--name-only", "--diff-filter=U"])?;
    Ok(out.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

/// What keeps `--continue` from going on (C2): the unmerged paths, and the changed files
/// (`git diff --name-only HEAD`) with a conflict marker line at column 0. `=======` alone is
/// not one (a Markdown underline); it counts with a `<<<<<<<` line, which counts by itself.
fn unresolved(lead: &Path) -> Result<Vec<String>, CpdError> {
    unresolved_with(&*Ports::current().runner, lead)
}

/// [`unresolved`] on `runner` (a test passes a `FakeRunner`).
pub(crate) fn unresolved_with(runner: &dyn Runner, lead: &Path) -> Result<Vec<String>, CpdError> {
    let mut files: BTreeSet<String> = unmerged(runner, lead)?.into_iter().collect();
    let changed = git::run::ok_with(runner, lead, &["diff", "--name-only", "-z", "HEAD"])?;
    for name in changed.split('\0').filter(|n| !n.is_empty()) {
        // A deleted file has no markers.
        let Ok(bytes) = std::fs::read(lead.join(name)) else { continue };
        if bytes.split(|b| *b == b'\n').any(|line| line.starts_with(b"<<<<<<< ") || line.starts_with(b">>>>>>> ")) {
            files.insert(name.to_owned());
        }
    }
    Ok(files.into_iter().collect())
}

/// `cpd pr rebase --pr <key> [--plan <id>] <flag>`: `--plan` unless `CPD_PLAN` already names the Plan.
fn command(plan: &PlanId, key: &PrKey, flag: &str) -> String {
    let selected = std::env::var("CPD_PLAN").is_ok_and(|p| p == plan.as_str());
    if selected { format!("cpd pr rebase --pr {key} {flag}") } else { format!("cpd pr rebase --pr {key} --plan {plan} {flag}") }
}

/// The rebase stopped at a conflict in the Lead worktree (the stop's `rebase_conflict`).
fn stopped(plan: &PlanId, key: &PrKey, lead: &Path, branch: &str, files: Vec<String>) -> CpdError {
    let message = format!(
        "{branch} conflicts in {} files: the rebase is stopped in {}. Resolve them and `git add` them (no commit, no push), then run `{}`; or give it up with `{}`.",
        files.len(),
        lead.display(),
        command(plan, key, "--continue"),
        command(plan, key, "--abort")
    );
    conflict(plan, key, lead, message, files)
}

fn conflict(plan: &PlanId, key: &PrKey, lead: &Path, message: String, files: Vec<String>) -> CpdError {
    CpdError::new(ErrorCode::RebaseConflict, message)
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("files", files)
        .with("worktree", lead.display().to_string())
        .with("next", command(plan, key, "--continue"))
        .with("abort", command(plan, key, "--abort"))
}

fn in_progress_error(plan: &PlanId, key: &PrKey) -> CpdError {
    CpdError::new(
        ErrorCode::RebaseInProgress,
        format!("A rebase of PR {key} is already in progress in its Lead worktree: run `{}` or `{}`.", command(plan, key, "--continue"), command(plan, key, "--abort")),
    )
    .with("plan", plan.as_str())
    .with("pr", key.as_str())
    .with("next", command(plan, key, "--continue"))
    .with("abort", command(plan, key, "--abort"))
}

fn no_rebase_error(plan: &PlanId, key: &PrKey) -> CpdError {
    CpdError::new(ErrorCode::NoRebaseInProgress, format!("No rebase of PR {key} is in progress in its Lead worktree."))
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
}

/// The Lead Branch is not where the stopped rebase started: a merge landed during the stop.
fn moved(plan: &PlanId, key: &PrKey, tip: &str) -> CpdError {
    CpdError::new(
        ErrorCode::CasLost,
        format!("The Lead Branch of PR {key} moved during the stop (a merge landed): run `{}`, then `cpd pr rebase --pr {key}`.", command(plan, key, "--abort")),
    )
    .with("plan", plan.as_str())
    .with("pr", key.as_str())
    .with("reason", "lead_branch_moved")
    .with("tip", tip)
}

/// The lease X0 (§7.6 step 2, Escalation nl-pr-a1-q1): `refs/remotes/origin/<branch>`,
/// what cpd last saw of the remote branch; every push of it from this machine,
/// rule P's included, updates it. A push cpd has not seen (another machine,
/// GitHub's "Update branch") makes the lease fail instead of being overwritten.
/// The empty string when cpd has never seen the branch: as a lease, "must not exist".
/// A push of what the remote already has is "up to date" whatever the lease (run
/// with git 2.43), so a second `pr rebase` pushing the same R' succeeds.
fn last_seen(repo: &Path, branch: &str) -> Result<String, CpdError> {
    let tracking = format!("refs/remotes/origin/{branch}");
    let out = git::run::run(repo, &["rev-parse", "--verify", "--quiet", &tracking])?;
    match out.code {
        0 => Ok(out.stdout.trim().to_owned()),
        1 => Ok(String::new()),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git rev-parse --verify {tracking} exited {code}."))
            .with("exit", code)
            .with("stderr", tail(&out.stderr, TAIL_LINES))),
    }
}

/// Whether origin's `reference` contains `sha`, as rule P checks (§7.4): its tip `X`
/// (`git ls-remote`) is in the local object store and `sha` is an ancestor of it, or it.
fn remote_contains(lead: &Path, reference: &str, sha: &str) -> Result<bool, CpdError> {
    remote_contains_with(&*Ports::current().runner, lead, reference, sha)
}

/// [`remote_contains`] on `runner` (a test passes a `FakeRunner`).
pub(crate) fn remote_contains_with(runner: &dyn Runner, lead: &Path, reference: &str, sha: &str) -> Result<bool, CpdError> {
    let listed = git::run::ok_with(runner, lead, &["ls-remote", "origin", reference])?;
    let tip = listed.lines().find(|l| l.split_whitespace().nth(1) == Some(reference)).and_then(|l| l.split_whitespace().next());
    let Some(tip) = tip else { return Ok(false) };
    let known = git::run::run_with(runner, lead, &["cat-file", "-e", &format!("{tip}^{{commit}}")])?.code == 0;
    Ok(known && git::run::run_with(runner, lead, &["merge-base", "--is-ancestor", sha, tip])?.code == 0)
}

/// `error`, after removing the temporary rebase worktree; the removal's own failure is
/// dropped, so that the error the Lead acts on is the one that stopped the command.
fn removing(worktree: RebaseWorktree, error: CpdError) -> CpdError {
    let _ = worktree.remove();
    error
}

/// Guard U (§7.6 step 5): every Attempt of the PR's Jobs in `merging` whose
/// Attempt branch tip is an ancestor of the snapshot `snapshot` (its
/// compare-and-swap happened, its step 10 has not committed) is ended before
/// the rebase rewrites it: by R when its driver is dead, else by its driver,
/// waited for at most 60 s, else `cas_lost` with `details.attempt`.
fn guard_u(env: &mut Env, plan: &PlanId, key: &PrKey, repo: &Path, snapshot: &str) -> Result<(), CpdError> {
    let merging: Vec<String> = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.id FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
              WHERE a.plan = ?1 AND j.pr = ?2 AND a.status = 'merging' ORDER BY a.id",
        )?;
        let rows = stmt.query_map([plan.as_str(), key.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let mut catalog = None;
    for attempt in merging {
        let attempt = AttemptId::parse(&attempt)?;
        let branch = env.store.read(|conn| merge::load(conn, plan, &attempt))?.branch;
        let tip = git::run::rev_parse(repo, &format!("refs/heads/{branch}"))?;
        let contained = git::run::run(repo, &["merge-base", "--is-ancestor", &tip, snapshot])?;
        match contained.code {
            0 => settle_in(env, &mut catalog, plan, key, &attempt)?,
            1 => {}
            code => {
                return Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-base --is-ancestor exited {code}."))
                    .with("exit", code)
                    .with("stderr", tail(&contained.stderr, TAIL_LINES)));
            }
        }
    }
    Ok(())
}

/// Waits until Attempt `attempt` is no longer `merging`, at most 60 s, reading the
/// store every 200 ms; whenever its driver lock is free (the driver is dead), R
/// ends it. pi's catalog, which R needs for the ledger's Tier checks (a crashed
/// Attempt is retried at its own Tier), is loaded the first time R runs.
/// Reads the time and takes the naps on `env.ports.clock`.
pub(crate) fn settle_in(env: &mut Env, catalog: &mut Option<Catalog>, plan: &PlanId, key: &PrKey, attempt: &AttemptId) -> Result<(), CpdError> {
    let clock = env.ports.clock.clone();
    let deadline = clock.now() + GUARD_WAIT;
    loop {
        if crate::driver::lock::probe(&env.home, plan, attempt)?.is_some() {
            if catalog.is_none() {
                let home = env.home.clone();
                let models = env.store.read(|conn| crate::policy::model::models_of_pr(conn, &home, plan, key))?;
                *catalog = Some(config::load_catalog(&env.config, &models)?);
            }
            let catalog = catalog.as_ref().expect("loaded above");
            let ctx = merge::Ctx { home: &env.home, config: &env.config, catalog, source: COMMAND, metrics: &|_| None };
            merge::recover_attempt(&mut env.store, &ctx, plan, attempt)?;
        }
        let status = env.store.read(|conn| merge::load(conn, plan, attempt))?.status;
        if status != "merging" {
            return Ok(());
        }
        if clock.now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::CasLost,
                format!(
                    "Attempt {attempt} of PR {key} is on the Lead Branch but its merge has not been recorded after {} s: nothing was changed. Rerun the command.",
                    GUARD_WAIT.as_secs()
                ),
            )
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("attempt", attempt.to_string()));
        }
        clock.sleep(GUARD_POLL);
    }
}

/// Rule S stopped after the Lead Branch moved (§7.6 step 7, and `--abort`; `what` is
/// "rebased" or "aborted"): one `error` Event `lead_worktree_stale` for the Plan; the command goes on.
fn report_stale(env: &mut Env, plan: &PlanId, key: &PrKey, lead: &Path, what: &str, stale: &CpdError) -> Result<(), CpdError> {
    let fix = stale.details().get("fix").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    let text = format!(
        "PR {key} of Plan {plan} was {what}, but its Lead worktree {} still holds the previous tree. {fix}. Then: cpd plan show --plan {plan}",
        lead.display()
    );
    let mut details = stale.details().clone();
    details.insert("pr".to_owned(), json!(key));
    env.store.write(COMMAND, |tx| {
        append(tx, NewEvent::error(text, COMMAND, stale.code().as_str(), stale.message(), &details).plan(plan.clone()))?;
        Ok(())
    })
}

/// The selected PR must be `open` (`illegal_transition`); a Plan that has not
/// been locked is `illegal_phase`.
/// An adopted PR may also be `dispatch` once its Jobs but the `open` and `update` ones are done (A8).
fn require_open<'a>(env: &mut Env, plan: &'a PlanPrs, args: &Args) -> Result<&'a PrRow, CpdError> {
    if matches!(plan.plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "deploy"));
    }
    let pr = plan.select(&args.pr)?;
    if pr.head_ref.is_some() && pr.status == "dispatch" {
        require_jobs_done(env, plan, &pr.key)?;
        return Ok(pr);
    }
    if pr.status != "open" {
        return Err(pr.refuse(&plan.plan.id, COMMAND, "open"));
    }
    Ok(pr)
}

/// `git rebase <onto>` in `worktree`. `Some(files)` on a conflict, after
/// `git rebase --abort`; `None` when it rebased.
fn rebase_onto(worktree: &Path, onto: &str) -> Result<Option<Vec<String>>, CpdError> {
    rebase_onto_with(&*Ports::current().runner, worktree, onto)
}

/// [`rebase_onto`] on `runner` (a test passes a `FakeRunner`).
pub(crate) fn rebase_onto_with(runner: &dyn Runner, worktree: &Path, onto: &str) -> Result<Option<Vec<String>>, CpdError> {
    let out = git::run::run_with(runner, worktree, &["rebase", onto])?;
    if out.code == 0 {
        return Ok(None);
    }
    let conflicts = git::run::ok_with(runner, worktree, &["diff", "--name-only", "--diff-filter=U"])?;
    let files: Vec<String> = conflicts.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
    let abort = git::run::run_with(runner, worktree, &["rebase", "--abort"])?;
    if files.is_empty() || abort.code != 0 {
        let (what, failed) = if files.is_empty() {
            ("git rebase failed without a conflict", &out)
        } else {
            ("git rebase --abort failed after a conflict", &abort)
        };
        return Err(CpdError::new(ErrorCode::GitFailed, format!("{what}."))
            .with("exit", failed.code)
            .with("stderr", tail(&failed.stderr, TAIL_LINES)));
    }
    Ok(Some(files))
}
