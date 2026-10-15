//! Worktrees and branches (contract §7.1, §7.2).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::hooks::{HookOutcome, Operation, run_hook};
use super::run::{branch_exists, ok, rev_parse, run};
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub path: PathBuf,
    pub branch: String,
    /// The commit the branch was cut at: the Lead Branch HEAD for an Attempt
    /// (`base_sha`), `origin/<base>` (or `<base>`) for a Lead.
    pub base_sha: String,
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

/// Creates the Lead worktree and runs `pre-start` in it (contract §7.2, `plan lock`).
///
/// `repo` is the main checkout. A leftover of an interrupted lock (a worktree
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

    let has_origin = run(repo, &["remote", "get-url", "origin"])?.code == 0;
    let start = if has_origin {
        ok(repo, &["fetch", "origin", base])?;
        format!("origin/{base}")
    } else {
        base.to_owned()
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
    Ok(Created { path, branch, base_sha })
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
    Ok(Created { path, branch, base_sha })
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
