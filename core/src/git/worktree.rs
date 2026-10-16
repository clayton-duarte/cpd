//! Worktrees and branches (contract §7.1, §7.2).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::hooks::{HookOutcome, Operation, run_hook};
use super::run::{branch_exists, has_origin, ok, rev_parse, run};
use super::{TAIL_LINES, tail};
use crate::error::{CpdError, ErrorCode};
use crate::home::Hook;
use crate::ids::{AttemptId, PlanId, PrKey};

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

/// The Lead worktree of a PR (contract §7.1): `<repo-parent>/<repo-name>.<plan>.<key>`.
/// A PR made by migration 3 keeps the Lead Branch `cpd/<plan>` and with it the
/// worktree `<repo-parent>/<repo-name>.<plan>` it had.
pub fn pr_lead_worktree_path(repo: &Path, plan: &PlanId, key: &PrKey, lead_branch_name: &str) -> Result<PathBuf, CpdError> {
    if lead_branch_name == lead_branch(plan) {
        lead_worktree_path(repo, plan)
    } else {
        sibling(repo, &format!("{plan}.{key}"))
    }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
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

/// The part of a Lead worktree's start that failed (contract §3.8 "A failed
/// start": `step` of `pr_start_failed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The path, the local branches, `git worktree add`.
    Worktree,
    /// `fetch origin`, `ls-remote`, the ancestor check.
    Remote,
    /// The `pre-start` hook.
    PreStart,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Worktree => "worktree",
            Step::Remote => "remote",
            Step::PreStart => "pre-start",
        }
    }
}

/// A failed start: the error and the step it happened in. Nothing is left behind.
#[derive(Clone, Debug)]
pub struct StartFailure {
    pub step: Step,
    pub error: CpdError,
}

/// A Lead worktree that exists on its Lead Branch with `pre-start` passed;
/// nothing is pushed yet.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub path: PathBuf,
    pub branch: String,
    /// The commit the branch was cut at (the tip of `origin/<branch>` when reused).
    pub base_sha: String,
    pub remote: Remote,
}

impl Prepared {
    /// The Lead Branch was already on the remote and is continued (R35).
    pub fn reused(&self) -> bool {
        self.remote == Remote::Reusable
    }

    /// Removes the worktree and branch again.
    pub fn discard(&self, repo: &Path) -> Result<(), CpdError> {
        discard_worktree(repo, &self.path, &self.branch)
    }

    /// After `pre-start`: pushes a Lead Branch that is new on the remote,
    /// `push -u origin <branch>`. `Ok(true)`: pushed; `Ok(false)`: nothing to push
    /// (reused, or no `origin`); `Err`: the `push_failed` error, which never
    /// fails the caller's operation.
    pub fn push(&self) -> Result<bool, CpdError> {
        match self.remote {
            Remote::Absent => push_lead_branch(&self.path, &self.branch, true).map(|()| true),
            Remote::None | Remote::Reusable => Ok(false),
        }
    }
}

/// Creates a Lead worktree at `path` on `branch` and runs `pre-start` in it,
/// without pushing (contract §7.2; `plan lock`, engine E7 and `pr start` all
/// start a PR this way).
///
/// `repo` is the main checkout. A leftover of an interrupted start (a worktree
/// of this repo at `path`, on `branch`) is removed first; any other thing at
/// the path is `worktree_exists`, any other `branch` (or a local `cpd/<plan>`
/// under a `cpd/<plan>/<key>` branch, which would make it impossible) is
/// `branch_exists`. With an `origin`, the branch on the remote is reused when
/// it contains `origin/<base>`, refused `branch_exists` when it does not, and
/// cut from `origin/<base>` when absent (R35). On failure the worktree and
/// branch are removed.
pub fn prepare_lead_worktree(
    repo: &Path,
    plan: &PlanId,
    path: &Path,
    branch: &str,
    base: &str,
    setup: &Setup,
) -> Result<Prepared, StartFailure> {
    let at = |step| move |error| StartFailure { step, error };
    let in_worktree = at(Step::Worktree);

    let leftover = list(repo).map_err(in_worktree)?.iter().any(|w| real(&w.path) == real(path) && w.branch.as_deref() == Some(branch));
    if leftover {
        discard_worktree(repo, path, branch).map_err(in_worktree)?;
    }
    if path.exists() {
        let error = CpdError::new(ErrorCode::WorktreeExists, format!("{} already exists.", path.display()))
            .with("path", path.to_string_lossy().into_owned());
        return Err(in_worktree(error));
    }
    let mut clashes = vec![branch.to_owned()];
    if branch != lead_branch(plan) {
        clashes.push(lead_branch(plan));
    }
    for clash in clashes {
        if branch_exists(repo, &clash).map_err(in_worktree)? {
            let error = CpdError::new(ErrorCode::BranchExists, format!("Branch {clash} already exists.")).with("branch", clash);
            return Err(in_worktree(error));
        }
    }

    let remote = check_remote(repo, branch, base).map_err(at(Step::Remote))?;
    let start = match remote {
        Remote::None => base.to_owned(),
        Remote::Absent => format!("origin/{base}"),
        Remote::Reusable => format!("origin/{branch}"),
    };

    let created = (|| {
        ok(repo, &["worktree", "add", "-b", branch, path_str(path)?, &start])?;
        rev_parse(path, "HEAD")
    })();
    let base_sha = match created {
        Ok(sha) => sha,
        Err(e) => {
            let _ = discard_worktree(repo, path, branch);
            return Err(in_worktree(e));
        }
    };
    run_setup(repo, path, branch, base, setup).map_err(at(Step::PreStart))?;
    Ok(Prepared { path: path.to_path_buf(), branch: branch.to_owned(), base_sha, remote })
}

/// Creates the Lead worktree of a Plan's Lead Branch `cpd/<plan>` and runs
/// `pre-start` in it (contract §7.2 for a PR made by migration 3), then pushes
/// a new branch (R35). A failed push does not fail it: see [`Created`].
pub fn create_lead_worktree(repo: &Path, plan: &PlanId, base: &str, setup: &Setup) -> Result<Created, CpdError> {
    let path = lead_worktree_path(repo, plan)?;
    let prepared = prepare_lead_worktree(repo, plan, &path, &lead_branch(plan), base, setup).map_err(|f| f.error)?;
    // The remote is touched only after `pre-start` has passed.
    let push_failed = prepared.push().err();
    Ok(Created { path: prepared.path, branch: prepared.branch, base_sha: prepared.base_sha, push_failed })
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
