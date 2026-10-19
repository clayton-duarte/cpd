//! Running git (contract §7): every call is `git -C <dir> <argv…>` with
//! `GIT_TERMINAL_PROMPT=0`, the ref-lock timeouts of [`LOCK_TIMEOUTS`] and
//! stdin from `/dev/null`.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::json;

use super::{TAIL_LINES, tail};
use crate::error::{CpdError, ErrorCode};

/// The config every git call gets through `GIT_CONFIG_COUNT` (contract §7, hs-d243 G2):
/// a ref or `packed-refs` lock held by another git process is waited for up to
/// 10 s instead of git's 100 ms and 1 s, because cpd has concurrent writers of
/// shared refs such as `refs/remotes/origin/<base>`.
pub const LOCK_TIMEOUTS: [(&str, &str); 2] = [("core.filesRefLockTimeout", "10000"), ("core.packedRefsTimeout", "10000")];

/// What a finished git call printed.
#[derive(Clone, Debug)]
pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn argv(dir: &Path, args: &[&str]) -> Vec<String> {
    let mut all = vec!["git".to_owned(), "-C".to_owned(), dir.to_string_lossy().into_owned()];
    all.extend(args.iter().map(|a| (*a).to_owned()));
    all
}

pub(super) fn failed(dir: &Path, args: &[&str], message: String, exit: Option<i32>, stderr: &str) -> CpdError {
    CpdError::new(ErrorCode::GitFailed, message)
        .with("argv", json!(argv(dir, args)))
        .with("exit", json!(exit))
        .with("stderr", tail(stderr, TAIL_LINES))
}

/// Runs `git -C <dir> <args>` and returns what it printed, whatever its exit
/// code. Only a git that cannot start, or dies from a signal, is an error.
pub fn run(dir: &Path, args: &[&str]) -> Result<Out, CpdError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_CONFIG_COUNT", LOCK_TIMEOUTS.len().to_string());
    for (i, (key, value)) in LOCK_TIMEOUTS.iter().enumerate() {
        command.env(format!("GIT_CONFIG_KEY_{i}"), key).env(format!("GIT_CONFIG_VALUE_{i}"), value);
    }
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| failed(dir, args, format!("Cannot run git: {e}."), None, ""))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    match output.status.code() {
        Some(code) => Ok(Out { code, stdout, stderr }),
        None => Err(failed(dir, args, "git was killed by a signal.".to_owned(), None, &stderr)),
    }
}

/// Like [`run`], but any non-zero exit is `git_failed` (with `argv`, `exit` and
/// the last 60 lines of `stderr` in `details`). Returns stdout unchanged.
pub fn ok(dir: &Path, args: &[&str]) -> Result<String, CpdError> {
    let out = run(dir, args)?;
    if out.code == 0 {
        Ok(out.stdout)
    } else {
        let message = format!("git {} exited {}.", args.first().copied().unwrap_or(""), out.code);
        Err(failed(dir, args, message, Some(out.code), &out.stderr))
    }
}

/// How many times [`fetch`] runs a fetch that a concurrent `worktree add` broke.
const FETCH_ROUNDS: u32 = 20;

/// `git fetch <args>`, as [`ok`], retried while it fails on a concurrent
/// `git worktree add` (contract §7): that writes a placeholder all-zero
/// `.git/worktrees/<id>/HEAD` before the real one, and the fetch's
/// connectivity check, which walks every worktree's `HEAD`, dies on it with
/// `bad object worktrees/<id>/HEAD`. Only that failure is retried, up to 20
/// rounds 100–500 ms apart; any other is `git_failed` at once.
pub fn fetch(dir: &Path, args: &[&str]) -> Result<String, CpdError> {
    let all: Vec<&str> = std::iter::once("fetch").chain(args.iter().copied()).collect();
    let mut round = 1;
    loop {
        let out = run(dir, &all)?;
        if out.code == 0 {
            return Ok(out.stdout);
        }
        if round >= FETCH_ROUNDS || !out.stderr.contains("bad object worktrees/") {
            return Err(failed(dir, &all, format!("git fetch exited {}.", out.code), Some(out.code), &out.stderr));
        }
        round += 1;
        let jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() % 401);
        std::thread::sleep(std::time::Duration::from_millis(100 + u64::from(jitter)));
    }
}

/// `git rev-parse <rev>`, trimmed.
pub fn rev_parse(dir: &Path, rev: &str) -> Result<String, CpdError> {
    Ok(ok(dir, &["rev-parse", "--verify", "--end-of-options", rev])?.trim().to_owned())
}

/// Whether `refs/heads/<branch>` exists.
pub fn branch_exists(dir: &Path, branch: &str) -> Result<bool, CpdError> {
    let reference = format!("refs/heads/{branch}");
    let out = run(dir, &["rev-parse", "--verify", "--quiet", &reference])?;
    match out.code {
        0 => Ok(true),
        1 => Ok(false),
        code => Err(failed(
            dir,
            &["rev-parse", "--verify", "--quiet", &reference],
            format!("git rev-parse exited {code}."),
            Some(code),
            &out.stderr,
        )),
    }
}

/// Whether the repo `dir` belongs to has a remote called `origin` (R35, A3).
pub fn has_origin(dir: &Path) -> Result<bool, CpdError> {
    Ok(run(dir, &["remote", "get-url", "origin"])?.code == 0)
}

/// The checked-out branch of the worktree at `dir`; `None` when detached.
pub fn current_branch(dir: &Path) -> Result<Option<String>, CpdError> {
    let out = run(dir, &["symbolic-ref", "--short", "-q", "HEAD"])?;
    match out.code {
        0 => Ok(Some(out.stdout.trim().to_owned())),
        1 => Ok(None),
        code => Err(failed(
            dir,
            &["symbolic-ref", "--short", "-q", "HEAD"],
            format!("git symbolic-ref exited {code}."),
            Some(code),
            &out.stderr,
        )),
    }
}

/// Whether `origin` has `refs/heads/<branch>` (`git ls-remote --heads origin`); a failure is
/// `push_failed`, as the delete it guards.
pub fn remote_has_branch(dir: &Path, branch: &str) -> Result<bool, CpdError> {
    let reference = format!("refs/heads/{branch}");
    let out = run(dir, &["ls-remote", "--heads", "origin", &reference])?;
    if out.code != 0 {
        return Err(push_failed(dir, branch, &out.stderr));
    }
    Ok(out.stdout.lines().any(|line| line.split_whitespace().nth(1) == Some(reference.as_str())))
}

/// `git push origin --delete <branch>`; a failure is `push_failed` (`branch`, `repo`, `stderr`).
pub fn delete_remote_branch(dir: &Path, branch: &str) -> Result<(), CpdError> {
    let out = run(dir, &["push", "origin", "--delete", branch])?;
    if out.code != 0 {
        return Err(push_failed(dir, branch, &out.stderr));
    }
    Ok(())
}

fn push_failed(dir: &Path, branch: &str, stderr: &str) -> CpdError {
    CpdError::new(ErrorCode::PushFailed, format!("Cannot delete {branch} on origin of {}.", dir.display()))
        .with("branch", branch)
        .with("repo", dir.display().to_string())
        .with("stderr", tail(stderr, TAIL_LINES))
}
