//! Running git (contract §7): every call is `git -C <dir> <argv…>` with
//! `GIT_TERMINAL_PROMPT=0`, the ref-lock timeouts of [`LOCK_TIMEOUTS`] and
//! stdin from `/dev/null`, under a deadline.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;

use super::{TAIL_LINES, tail};
use crate::clock::Clock;
use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::host::runner::Runner;
use crate::ports::Ports;

/// A local git call's deadline (contract §7): a local repo answers in seconds.
const LOCAL_DEADLINE: Duration = Duration::from_secs(60);
/// A network call's deadline (`fetch`, `push`, `ls-remote`; contract §7): a slow
/// remote gets minutes, a stalled one (an ssh or TCP stall, which
/// `GIT_TERMINAL_PROMPT=0` does not stop) ends and releases the locks held across it.
const NETWORK_DEADLINE: Duration = Duration::from_secs(300);

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
/// code. Only a git that cannot start, dies from a signal (`git_failed`) or runs
/// past its deadline (`tool_timeout`) is an error.
pub fn run(dir: &Path, args: &[&str]) -> Result<Out, CpdError> {
    run_with(&*Ports::current().runner, dir, args)
}

/// [`run`] on `runner`.
pub fn run_with(runner: &dyn Runner, dir: &Path, args: &[&str]) -> Result<Out, CpdError> {
    let limit = match args.first() {
        Some(&("fetch" | "push" | "ls-remote")) => NETWORK_DEADLINE,
        _ => LOCAL_DEADLINE,
    };
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
    cmd.env("GIT_CONFIG_COUNT", LOCK_TIMEOUTS.len().to_string());
    for (i, (key, value)) in LOCK_TIMEOUTS.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), key).env(format!("GIT_CONFIG_VALUE_{i}"), value);
    }
    let output = runner.output(&mut cmd, "git", limit).map_err(|e| match e {
        Failed::Io(e) => failed(dir, args, format!("Cannot run git: {e}."), None, ""),
        Failed::Timeout(e) => e,
    })?;
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
    ok_with(&*Ports::current().runner, dir, args)
}

/// [`ok`] on `runner`.
pub fn ok_with(runner: &dyn Runner, dir: &Path, args: &[&str]) -> Result<String, CpdError> {
    let out = run_with(runner, dir, args)?;
    if out.code == 0 {
        Ok(out.stdout)
    } else {
        let message = format!("git {} exited {}.", args.first().copied().unwrap_or(""), out.code);
        Err(failed(dir, args, message, Some(out.code), &out.stderr))
    }
}

/// How many rounds a git call that hit a transient race of concurrent git calls gets (contract §7).
const RACE_ROUNDS: u32 = 20;

/// Runs `args` as [`ok`], again while it fails with stderr that `transient`
/// accepts: up to [`RACE_ROUNDS`] rounds, 100–500 ms apart, `between` run before
/// each new round. Any other failure, or the last round's, is `git_failed` at once.
fn retry_transient(
    dir: &Path,
    args: &[&str],
    transient: fn(&str) -> bool,
    between: impl FnMut(),
) -> Result<String, CpdError> {
    let ports = Ports::current();
    retry_transient_with(&*ports.runner, &*ports.clock, dir, args, transient, between)
}

/// [`retry_transient`] on `runner`, its naps on `clock`.
fn retry_transient_with(
    runner: &dyn Runner,
    clock: &dyn Clock,
    dir: &Path,
    args: &[&str],
    transient: fn(&str) -> bool,
    mut between: impl FnMut(),
) -> Result<String, CpdError> {
    let mut round = 1;
    loop {
        let out = run_with(runner, dir, args)?;
        if out.code == 0 {
            return Ok(out.stdout);
        }
        if round >= RACE_ROUNDS || !transient(&out.stderr) {
            let message = format!("git {} exited {}.", args.first().copied().unwrap_or(""), out.code);
            return Err(failed(dir, args, message, Some(out.code), &out.stderr));
        }
        round += 1;
        between();
        let jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() % 401);
        clock.sleep(Duration::from_millis(100 + u64::from(jitter)));
    }
}

/// Whether a failed `git fetch` died on a concurrent `git worktree add`: that
/// writes a placeholder all-zero `.git/worktrees/<id>/HEAD` before the real
/// one, and the fetch's connectivity check, which walks every worktree's
/// `HEAD`, dies on it with `bad object worktrees/<id>/HEAD`.
pub fn is_fetch_race(stderr: &str) -> bool {
    stderr.contains("bad object worktrees/")
}

/// Whether a failed `git worktree add` or `git worktree list` read another
/// cpd operation's half-written (being added) or half-removed `.git/worktrees/<id>/`: `fatal: failed to read
/// .git/worktrees/<id>/commondir: Success` (the file created, not yet written)
/// `...: Undefined error: 0` (macOS's text for strerror(0)) or `...: No such file or directory`.
pub fn is_admin_dir_race(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.starts_with("fatal: failed to read ")
            && line.contains("worktrees/")
            && (line.ends_with("/commondir: Success")
                || line.ends_with("/commondir: Undefined error: 0")
                || line.ends_with("/commondir: No such file or directory"))
    })
}

/// `git fetch <args>`, as [`ok`], retried while it fails on a concurrent
/// `git worktree add` ([`is_fetch_race`], contract §7).
pub fn fetch(dir: &Path, args: &[&str]) -> Result<String, CpdError> {
    let all: Vec<&str> = std::iter::once("fetch").chain(args.iter().copied()).collect();
    retry_transient(dir, &all, is_fetch_race, || {})
}

/// `git worktree list --porcelain`, as [`ok`], read again while it fails on a
/// half-written or half-removed admin dir ([`is_admin_dir_race`], contract §7).
pub fn worktree_list(dir: &Path) -> Result<String, CpdError> {
    let ports = Ports::current();
    worktree_list_with(&*ports.runner, &*ports.clock, dir)
}

/// [`worktree_list`] on `runner`, its naps on `clock`.
pub fn worktree_list_with(runner: &dyn Runner, clock: &dyn Clock, dir: &Path) -> Result<String, CpdError> {
    retry_transient_with(runner, clock, dir, &["worktree", "list", "--porcelain"], is_admin_dir_race, || {})
}

/// `git worktree add <args>`, as [`ok`], retried while it fails on a concurrent
/// `git worktree add` ([`is_admin_dir_race`], contract §7). `path` is the
/// worktree being added, and is the caller's own: a partial one the failed round left
/// is removed before the next.
pub fn worktree_add(dir: &Path, args: &[&str], path: &Path) -> Result<String, CpdError> {
    let all: Vec<&str> = ["worktree", "add"].into_iter().chain(args.iter().copied()).collect();
    retry_transient(dir, &all, is_admin_dir_race, || {
        let target = path.to_string_lossy();
        let _ = run(dir, &["worktree", "remove", "--force", "--force", &target]);
        let _ = std::fs::remove_dir_all(path);
    })
}

/// `git rev-parse <rev>`, trimmed.
pub fn rev_parse(dir: &Path, rev: &str) -> Result<String, CpdError> {
    rev_parse_with(&*Ports::current().runner, dir, rev)
}

/// [`rev_parse`] on `runner`.
pub fn rev_parse_with(runner: &dyn Runner, dir: &Path, rev: &str) -> Result<String, CpdError> {
    Ok(ok_with(runner, dir, &["rev-parse", "--verify", "--end-of-options", rev])?.trim().to_owned())
}

/// Whether `refs/heads/<branch>` exists.
pub fn branch_exists(dir: &Path, branch: &str) -> Result<bool, CpdError> {
    branch_exists_with(&*Ports::current().runner, dir, branch)
}

/// [`branch_exists`] on `runner`.
pub fn branch_exists_with(runner: &dyn Runner, dir: &Path, branch: &str) -> Result<bool, CpdError> {
    let reference = format!("refs/heads/{branch}");
    let out = run_with(runner, dir, &["rev-parse", "--verify", "--quiet", &reference])?;
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

/// The URL of the remote `origin` of the repo `dir` belongs to, as configured; `None` without one.
/// `git config` rather than `git remote get-url`: the latter applies `url.<base>.insteadOf`, so a
/// rewrite would key the repo by where git goes, not by the GitHub repo its `origin` names.
pub fn origin_url(dir: &Path) -> Result<Option<String>, CpdError> {
    let out = run(dir, &["config", "--get", "remote.origin.url"])?;
    Ok((out.code == 0).then(|| out.stdout.trim().to_owned()))
}

/// The checked-out branch of the worktree at `dir`; `None` when detached.
pub fn current_branch(dir: &Path) -> Result<Option<String>, CpdError> {
    current_branch_with(&*Ports::current().runner, dir)
}

/// [`current_branch`] on `runner`.
pub fn current_branch_with(runner: &dyn Runner, dir: &Path) -> Result<Option<String>, CpdError> {
    let out = run_with(runner, dir, &["symbolic-ref", "--short", "-q", "HEAD"])?;
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

#[cfg(test)]
mod tests {
    #[test]
    fn is_admin_dir_race_matches_the_linux_and_macos_text_of_strerror_zero() {
        for tail in ["Success", "Undefined error: 0"] {
            let line = format!("fatal: failed to read .git/worktrees/x/commondir: {tail}");
            assert!(super::is_admin_dir_race(&line), "{line}");
        }
    }

    use super::{is_fetch_race, is_admin_dir_race};

    #[test]
    fn worktree_add_retries_the_reproduced_shapes_only() {
        for stderr in [
            "Preparing worktree (detached HEAD f094ea8)\nfatal: failed to read .git/worktrees/pay.gate-7/commondir: Success\n",
            "fatal: failed to read .git/worktrees/w3/commondir: No such file or directory",
        ] {
            assert!(is_admin_dir_race(stderr), "{stderr}");
        }
        for stderr in [
            "fatal: '/x/w' already exists",
            "fatal: invalid reference: nosuch",
            "fatal: failed to read .git/worktrees/w3/commondir: Permission denied",
            "fatal: failed to read .git/index: Success",
            "",
        ] {
            assert!(!is_admin_dir_race(stderr), "{stderr}");
        }
    }

    #[test]
    fn fetch_retries_only_the_placeholder_head() {
        assert!(is_fetch_race("fatal: bad object worktrees/adding/HEAD"));
        assert!(!is_fetch_race("fatal: couldn't find remote ref no-such-branch"));
    }
}
