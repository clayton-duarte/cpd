//! The gate worktree (contract §7.1, §7.6 "The Gate on a snapshot", hs-d243):
//! `pr open`, `pr push` and `pr rebase` run the Gate on one commit, in a
//! throwaway worktree of their own, detached at it, never in the Lead worktree.

use std::path::{Path, PathBuf};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

use super::hooks::{HookOutcome, Operation, run_hook_as};
use super::run::{ok, worktree_add};
use super::worktree::{Setup, list};
use crate::error::{CpdError, ErrorCode};
use crate::home::Hook;
use crate::ids::{PlanId, PrKey};

/// `<repo-parent>/<repo-name>.<plan>.<key>.gate-` (contract §7.1); a gate
/// worktree's path is this followed by the pid of the process that runs the Gate.
fn prefix(repo: &Path, plan: &PlanId, key: &PrKey) -> Result<(PathBuf, String), CpdError> {
    let (Some(parent), Some(name)) = (repo.parent(), repo.file_name()) else {
        return Err(CpdError::new(ErrorCode::NotAGitRepo, "The repo path has no parent directory to put worktrees in.")
            .with("repo", repo.to_string_lossy().into_owned()));
    };
    Ok((parent.to_path_buf(), format!("{}.{plan}.{key}.gate-", name.to_string_lossy())))
}

/// The gate worktree of PR `key` run by process `pid`:
/// `<repo-parent>/<repo-name>.<plan>.<key>.gate-<pid>` (contract §7.1).
pub fn gate_worktree_path(repo: &Path, plan: &PlanId, key: &PrKey, pid: u32) -> Result<PathBuf, CpdError> {
    let (parent, leaf) = prefix(repo, plan, key)?;
    Ok(parent.join(format!("{leaf}{pid}")))
}

/// Removes every gate worktree of PR `key` whose pid is not alive (`kill(pid, 0)`
/// fails with `ESRCH`): the leftovers of killed runs (contract §7.6 step 1,
/// §7.2). One whose pid lives is left alone. Returns the removed paths.
pub fn sweep(repo: &Path, plan: &PlanId, key: &PrKey) -> Result<Vec<PathBuf>, CpdError> {
    let (parent, leaf) = prefix(repo, plan, key)?;
    let parent = parent.canonicalize().unwrap_or(parent);
    let mut removed = Vec::new();
    for worktree in list(repo)? {
        let (Some(dir), Some(name)) = (worktree.path.parent(), worktree.path.file_name()) else { continue };
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix(&leaf)).and_then(|p| p.parse::<i32>().ok()) else { continue };
        if dir != parent || pid <= 0 || kill(Pid::from_raw(pid), None) != Err(Errno::ESRCH) {
            continue;
        }
        force_remove(repo, &worktree.path)?;
        removed.push(worktree.path);
    }
    Ok(removed)
}

/// `git -C <repo> worktree remove --force --force <path>`: twice, because a gate
/// worktree is locked (git-worktree(1)); also when its directory is gone.
fn force_remove(repo: &Path, path: &Path) -> Result<(), CpdError> {
    let path = path.to_string_lossy();
    ok(repo, &["worktree", "remove", "--force", "--force", &path]).map(|_| ())
}

/// How the Gate runs in a gate worktree (contract §7.3, §7.6 step 3).
#[derive(Clone, Debug)]
pub struct GateHooks<'a> {
    /// The PR's Lead Branch: the template variable `branch` of both hooks,
    /// since the gate worktree's `HEAD` is detached.
    pub lead_branch: &'a str,
    /// The PR's base: `base` of `pre-start` and `target` of `pre-merge`.
    pub base: &'a str,
    /// `setup_minutes`, and `run/<plan>/logs/pr-<key>.pre-start.log`.
    pub pre_start: Setup,
    /// `gate_minutes`, and `run/<plan>/logs/pr-<key>.pre-merge.log`.
    pub pre_merge: Setup,
}

/// How the Gate ended when it did not pass. What it means is the caller's
/// (contract §7.6 step 3): a `pre-start` that did not pass is `gate_error`;
/// a `pre-merge` that failed is `gate_failed`, one that erred `gate_error`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// The hook that did not pass: `pre-start` (then `pre-merge` did not run) or `pre-merge`.
    pub hook: Hook,
    /// Never [`HookOutcome::Passed`].
    pub outcome: HookOutcome,
    /// Its last 60 lines; `None` when no command ran.
    pub tail: Option<String>,
    /// Its whole output.
    pub log: PathBuf,
}

/// A gate worktree that exists: detached at the gated commit and locked.
/// Remove it with [`GateWorktree::remove`] (or [`GateWorktree::gate_and_remove`]),
/// whatever the result (contract §7.6 step 4).
#[derive(Debug)]
pub struct GateWorktree {
    repo: PathBuf,
    path: PathBuf,
}

impl GateWorktree {
    /// `git -C <repo> worktree add --detach --lock --reason "cpd gate <pid>" W <sha>`,
    /// `W` the path of this process's pid (contract §7.6 step 2): no branch and
    /// no `.git/config` write; locked, so `worktree prune` leaves it alone. Retried
    /// while it fails on a concurrent `worktree add` of the repo (contract §7).
    pub fn add(repo: &Path, plan: &PlanId, key: &PrKey, sha: &str) -> Result<GateWorktree, CpdError> {
        let pid = std::process::id();
        let path = gate_worktree_path(repo, plan, key, pid)?;
        let reason = format!("cpd gate {pid}");
        let target = path.to_string_lossy().into_owned();
        worktree_add(repo, &["--detach", "--lock", "--reason", &reason, &target, sha], &path)?;
        Ok(GateWorktree { repo: repo.to_path_buf(), path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The Gate in W (contract §7.6 step 3): `pre-start`, then, when it passed,
    /// `pre-merge`, both from W's `.config/wt.toml` (the gated commit's), with
    /// `branch` the Lead Branch. `Err` is a git call that failed while building
    /// the template variables.
    pub fn run(&self, hooks: &GateHooks<'_>) -> Result<Result<(), Refused>, CpdError> {
        let operation = Operation { branch: hooks.base };
        for (hook, setup) in [(Hook::PreStart, &hooks.pre_start), (Hook::PreMerge, &hooks.pre_merge)] {
            let result = run_hook_as(hook, &self.path, hooks.lead_branch, Some(&operation), setup.timeout, &setup.log)?;
            if result.outcome != HookOutcome::Passed {
                return Ok(Err(Refused { hook, outcome: result.outcome, tail: result.tail, log: setup.log.clone() }));
            }
        }
        Ok(Ok(()))
    }

    /// `git -C <repo> worktree remove --force --force W` (contract §7.6 step 4).
    pub fn remove(self) -> Result<(), CpdError> {
        force_remove(&self.repo, &self.path)
    }

    /// [`GateWorktree::run`], then [`GateWorktree::remove`] whatever it returned.
    /// The Gate's own error wins over a failed removal; a failed removal after a
    /// Gate that ran is returned as the error (the leftover is swept once this
    /// process has ended).
    pub fn gate_and_remove(self, hooks: &GateHooks<'_>) -> Result<Result<(), Refused>, CpdError> {
        let gated = self.run(hooks);
        let removed = self.remove();
        let gated = gated?;
        removed?;
        Ok(gated)
    }
}

/// The Gate on a snapshot, steps 1–4 (contract §7.6): sweep the PR's dead gate
/// worktrees, add one at `sha`, run the Gate in it, remove it.
pub fn gate(repo: &Path, plan: &PlanId, key: &PrKey, sha: &str, hooks: &GateHooks<'_>) -> Result<Result<(), Refused>, CpdError> {
    sweep(repo, plan, key)?;
    GateWorktree::add(repo, plan, key, sha)?.gate_and_remove(hooks)
}
