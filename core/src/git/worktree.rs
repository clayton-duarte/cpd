//! Worktrees and branches (contract §7.1, §7.2).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::hooks::{HookOutcome, Operation, run_hook};
use super::run::{branch_exists, has_origin, ok, rev_parse, run};
use super::{TAIL_LINES, tail};
use crate::error::{CpdError, ErrorCode};
use crate::home::Hook;
use crate::ids::{AttemptId, PlanId};

/// The Lead Branch of a Plan: `cpd/<plan>`.
pub fn lead_branch(plan: &PlanId) -> String {
    format!("cpd/{plan}")
}

/// The branch of an Attempt: `cpd/<plan>--<attempt>`.
pub fn attempt_branch(plan: &PlanId, attempt: &AttemptId) -> String {
    format!("cpd/{plan}--{attempt}")
}

/// `<repo-parent>/<repo-name>.<suffix>`: a sibling of the repo's main checkout.
fn sibling(repo: &Path, suffix: &str) -> Result<PathBuf, CpdError> {
    let (Some(parent), Some(name)) = (repo.parent(), repo.file_name()) else {
        return Err(CpdError::new(
            ErrorCode::NotAGitRepo,
            "The repo path has no parent directory to put worktrees in.",
        )
        .with("repo", repo.to_string_lossy().into_owned()));
    };
    let mut leaf = name.to_os_string();
    leaf.push(format!(".{suffix}"));
    Ok(parent.join(leaf))
}

/// The Lead worktree: `<repo-parent>/<repo-name>.<plan>`.
pub fn lead_worktree_path(repo: &Path, plan: &PlanId) -> Result<PathBuf, CpdError> {
    sibling(repo, plan.as_str())
}

/// The Attempt worktree: `<repo-parent>/<repo-name>.<plan>--<attempt>`.
pub fn attempt_worktree_path(repo: &Path, plan: &PlanId, attempt: &AttemptId) -> Result<PathBuf, CpdError> {
    sibling(repo, &format!("{plan}--{attempt}"))
}

/// One entry of `git worktree list --porcelain`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// The checked-out branch, without `refs/heads/`; `None` when detached or bare.
    pub branch: Option<String>,
}

/// Every worktree of the repo `dir` belongs to; the first is the main one.
pub fn list(dir: &Path) -> Result<Vec<Worktree>, CpdError> {
    let text = ok(dir, &["worktree", "list", "--porcelain"])?;
    let mut found = Vec::new();
    for block in text.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_owned());
            }
        }
        if let Some(path) = path {
            found.push(Worktree { path, branch });
        }
    }
    Ok(found)
}

/// The worktree that has `branch` checked out, if any.
pub fn worktree_of_branch(dir: &Path, branch: &str) -> Result<Option<PathBuf>, CpdError> {
    Ok(list(dir)?.into_iter().find(|w| w.branch.as_deref() == Some(branch)).map(|w| w.path))
}

/// `path` with symlinks resolved, also when the last component does not exist yet.
fn real(path: &Path) -> PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    match (path.parent().and_then(|p| p.canonicalize().ok()), path.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path.to_path_buf(),
    }
}

fn path_str(path: &Path) -> Result<&str, CpdError> {
    path.to_str().ok_or_else(|| {
        CpdError::new(ErrorCode::GitFailed, "A worktree path is not valid UTF-8.")
            .with("path", path.to_string_lossy().into_owned())
    })
}

/// How `pre-start` runs after a worktree is created (contract §7.3).
#[derive(Clone, Debug)]
pub struct Setup {
    /// `setup_minutes`.
    pub timeout: Duration,
    /// Where the hook's whole output goes: `run/<plan>/logs/<attempt>.pre-start.log`
    /// (use `CpdHome::hook_log`).
    pub log: PathBuf,
}

/// A worktree that exists, set up, on its new branch.
#[derive(Clone, Debug, PartialEq)]
pub struct Created {
    pub path: PathBuf,
    pub branch: String,
    /// The commit the branch was cut at: the Lead Branch HEAD for an Attempt
    /// (`base_sha`), `origin/<base>` (or `<base>`) for a Lead, the tip of
    /// `origin/cpd/<plan>` for a Lead Branch reused from the remote.
    pub base_sha: String,
    /// A Lead worktree only: the `push_failed` error of the push at `plan lock`
    /// (R35). A failed push does not fail the lock; the caller appends it as an
    /// `error` Event. `None` when the push succeeded, was not needed (the branch
    /// was reused) or the repo has no `origin`.
    pub push_failed: Option<CpdError>,
}

/// Runs `pre-start` in `path`; on any failure removes the worktree and branch
/// and returns `setup_failed` (contract §7.2).
fn run_setup(
    dir: &Path,
    path: &Path,
    branch: &str,
    base_branch: &str,
    setup: &Setup,
) -> Result<(), CpdError> {
    let op = Operation { branch: base_branch };
    let result = run_hook(Hook::PreStart, path, Some(&op), setup.timeout, &setup.log);
    let failure = match result {
        Ok(r) if r.outcome == HookOutcome::Passed => return Ok(()),
        Ok(r) => {
            let mut e = CpdError::new(
                ErrorCode::SetupFailed,
                format!("pre-start failed in {}.", path.display()),
            )
            .with("outcome", r.outcome.to_string())
            .with("tail", json!(r.tail));
            if r.tail.is_some() {
                e = e.with("log", setup.log.to_string_lossy().into_owned());
            }
            e
        }
        Err(e) => e,
    };
    // Best effort: the failure to report is the original one.
    let _ = discard_worktree(dir, path, branch);
    Err(failure)
}

/// Pushes the Lead Branch from the Lead worktree: `git -C <lead worktree> push
/// [-u] origin <branch>`, a plain push, never forced (contract §7.2, §7.4, R35).
/// `Ok(())` also when the repo has no `origin` (A3): nothing to push to. A
/// rejected or failed push is the returned `push_failed` error with git's
/// stderr; it never fails the caller's operation.
pub fn push_lead_branch(lead_worktree: &Path, branch: &str, set_upstream: bool) -> Result<(), CpdError> {
    let pushed = has_origin(lead_worktree).and_then(|origin| {
        if !origin {
            return Ok(None);
        }
        let mut args = vec!["push"];
        if set_upstream {
            args.push("-u");
        }
        args.extend(["origin", branch]);
        run(lead_worktree, &args).map(Some)
    });
    match pushed {
        Ok(None) => Ok(()),
        Ok(Some(out)) if out.code == 0 => Ok(()),
        Ok(Some(out)) => Err(CpdError::new(
            ErrorCode::PushFailed,
            format!("git push of {branch} to origin exited {}.", out.code),
        )
        .with("branch", branch)
        .with("exit", out.code)
        .with("stderr", tail(&out.stderr, TAIL_LINES))),
        Err(e) => Err(CpdError::new(
            ErrorCode::PushFailed,
            format!("git push of {branch} to origin could not run: {}", e.message()),
        )
        .with("branch", branch)
        .with("git", e.to_json())),
    }
}

/// What the remote has of the Lead Branch (contract §7.2, R35).
enum Remote {
    /// No `origin`: no remote check (A3).
    None,
    /// `origin` has no such branch: create it and push it.
    Absent,
    /// `origin/<branch>` exists and has `origin/<base>` as an ancestor.
    Reusable,
}

/// `fetch origin <base>` and the remote check of the Lead Branch. Refuses
/// `branch_exists` when `origin/<branch>` does not contain `origin/<base>`.
fn check_remote(repo: &Path, branch: &str, base: &str) -> Result<Remote, CpdError> {
    if !has_origin(repo)? {
        return Ok(Remote::None);
    }
    ok(repo, &["fetch", "origin", base])?;
    let listed = run(repo, &["ls-remote", "--exit-code", "--heads", "origin", branch])?;
    match listed.code {
        0 => {}
        2 => return Ok(Remote::Absent),
        code => {
            return Err(CpdError::new(
                ErrorCode::SetupFailed,
                format!("Cannot tell whether {branch} is on origin: git ls-remote exited {code}."),
            )
            .with("exit", code)
            .with("stderr", tail(&listed.stderr, TAIL_LINES)));
        }
    }
    ok(repo, &["fetch", "origin", branch])?;
    let remote_branch = format!("origin/{branch}");
    let remote_base = format!("origin/{base}");
    let ancestor = run(repo, &["merge-base", "--is-ancestor", &remote_base, &remote_branch])?;
    match ancestor.code {
        0 => Ok(Remote::Reusable),
        1 => Err(CpdError::new(
            ErrorCode::BranchExists,
            format!("Branch {branch} already exists on origin and does not contain {remote_base}: cpd does not reset or force-push it."),
        )
        .with("branch", branch)
        .with("remote", remote_branch)
        .with("base", remote_base)),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-base --is-ancestor exited {code}."))
            .with("exit", code)
            .with("stderr", tail(&ancestor.stderr, TAIL_LINES))),
    }
}

/// Creates the Lead worktree and runs `pre-start` in it (contract §7.2, `plan lock`).
///
/// `repo` is the main checkout. With an `origin`, the Lead Branch on the
/// remote is reused when it contains `origin/<base>`, refused `branch_exists`
/// when it does not, and created from `origin/<base>` and pushed (after
/// `pre-start`) when absent (R35). A leftover of an interrupted lock (a worktree
/// of this repo at the path, on `cpd/<plan>`) is removed first; any other
/// thing at the path is `worktree_exists`, any other `cpd/<plan>` is
/// `branch_exists`. On failure the worktree and branch are removed.
pub fn create_lead_worktree(repo: &Path, plan: &PlanId, base: &str, setup: &Setup) -> Result<Created, CpdError> {
    let path = lead_worktree_path(repo, plan)?;
    let branch = lead_branch(plan);

    let leftover = list(repo)?.iter().any(|w| real(&w.path) == real(&path) && w.branch.as_deref() == Some(&branch));
    if leftover {
        discard_worktree(repo, &path, &branch)?;
    }
    if path.exists() {
        return Err(CpdError::new(ErrorCode::WorktreeExists, format!("{} already exists.", path.display()))
            .with("path", path.to_string_lossy().into_owned()));
    }
    if branch_exists(repo, &branch)? {
        return Err(CpdError::new(ErrorCode::BranchExists, format!("Branch {branch} already exists."))
            .with("branch", branch));
    }

    let remote = check_remote(repo, &branch, base)?;
    let start = match remote {
        Remote::None => base.to_owned(),
        Remote::Absent => format!("origin/{base}"),
        Remote::Reusable => format!("origin/{branch}"),
    };

    ok(repo, &["worktree", "add", "-b", &branch, path_str(&path)?, &start])?;
    let base_sha = match rev_parse(&path, "HEAD") {
        Ok(sha) => sha,
        Err(e) => {
            let _ = discard_worktree(repo, &path, &branch);
            return Err(e);
        }
    };
    run_setup(repo, &path, &branch, base, setup)?;
    // The remote is touched only after `pre-start` has passed.
    let push_failed = match remote {
        Remote::Absent => push_lead_branch(&path, &branch, true).err(),
        Remote::None | Remote::Reusable => None,
    };
    Ok(Created { path, branch, base_sha, push_failed })
}

/// Creates an Attempt worktree from the Lead Branch HEAD and runs `pre-start`
/// in it, blocking (contract §7.2, A2). On failure the worktree and branch are
/// removed; the driver ends the Attempt `failed` / `setup_failed`.
pub fn create_attempt_worktree(
    repo: &Path,
    lead_worktree: &Path,
    plan: &PlanId,
    attempt: &AttemptId,
    setup: &Setup,
) -> Result<Created, CpdError> {
    let path = attempt_worktree_path(repo, plan, attempt)?;
    let lead = lead_branch(plan);
    let branch = attempt_branch(plan, attempt);

    let base_sha = rev_parse(lead_worktree, &format!("refs/heads/{lead}"))?;
    ok(lead_worktree, &["worktree", "add", "-b", &branch, path_str(&path)?, &base_sha])?;
    run_setup(lead_worktree, &path, &branch, &lead, setup)?;
    Ok(Created { path, branch, base_sha, push_failed: None })
}

/// Removes a worktree and its branch whatever state they are in:
/// `git worktree remove --force`, `git branch -D`. Used after a failed setup
/// and when a Plan is `done` (contract §7.2, commands.md §9). Idempotent: what
/// is already gone is skipped. Run `dir` in the main repo or another worktree.
pub fn discard_worktree(dir: &Path, path: &Path, branch: &str) -> Result<(), CpdError> {
    remove(dir, path, branch, true)
}

/// Removes the worktree and branch of a Merged Attempt (contract §7.2, A9):
/// `git worktree remove` then `git branch -d`; git refuses the `-d` unless the
/// branch is merged into HEAD of `lead_worktree`. Idempotent for what is already gone.
pub fn remove_merged_attempt(lead_worktree: &Path, path: &Path, branch: &str) -> Result<(), CpdError> {
    remove(lead_worktree, path, branch, false)
}

fn remove(dir: &Path, path: &Path, branch: &str, force: bool) -> Result<(), CpdError> {
    if list(dir)?.iter().any(|w| real(&w.path) == real(path)) {
        let path = path_str(path)?;
        if force {
            ok(dir, &["worktree", "remove", "--force", path])?;
        } else {
            ok(dir, &["worktree", "remove", path])?;
        }
    }
    if branch_exists(dir, branch)? {
        ok(dir, &["branch", if force { "-D" } else { "-d" }, branch])?;
    }
    Ok(())
}
