//! Worktrees and branches (contract §7.1, §7.2).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::hooks::{HookOutcome, Operation, Pipeline, run_hook};
use super::run::{branch_exists, fetch, has_origin, ok, ok_with, rev_parse, run, run_with, worktree_add, worktree_list_with};
use super::{TAIL_LINES, tail};
use crate::clock::Clock;
use crate::error::{CpdError, ErrorCode};
use crate::home::Hook;
use crate::host::runner::Runner;
use crate::ports::Ports;
use crate::ids::{AttemptId, PlanId, PrKey};

/// The LEGACY Lead Branch of a Plan: `cpd/<plan>` (a PR made before R34 keeps it; a new PR's is `<plan>/<key>`).
pub fn legacy_lead_branch(plan: &PlanId) -> String {
    format!("cpd/{plan}")
}

/// The branch of an Attempt: `cpd/<plan>--<attempt>`.
pub fn attempt_branch(plan: &PlanId, attempt: &AttemptId) -> String {
    format!("cpd/{plan}--{attempt}")
}

/// Whether an Attempt is **linear** (hs-d680, hs-d682): it runs in its PR's one workflow worktree, on the Lead
/// Branch, so its branch is the Lead Branch. It is its PR's only live Attempt for its whole life; a parallel
/// Attempt has its own `cpd/<plan>--<attempt>` worktree and branch.
pub fn is_linear(attempt_branch: &str, lead_branch: &str) -> bool {
    attempt_branch == lead_branch
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
/// A PR made by migration 4 keeps the Lead Branch `cpd/<plan>` and with it the
/// worktree `<repo-parent>/<repo-name>.<plan>` it had.
pub fn pr_lead_worktree_path(repo: &Path, plan: &PlanId, key: &PrKey, lead_branch_name: &str) -> Result<PathBuf, CpdError> {
    if lead_branch_name == legacy_lead_branch(plan) {
        lead_worktree_path(repo, plan)
    } else {
        sibling(repo, &format!("{plan}.{key}"))
    }
}

/// The Attempt worktree: `<repo-parent>/<repo-name>.<plan>--<attempt>`.
pub fn attempt_worktree_path(repo: &Path, plan: &PlanId, attempt: &AttemptId) -> Result<PathBuf, CpdError> {
    sibling(repo, &format!("{plan}--{attempt}"))
}

/// The Gate worktree for a PR's process: `<repo-parent>/<repo-name>.<plan>--<key>-gate-<pid>`.
pub fn gate_worktree_path(repo: &Path, plan: &PlanId, key: &PrKey, pid: u32) -> Result<PathBuf, CpdError> {
    sibling(repo, &format!("{plan}--{key}-gate-{pid}"))
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
    let ports = Ports::current();
    list_with(&*ports.runner, &*ports.clock, dir)
}

/// [`list`] on `runner`, the naps of its re-reads on `clock`.
pub fn list_with(runner: &dyn Runner, clock: &dyn Clock, dir: &Path) -> Result<Vec<Worktree>, CpdError> {
    let text = worktree_list_with(runner, clock, dir)?;
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
    let ports = Ports::current();
    worktree_of_branch_with(&*ports.runner, &*ports.clock, dir, branch)
}

/// [`worktree_of_branch`] on `runner`, its naps on `clock`.
pub fn worktree_of_branch_with(runner: &dyn Runner, clock: &dyn Clock, dir: &Path, branch: &str) -> Result<Option<PathBuf>, CpdError> {
    Ok(list_with(runner, clock, dir)?.into_iter().find(|w| w.branch.as_deref() == Some(branch)).map(|w| w.path))
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

/// Runs `pre_start` (the policy's `hooks.pre-start`) in `path`; on any failure removes the worktree and branch
/// and returns `setup_failed` (contract §7.2).
fn run_setup(
    pre_start: &Pipeline,
    dir: &Path,
    path: &Path,
    branch: &str,
    base_branch: &str,
    setup: &Setup,
) -> Result<(), CpdError> {
    let op = Operation { branch: base_branch };
    let result = run_hook(Hook::PreStart, pre_start, path, Some(&op), setup.timeout, &setup.log);
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
/// origin <branch>`, a plain push, never forced and never `-u` (contract §7.2,
/// §7.4, R35, hs-d243 G1).
/// `Ok(())` also when the repo has no `origin` (A3): nothing to push to. A
/// rejected or failed push is the returned `push_failed` error with git's
/// stderr; it never fails the caller's operation.
pub fn push_lead_branch(lead_worktree: &Path, branch: &str) -> Result<(), CpdError> {
    let pushed = has_origin(lead_worktree)
        .and_then(|origin| if origin { run(lead_worktree, &["push", "origin", branch]).map(Some) } else { Ok(None) });
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

/// What a push by rule P did (contract §7.4, hs-d243).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Push {
    /// `origin` has `sha` as the branch's tip: `new` when this push moved it,
    /// `false` when it was there already.
    Pushed { new: bool },
    /// Rejected as not a fast-forward, but the remote tip `remote` already
    /// contains `sha` (a later merge pushed a descendant): a success that pushed nothing.
    Contained { remote: String },
    /// Rejected as not a fast-forward, and the remote tip does not contain
    /// `sha` (or is not in the local object store). The caller reports it as
    /// its contract says (`push_failed`, `push_rejected` or `git_failed`).
    Rejected { stderr: String },
}

/// Rule P (contract §7.4): `git -C <lead worktree> push --porcelain origin
/// <sha>:refs/heads/<branch>`, a plain push, never `-u`, so that it publishes
/// the commit it means, not whatever the ref holds at that moment. A rejection
/// as not a fast-forward is checked against the remote tip `X` (`ls-remote`):
/// when `X` is in the local object store and contains `sha`, the push is
/// [`Push::Contained`]. Any other failure (no `origin` included) is `git_failed`.
pub fn push_commit(lead_worktree: &Path, branch: &str, sha: &str) -> Result<Push, CpdError> {
    push_commit_with(&*Ports::current().runner, lead_worktree, branch, sha)
}

/// [`push_commit`] on `runner` (a test passes a `FakeRunner`).
pub fn push_commit_with(runner: &dyn Runner, lead_worktree: &Path, branch: &str, sha: &str) -> Result<Push, CpdError> {
    let (run, ok) = (|dir: &Path, args: &[&str]| run_with(runner, dir, args), |dir: &Path, args: &[&str]| ok_with(runner, dir, args));
    let reference = format!("refs/heads/{branch}");
    let refspec = format!("{sha}:{reference}");
    let args = ["push", "--porcelain", "origin", refspec.as_str()];
    let out = run(lead_worktree, &args)?;
    let line = ref_line(&out.stdout);
    if out.code == 0 {
        return Ok(Push::Pushed { new: line.and_then(|l| l.chars().next()) != Some('=') });
    }
    if !line.is_some_and(|l| l.starts_with('!') && (l.contains("(non-fast-forward)") || l.contains("(fetch first)"))) {
        return Err(super::run::failed(lead_worktree, &args, format!("git push exited {}.", out.code), Some(out.code), &out.stderr));
    }
    let listed = ok(lead_worktree, &["ls-remote", "origin", &reference])?;
    let remote = listed.lines().find(|l| l.split_whitespace().nth(1) == Some(reference.as_str())).and_then(|l| l.split_whitespace().next());
    if let Some(remote) = remote {
        let known = run(lead_worktree, &["cat-file", "-e", &format!("{remote}^{{commit}}")])?.code == 0;
        if known && run(lead_worktree, &["merge-base", "--is-ancestor", sha, remote])?.code == 0 {
            return Ok(Push::Contained { remote: remote.to_owned() });
        }
    }
    Ok(Push::Rejected { stderr: tail(&out.stderr, TAIL_LINES) })
}

/// The line of `push --porcelain` for the pushed ref: `<flag>\t<from>:<to>\t<summary>`,
/// the flag `=` up to date, ` ` fast-forward, `*` new, `!` rejected.
fn ref_line(stdout: &str) -> Option<&str> {
    stdout.lines().find(|l| l.chars().nth(1) == Some('\t'))
}

/// What the remote has of the Lead Branch (contract §7.2, R35).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
    /// No `origin`: no remote check (A3).
    None,
    /// `origin` has no such branch: create it and push it.
    Absent,
    /// `origin/<branch>` exists and does not conflict with `origin/<base>`.
    Reusable,
    /// An adopted PR's Lead Branch: cut from `origin/<head_ref>`, which is the pull request's own branch; nothing is pushed.
    Adopted,
}

/// Whether `origin` has branch `name`: `ls-remote --exit-code --heads` exits 0 (it has) or 2 (it has not);
/// any other result is `setup_failed`.
fn on_remote(repo: &Path, name: &str) -> Result<bool, CpdError> {
    let listed = run(repo, &["ls-remote", "--exit-code", "--heads", "origin", name])?;
    match listed.code {
        0 => Ok(true),
        2 => Ok(false),
        code => Err(CpdError::new(ErrorCode::SetupFailed, format!("Cannot tell whether {name} is on origin: git ls-remote exited {code}."))
            .with("exit", code)
            .with("stderr", tail(&listed.stderr, TAIL_LINES))),
    }
}

/// `git merge-tree --write-tree --name-only <onto> <sha>` (contract §7.6 `pr open` step 1, `pr rebase` step 3, §7.2):
/// `Some(files)`, the conflicted paths, when `sha` conflicts with `onto`; `None` when they merge cleanly, whether or not
/// `sha` contains `onto`; any other result is `git_failed`. One call, no worktree.
pub fn merge_conflicts(repo: &Path, onto: &str, sha: &str) -> Result<Option<Vec<String>>, CpdError> {
    merge_conflicts_with(&*Ports::current().runner, repo, onto, sha)
}

/// [`merge_conflicts`] on `runner` (a test passes a `FakeRunner`).
pub fn merge_conflicts_with(runner: &dyn Runner, repo: &Path, onto: &str, sha: &str) -> Result<Option<Vec<String>>, CpdError> {
    let merge = run_with(runner, repo, &["merge-tree", "--write-tree", "--name-only", onto, sha])?;
    match merge.code {
        0 => Ok(None),
        // The tree's id, the conflicted files, a blank line, then git's messages.
        1 => Ok(Some(merge.stdout.lines().skip(1).take_while(|l| !l.is_empty()).map(str::to_owned).collect())),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-tree exited {code}."))
            .with("exit", code)
            .with("stderr", tail(&merge.stderr, TAIL_LINES))),
    }
}

/// Fetches `origin/<branch>` and refuses `branch_exists` when it conflicts with `origin/<base>` (hs-d490: a base that
/// merely moved ahead of the branch is no conflict); `details.files` are the conflicted paths.
fn require_no_conflict(repo: &Path, branch: &str, base: &str) -> Result<(), CpdError> {
    fetch(repo, &["origin", branch])?;
    let remote_branch = format!("origin/{branch}");
    let remote_base = format!("origin/{base}");
    match merge_conflicts(repo, &remote_base, &remote_branch)? {
        None => Ok(()),
        Some(files) => Err(CpdError::new(
            ErrorCode::BranchExists,
            format!("Branch {branch} already exists on origin and conflicts with {remote_base} in {}: cpd does not reset or force-push it.", files.join(", ")),
        )
        .with("branch", branch)
        .with("remote", remote_branch)
        .with("base", remote_base)
        .with("files", files)),
    }
}

/// `fetch origin <base>` and the remote check of the Lead Branch. Refuses
/// `branch_exists` when `origin/<branch>` conflicts with `origin/<base>`.
fn check_remote(repo: &Path, branch: &str, base: &str) -> Result<Remote, CpdError> {
    if !has_origin(repo)? {
        return Ok(Remote::None);
    }
    fetch(repo, &["origin", base])?;
    if !on_remote(repo, branch)? {
        return Ok(Remote::Absent);
    }
    require_no_conflict(repo, branch, base)?;
    Ok(Remote::Reusable)
}

/// The remote check and the start point of a **stacked** PR's Lead Branch (contract §7.2 "A stacked PR"): `base`
/// is its base PR's Lead Branch, and the branch starts from that branch's head `S`, the local `refs/heads/<base>`
/// (which the base PR's merges move), else `origin/<base>` when the local branch is gone, else `setup_failed`
/// (`step` `base_pr_branch`). `origin/<base>` is fetched only when `origin` has it; a Lead Branch on the remote is
/// reused when it does not conflict with `origin/<base>`, or at once when `origin` has no `base` (nothing to be behind).
fn check_stacked(repo: &Path, branch: &str, base: &str) -> Result<(Remote, String), StartFailure> {
    let at = |step| move |error| StartFailure { step, error };
    let origin = has_origin(repo).map_err(at(Step::Remote))?;
    let base_on_remote = if origin {
        let found = on_remote(repo, base).map_err(at(Step::Remote))?;
        if found {
            fetch(repo, &["origin", base]).map_err(at(Step::Remote))?;
        }
        found
    } else {
        false
    };
    let local = format!("refs/heads/{base}");
    let start = if run(repo, &["rev-parse", "--verify", "--quiet", &local]).map_err(at(Step::BasePrBranch))?.code == 0 {
        rev_parse(repo, &local).map_err(at(Step::BasePrBranch))?
    } else if base_on_remote {
        rev_parse(repo, &format!("origin/{base}")).map_err(at(Step::BasePrBranch))?
    } else {
        let error = CpdError::new(
            ErrorCode::SetupFailed,
            format!("The base PR's Lead Branch {base} is neither a local branch of this repo nor on origin: the stacked PR has nothing to start from."),
        )
        .with("base", base);
        return Err(StartFailure { step: Step::BasePrBranch, error });
    };
    if !origin {
        return Ok((Remote::None, start));
    }
    if !on_remote(repo, branch).map_err(at(Step::Remote))? {
        return Ok((Remote::Absent, start));
    }
    if base_on_remote {
        require_no_conflict(repo, branch, base).map_err(at(Step::Remote))?;
    } else {
        fetch(repo, &["origin", branch]).map_err(at(Step::Remote))?;
    }
    Ok((Remote::Reusable, format!("origin/{branch}")))
}

/// The part of a Lead worktree's start that failed (contract §3.8 "A failed
/// start": `step` of `pr_start_failed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The path, the local branches, `git worktree add`.
    Worktree,
    /// `fetch origin`, `ls-remote`, the ancestor check.
    Remote,
    /// A stacked PR's start point: its base PR's Lead Branch, local or on `origin` (§7.2).
    BasePrBranch,
    /// The `pre-start` hook.
    PreStart,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Worktree => "worktree",
            Step::Remote => "remote",
            Step::BasePrBranch => "base_pr_branch",
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

/// The temporary worktree `pr rebase` rewrites the Lead Branch in (contract §7.6): detached at the
/// snapshot it rebases, so the Lead worktree itself is never rewritten and two rebases of one PR do
/// not race on its index. It belongs to one rebase run: no test slot, no clean-up sweep, and the
/// run that adds it removes it.
#[derive(Debug)]
pub struct RebaseWorktree {
    repo: PathBuf,
    path: PathBuf,
}

impl RebaseWorktree {
    /// `git -C <repo> worktree add --detach --lock --reason "cpd rebase <pid>" W <sha>`, `W` the
    /// rebase worktree of this process's pid.
    pub fn add(repo: &Path, plan: &PlanId, key: &PrKey, sha: &str) -> Result<RebaseWorktree, CpdError> {
        let pid = std::process::id();
        let path = sibling(repo, &format!("{plan}.{key}.rebase-{pid}"))?;
        let reason = format!("cpd rebase {pid}");
        let target = path.to_string_lossy().into_owned();
        worktree_add(repo, &["--detach", "--lock", "--reason", &reason, &target, sha], &path)?;
        Ok(RebaseWorktree { repo: repo.to_path_buf(), path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `git -C <repo> worktree remove --force --force W`.
    pub fn remove(self) -> Result<(), CpdError> {
        let path = self.path.to_string_lossy();
        ok(&self.repo, &["worktree", "remove", "--force", "--force", &path]).map(|_| ())
    }
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
    /// `push origin <branch>` (no `-u`, hs-d243 G1). `Ok(true)`: pushed; `Ok(false)`: nothing to push
    /// (reused, or no `origin`); `Err`: the `push_failed` error, which never
    /// fails the caller's operation.
    pub fn push(&self) -> Result<bool, CpdError> {
        match self.remote {
            Remote::Absent => push_lead_branch(&self.path, &self.branch).map(|()| true),
            Remote::None | Remote::Reusable | Remote::Adopted => Ok(false),
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
/// it does not conflict with `origin/<base>`, refused `branch_exists` when it does, and
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
    prepare_pr_lead_worktree(repo, plan, path, branch, base, Cut::Base, setup, &Pipeline::default())
}

/// [`prepare_lead_worktree`] of a **stacked** PR: `base` is its base PR's Lead Branch, and the branch is cut from
/// that branch's head, not from `origin/<base>` (contract §7.2 "A stacked PR", [`check_stacked`]); everything else
/// is the same.
pub fn prepare_stacked_lead_worktree(
    repo: &Path,
    plan: &PlanId,
    path: &Path,
    branch: &str,
    base: &str,
    setup: &Setup,
) -> Result<Prepared, StartFailure> {
    prepare_pr_lead_worktree(repo, plan, path, branch, base, Cut::Stacked, setup, &Pipeline::default())
}

/// Where a Lead Branch is cut from: `origin/<base>` (or the branch already on the remote), a stacked PR's base PR's
/// Lead Branch, or, for an **adopted** PR (contract §7.2), `origin/<head_ref>`: `origin <head_ref>` is fetched (a failure
/// is step `remote`), no conflict check against `base` runs, nothing is pushed, and the User's own local branch of the
/// head's name is never read.
#[derive(Clone, Copy)]
pub enum Cut<'a> {
    Base,
    Stacked,
    Adopted(&'a str),
}

/// [`prepare_lead_worktree`] (or [`prepare_stacked_lead_worktree`], for `Cut::Stacked`; `Cut::Adopted` for an adopted PR) running `pre_start`, the PR's policy
/// `hooks.pre-start` (§7.3), after the worktree is created. The two above run no hook: they are for a caller with none.
#[allow(clippy::too_many_arguments)]
pub fn prepare_pr_lead_worktree(
    repo: &Path,
    plan: &PlanId,
    path: &Path,
    branch: &str,
    base: &str,
    cut: Cut<'_>,
    setup: &Setup,
    pre_start: &Pipeline,
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
    if branch != legacy_lead_branch(plan)
        && let Some((parent, _)) = branch.rsplit_once('/')
    {
        clashes.push(parent.to_owned());
    }
    for clash in clashes {
        if branch_exists(repo, &clash).map_err(in_worktree)? {
            let error = CpdError::new(ErrorCode::BranchExists, format!("Branch {clash} already exists.")).with("branch", clash);
            return Err(in_worktree(error));
        }
    }

    let (remote, start) = if let Cut::Adopted(head_ref) = cut {
        fetch(repo, &["origin", head_ref]).map_err(at(Step::Remote))?;
        (Remote::Adopted, format!("origin/{head_ref}"))
    } else if matches!(cut, Cut::Stacked) {
        check_stacked(repo, branch, base)?
    } else {
        let remote = check_remote(repo, branch, base).map_err(at(Step::Remote))?;
        let start = match remote {
            Remote::None => base.to_owned(),
            Remote::Absent => format!("origin/{base}"),
            // `check_remote` never answers `Adopted`: that start is cut above.
            Remote::Reusable | Remote::Adopted => format!("origin/{branch}"),
        };
        (remote, start)
    };

    let created = (|| {
        // `--no-track`: no upstream, so nothing writes the shared `.git/config` (hs-d243 G1).
        ok(repo, &["worktree", "add", "--no-track", "-b", branch, path_str(path)?, &start])?;
        rev_parse(path, "HEAD")
    })();
    let base_sha = match created {
        Ok(sha) => sha,
        Err(e) => {
            let _ = discard_worktree(repo, path, branch);
            return Err(in_worktree(e));
        }
    };
    run_setup(pre_start, repo, path, branch, base, setup).map_err(at(Step::PreStart))?;
    Ok(Prepared { path: path.to_path_buf(), branch: branch.to_owned(), base_sha, remote })
}

/// Creates the Lead worktree of a Plan's legacy Lead Branch `cpd/<plan>` and runs
/// `pre-start` in it (contract §7.2 for a PR made by migration 4), then pushes
/// a new branch (R35). A failed push does not fail it: see [`Created`].
pub fn create_lead_worktree(repo: &Path, plan: &PlanId, base: &str, setup: &Setup) -> Result<Created, CpdError> {
    create_lead_worktree_with(repo, plan, base, setup, &Pipeline::default())
}

/// [`create_lead_worktree`] running `pre_start`, the policy's `hooks.pre-start`; the one above runs no hook.
pub fn create_lead_worktree_with(repo: &Path, plan: &PlanId, base: &str, setup: &Setup, pre_start: &Pipeline) -> Result<Created, CpdError> {
    let path = lead_worktree_path(repo, plan)?;
    let prepared = prepare_pr_lead_worktree(repo, plan, &path, &legacy_lead_branch(plan), base, Cut::Base, setup, pre_start).map_err(|f| f.error)?;
    // The remote is touched only after `pre-start` has passed.
    let push_failed = prepared.push().err();
    Ok(Created { path: prepared.path, branch: prepared.branch, base_sha: prepared.base_sha, push_failed })
}

/// The Lead Branch of the PR whose Lead worktree is `lead_worktree`: the branch checked out there, whatever its
/// name (the stored lead_branch is the caller's). A detached HEAD is `setup_failed`.
fn checked_out_lead_branch(lead_worktree: &Path, plan: &PlanId) -> Result<String, CpdError> {
    let fail = |why: String| {
        CpdError::new(ErrorCode::SetupFailed, format!("The Lead worktree {} has no Lead Branch of Plan {plan}: {why}.", lead_worktree.display()))
            .with("lead_worktree", lead_worktree.to_string_lossy().into_owned())
    };
    match super::run::current_branch(lead_worktree)? {
        Some(branch) => Ok(branch),
        None => Err(fail("its HEAD is detached".to_owned())),
    }
}

/// Creates an Attempt worktree from the Lead Branch HEAD (the branch checked
/// out in `lead_worktree`) and runs `pre-start` in it, blocking (contract §7.2, A2). On failure the worktree and branch are
/// removed; the driver ends the Attempt `failed` / `setup_failed`.
pub fn create_attempt_worktree(
    repo: &Path,
    lead_worktree: &Path,
    plan: &PlanId,
    attempt: &AttemptId,
    setup: &Setup,
) -> Result<Created, CpdError> {
    create_attempt_worktree_with(repo, lead_worktree, plan, attempt, setup, &Pipeline::default())
}

/// [`create_attempt_worktree`] running `pre_start`, the PR's policy `hooks.pre-start`; the one above runs no hook.
pub fn create_attempt_worktree_with(
    repo: &Path,
    lead_worktree: &Path,
    plan: &PlanId,
    attempt: &AttemptId,
    setup: &Setup,
    pre_start: &Pipeline,
) -> Result<Created, CpdError> {
    let path = attempt_worktree_path(repo, plan, attempt)?;
    let lead = checked_out_lead_branch(lead_worktree, plan)?;
    let branch = attempt_branch(plan, attempt);

    let base_sha = rev_parse(lead_worktree, &format!("refs/heads/{lead}"))?;
    ok(lead_worktree, &["worktree", "add", "-b", &branch, path_str(&path)?, &base_sha])?;
    run_setup(pre_start, lead_worktree, &path, &branch, &lead, setup)?;
    Ok(Created { path, branch, base_sha, push_failed: None })
}

/// Removes a worktree and its branch whatever state they are in:
/// `git worktree remove --force`, `git branch -D`. Used after a failed setup
/// and when a Plan is `done` (contract §7.2, commands.md §9). Idempotent: what
/// is already gone is skipped. Run `dir` in the main repo or another worktree.
pub fn discard_worktree(dir: &Path, path: &Path, branch: &str) -> Result<(), CpdError> {
    remove(dir, path, branch, true, true)
}

/// Removes the worktree and branch of a Merged Attempt (contract §7.2, A9):
/// `git worktree remove` then `git branch -d`; git refuses the `-d` unless the
/// branch is merged into HEAD of `lead_worktree`. Idempotent for what is already gone.
pub fn remove_merged_attempt(lead_worktree: &Path, path: &Path, branch: &str) -> Result<(), CpdError> {
    remove(lead_worktree, path, branch, false, false)
}

/// Why a merged PR's Lead worktree was left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
    /// `git status --porcelain` is not empty (untracked files included).
    Dirty,
    /// `git rev-list HEAD --not --remotes=origin` is not empty, and `HEAD` is not the pull request's recorded head or
    /// an ancestor of it.
    Unpushed,
    /// The rename of `<path>` to `<path>.removing` failed: the error is the reason (DESIGN 4 (a)). Stub: nothing
    /// returns it yet.
    Rename(String),
}

impl Kept {
    pub fn as_str(&self) -> &str {
        match self {
            Kept::Dirty => "dirty",
            Kept::Unpushed => "unpushed",
            Kept::Rename(reason) => reason,
        }
    }
}

/// What [`remove_merged_lead`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeadRemoval {
    /// The worktree and the local Lead Branch are gone (or were).
    Removed,
    /// Nothing was touched.
    Kept(Kept),
}

/// Removes the Lead worktree of a merged or cancelled PR and its local Lead Branch, whole or not at all (DESIGN 4). A
/// worktree with uncommitted changes or commits not on `origin` is kept untouched, unless `HEAD` is the pull request's
/// head commit or an ancestor of it (`recorded_head`, called only when `rev-list` is not empty (merge commits do not
/// count, hygiene.md 8a) and `None` when GitHub's answer is unavailable; after a squash merge the head branch is
/// deleted and `origin/<branch>` pruned, so the PR's own commits look unpushed); the check is
/// `git --no-optional-locks status`, which takes no `index.lock` (hs-d243). Otherwise, in this order: (a) rename
/// `<path>` to `<path>.removing` (atomic; a failing rename leaves everything untouched and is `Kept(Rename)`), (b)
/// `git worktree prune`, (c) `git branch -D` when the branch exists, (d) delete `<path>.removing`. A stop leaves the
/// worktree whole (before a) or `<path>` absent with `<path>.removing` as the marker. A call that finds `<path>` absent
/// runs (b), (c), (d) again; every step is idempotent.
pub fn remove_merged_lead(
    repo: &Path,
    path: &Path,
    branch: &str,
    recorded_head: impl FnOnce() -> Option<String>,
) -> Result<LeadRemoval, CpdError> {
    let mut name = path.file_name().ok_or_else(|| CpdError::new(ErrorCode::GitFailed, format!("{} has no file name", path.display())))?.to_os_string();
    name.push(".removing");
    let aside = path.with_file_name(name);
    if path.exists() {
        if !ok(path, &["--no-optional-locks", "status", "--porcelain"])?.trim().is_empty() {
            return Ok(LeadRemoval::Kept(Kept::Dirty));
        }
        if has_unpushed_work(path, recorded_head)? {
            return Ok(LeadRemoval::Kept(Kept::Unpushed));
        }
        if let Err(e) = std::fs::rename(path, &aside) {
            return Ok(LeadRemoval::Kept(Kept::Rename(e.to_string())));
        }
    }
    ok(repo, &["worktree", "prune"])?;
    if branch_exists(repo, branch)? {
        ok(repo, &["branch", "-D", branch])?;
    }
    if aside.exists() {
        std::fs::remove_dir_all(&aside).map_err(|e| CpdError::new(ErrorCode::GitFailed, format!("remove {}: {e}", aside.display())))?;
    }
    Ok(LeadRemoval::Removed)
}

/// Whether `HEAD` of `worktree` holds work that is nowhere else: non-merge commits outside `origin/*` and outside the
/// pull request's recorded head `oid` (contract §7.2 "The recorded head"; no `oid`, or an object this repo does not
/// have, excludes nothing). A merge commit is not work: a lone local merge of the base (rule S) is no reason to keep a
/// merged PR's worktree (hygiene.md 8a).
fn has_unpushed_work(worktree: &Path, recorded_head: impl FnOnce() -> Option<String>) -> Result<bool, CpdError> {
    let list = |extra: Option<&str>| -> Result<bool, CpdError> {
        let mut args = vec!["rev-list", "--no-merges", "HEAD", "--not", "--remotes=origin"];
        args.extend(extra);
        Ok(!ok(worktree, &args)?.trim().is_empty())
    };
    if !list(None)? {
        return Ok(false);
    }
    let Some(oid) = recorded_head() else { return Ok(true) };
    if run(worktree, &["cat-file", "-e", &format!("{oid}^{{commit}}")])?.code != 0 {
        return Ok(true);
    }
    list(Some(&oid))
}

/// `force` is `--force` on the worktree; `delete` is `branch -D` rather than `-d`.
fn remove(dir: &Path, path: &Path, branch: &str, force: bool, delete: bool) -> Result<(), CpdError> {
    if list(dir)?.iter().any(|w| real(&w.path) == real(path)) {
        let path = path_str(path)?;
        if force {
            ok(dir, &["worktree", "remove", "--force", path])?;
        } else {
            ok(dir, &["worktree", "remove", path])?;
        }
    }
    if branch_exists(dir, branch)? {
        ok(dir, &["branch", if delete { "-D" } else { "-d" }, branch])?;
    }
    Ok(())
}
