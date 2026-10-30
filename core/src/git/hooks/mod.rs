//! The hooks of the PR's policy `hooks` domain (contract §7.3): `pre-start` (setup),
//! `pre-commit` (the quick check) and `pre-merge` (the Gate), given as the
//! hook's [`Pipeline`], rendered, run, and their output captured.
//!
//! What a result means (`setup_failed`, `check_failed`, `gate_failed`, …) is the
//! caller's: [`HookOutcome`] only says how the hook ended.

mod def;
mod exec;
#[cfg(test)]
mod exec_runner_tests;
mod template;

use std::fmt;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::run::{branch_exists_with, current_branch_with, ok_with, rev_parse_with, run_with};
use super::worktree::{list_with, worktree_of_branch_with};
use super::TAIL_LINES;
use crate::error::CpdError;
use crate::home::Hook;
use crate::host::runner::Runner;
use crate::ports::Ports;

pub use def::{HookCommand, Pipeline, parse_value};
pub use exec::is_withheld;
pub use template::{shell_escape, uses as template_uses};

/// Why a hook could not be judged (contract §7.3, the last row of the result table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookError {
    /// `hook_invalid`: a template does not
    /// render (an undefined variable, an unknown filter); nothing ran.
    Invalid(String),
    /// Killed at the timeout.
    Timeout,
    /// A command was killed by this signal.
    Signal(i32),
    /// `sh` could not start, or a file could not be read or written.
    Io(String),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::Invalid(why) => write!(f, "hook_invalid: {why}"),
            HookError::Timeout => f.write_str("killed at the timeout"),
            HookError::Signal(n) => write!(f, "killed by signal {n}"),
            HookError::Io(why) => f.write_str(why),
        }
    }
}

/// How a hook ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookOutcome {
    /// Exit 0, or no commands.
    Passed,
    /// The first failing command's exit code, in file order.
    Failed { exit: i32, command: String },
    Error(HookError),
}

impl fmt::Display for HookOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookOutcome::Passed => f.write_str("passed"),
            HookOutcome::Failed { exit, .. } => write!(f, "exit {exit}"),
            HookOutcome::Error(e) => e.fmt(f),
        }
    }
}

/// The result of running a hook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookResult {
    pub outcome: HookOutcome,
    /// The last 60 lines of the output; `None` when no command ran (no file, no
    /// table, or `hook_invalid`), so a caller's `check_tail` stays null.
    pub tail: Option<String>,
}

/// The other branch of the operation a hook runs for: `base` for `pre-start`
/// (the Plan's base for a Lead worktree, the Lead Branch for an Attempt's),
/// `target` for `pre-commit` (the Lead Branch) and `pre-merge` (the Plan's
/// base). Its `*_worktree_path` is the worktree that has it checked out;
/// undefined when none has.
#[derive(Clone, Copy, Debug)]
pub struct Operation<'a> {
    pub branch: &'a str,
}

/// Renders and runs the commands of `pipeline` (the policy's `hooks.<hook>`)
/// in `worktree`, and writes the output to `log` as it arrives.
///
/// `operation` is needed by every hook cpd runs; without it the `base` and
/// `target` variables are undefined. `timeout` is `setup_minutes` for `pre-start`,
/// `gate_minutes` for the others. `Err` is a git call that failed while building
/// the template variables.
pub fn run_hook(
    hook: Hook,
    pipeline: &Pipeline,
    worktree: &Path,
    operation: Option<&Operation<'_>>,
    timeout: Duration,
    log: &Path,
) -> Result<HookResult, CpdError> {
    run_hook_with(&Ports::current(), hook, pipeline, worktree, operation, timeout, log)
}

/// [`run_hook`] on `ports`: the git calls that build the template variables and the hooks themselves go through
/// `ports.runner`, the time of the timeout and of the git re-reads through `ports.clock`.
pub fn run_hook_with(
    ports: &Ports,
    hook: Hook,
    pipeline: &Pipeline,
    worktree: &Path,
    operation: Option<&Operation<'_>>,
    timeout: Duration,
    log: &Path,
) -> Result<HookResult, CpdError> {
    run_with_branch(ports, hook, pipeline, worktree, None, operation, timeout, log, Log::Truncate)
}

/// How a hook's log is opened: every hook starts its log afresh, except the Gate's `pre-merge`, whose log
/// already holds the lines of the wait for a slot (contract §7.6 "The test slot"), which its output follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Log {
    Truncate,
    Append,
}

/// What the Gate adds to [`run_hook`] for a hook in a gate worktree (contract §7.3, §7.6).
#[derive(Clone, Copy, Debug)]
pub struct Detached<'a> {
    /// The template variable `branch`, given explicitly because `HEAD` is detached.
    pub branch: &'a str,
    /// Variables added to the hook's environment, which holds no inherited `CPD_*` (contract §7.3): `CPD_TEST_SLOT`.
    pub env: &'a [(&'a str, &'a str)],
}

/// [`run_hook`] in a worktree whose `HEAD` is detached (a gate worktree,
/// contract §7.3, §7.6): the template variable `branch` is `detached.branch`
/// instead of being read from `HEAD`, and `detached.env` is added to the environment.
pub fn run_hook_as(
    hook: Hook,
    pipeline: &Pipeline,
    worktree: &Path,
    detached: &Detached<'_>,
    operation: Option<&Operation<'_>>,
    timeout: Duration,
    log: &Path,
    mode: Log,
) -> Result<HookResult, CpdError> {
    run_with_branch(&Ports::current(), hook, pipeline, worktree, Some(detached), operation, timeout, log, mode)
}

fn run_with_branch(
    ports: &Ports,
    hook: Hook,
    pipeline: &Pipeline,
    worktree: &Path,
    detached: Option<&Detached<'_>>,
    operation: Option<&Operation<'_>>,
    timeout: Duration,
    log: &Path,
    mode: Log,
) -> Result<HookResult, CpdError> {
    if pipeline.is_empty() {
        return Ok(HookResult { outcome: HookOutcome::Passed, tail: None });
    }

    let cwd = worktree.canonicalize().unwrap_or_else(|_| worktree.to_path_buf());
    let vars = variables(ports, hook, &cwd, detached.map(|d| d.branch), operation)?;
    let mut blocks = Vec::new();
    for block in &pipeline.blocks {
        let mut rendered = Vec::new();
        for c in block {
            let mut vars = vars.clone();
            if let Some(name) = &c.name {
                vars.insert("hook_name".to_owned(), json!(name));
            }
            let command = match template::render(&c.template, &vars) {
                Ok(command) => command,
                Err(why) => {
                    return Ok(HookResult { outcome: HookOutcome::Error(HookError::Invalid(why)), tail: None });
                }
            };
            rendered.push(exec::Rendered { name: c.name.clone(), command, context: exec::context_json(&vars) });
        }
        blocks.push(rendered);
    }

    Ok(run_rendered(ports, &blocks, &cwd, detached.map_or(&[], |d| d.env), timeout, log, mode))
}

/// Runs rendered `blocks` in `cwd` and writes the output to `log` (opened as `mode` says).
fn run_rendered(
    ports: &Ports,
    blocks: &[Vec<exec::Rendered>],
    cwd: &Path,
    env: &[(&str, &str)],
    timeout: Duration,
    log: &Path,
    mode: Log,
) -> HookResult {
    // The log is opened first, so a killed driver still leaves it; a hook's output is written when the hook ends.
    let open = || match mode {
        Log::Truncate => fs::File::create(log),
        Log::Append => fs::File::options().create(true).append(true).open(log),
    };
    let file = match log.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| open()) {
        Ok(file) => file,
        Err(e) => {
            let why = format!("Cannot write {}: {e}.", log.display());
            return HookResult { outcome: HookOutcome::Error(HookError::Io(why)), tail: None };
        }
    };
    let done = exec::run_on(&*ports.runner, &*ports.clock, blocks, cwd, timeout, env, file, TAIL_LINES);
    let tail = Some(done.tail.join("\n"));

    let written = match done.log_error {
        None => Ok(()),
        Some(e) => Err(HookError::Io(format!("Cannot write {}: {e}.", log.display()))),
    };
    let outcome = match (done.failure, written) {
        (None, Ok(())) => HookOutcome::Passed,
        (Some(exec::Ended::Exit(exit)), _) => {
            HookOutcome::Failed { exit, command: done.failed_command.expect("a failure has its command") }
        }
        (Some(exec::Ended::Signal(n)), _) => HookOutcome::Error(HookError::Signal(n)),
        (Some(exec::Ended::TimedOut), _) => HookOutcome::Error(HookError::Timeout),
        (Some(exec::Ended::NotStarted(why)), _) => HookOutcome::Error(HookError::Io(why)),
        (None, Err(e)) => HookOutcome::Error(e),
    };
    HookResult { outcome, tail }
}

/// One command of the repo's `[tests]` (contract §7.7), run as a hook is (§7.3): rendered with the hook's variables
/// (`target` the Lead Branch) and `extra` (`tests`, a list, or `test`), in `worktree`, `sh -c`, the hook environment.
/// Its output goes to `log`, which `mode` truncates or appends to. `HookOutcome::Error(Invalid)` when the template does not render; nothing ran then.
pub fn run_command(
    template: &str,
    extra: &[(&str, Value)],
    worktree: &Path,
    operation: &Operation<'_>,
    timeout: Duration,
    log: &Path,
    mode: Log,
) -> Result<HookResult, CpdError> {
    let ports = Ports::current();
    let cwd = worktree.canonicalize().unwrap_or_else(|_| worktree.to_path_buf());
    let mut vars = variables(&ports, Hook::PreCommit, &cwd, None, Some(operation))?;
    for (key, value) in extra {
        vars.insert((*key).to_owned(), value.clone());
    }
    let command = match template::render(template, &vars) {
        Ok(command) => command,
        Err(why) => return Ok(HookResult { outcome: HookOutcome::Error(HookError::Invalid(why)), tail: None }),
    };
    let rendered = exec::Rendered { name: None, command, context: exec::context_json(&vars) };
    Ok(run_rendered(&ports, &[vec![rendered]], &cwd, &[], timeout, log, mode))
}

/// The template variables of contract §7.3, as worktrunk defines them; also the
/// JSON each command gets on stdin. Unset ones are absent. `branch`, when
/// given, is used instead of the branch checked out in `cwd`.
fn variables(
    ports: &Ports,
    hook: Hook,
    cwd: &Path,
    branch: Option<&str>,
    operation: Option<&Operation<'_>>,
) -> Result<Map<String, Value>, CpdError> {
    let (runner, clock) = (&*ports.runner, &*ports.clock);
    let mut vars = Map::new();
    let mut set = |key: &str, value: Value| {
        vars.insert(key.to_owned(), value);
    };

    let branch = match branch {
        Some(branch) => Some(branch.to_owned()),
        None => current_branch_with(runner, cwd)?,
    };
    if let Some(branch) = branch {
        set("branch", json!(branch));
    }
    set("worktree_path", json!(cwd));
    set("worktree_name", json!(cwd.file_name().map(|n| n.to_string_lossy().into_owned())));
    set("commit", json!(rev_parse_with(runner, cwd, "HEAD")?));
    set("short_commit", json!(ok_with(runner, cwd, &["rev-parse", "--short", "HEAD"])?.trim()));

    let primary = list_with(runner, clock, cwd)?.into_iter().next().map(|w| w.path);
    if let Some(primary) = primary {
        set("repo", json!(primary.file_name().map(|n| n.to_string_lossy().into_owned())));
        set("repo_path", json!(primary));
        set("primary_worktree_path", json!(primary));
    }
    if let Some(default_branch) = default_branch(runner, cwd)? {
        set("default_branch", json!(default_branch));
    }
    if let Some(remote) = primary_remote(runner, cwd)? {
        let url = run_with(runner, cwd, &["remote", "get-url", &remote])?;
        if url.code == 0 {
            set("remote_url", json!(url.stdout.trim()));
        }
        set("remote", json!(remote));
    }

    if let Some(op) = operation {
        let (branch_key, path_key) = match hook {
            Hook::PreStart => ("base", "base_worktree_path"),
            Hook::PreCommit | Hook::PreMerge => ("target", "target_worktree_path"),
        };
        set(branch_key, json!(op.branch));
        if let Some(path) = worktree_of_branch_with(runner, clock, cwd, op.branch)? {
            set(path_key, json!(path));
        }
    }

    set("cwd", json!(cwd));
    set("hook_type", json!(hook.as_str()));
    set("args", json!([]));
    Ok(vars)
}

/// `origin`, else the first remote, else none.
fn primary_remote(runner: &dyn Runner, dir: &Path) -> Result<Option<String>, CpdError> {
    let remotes = ok_with(runner, dir, &["remote"])?;
    let mut names = remotes.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = names.next().map(str::to_owned);
    Ok(if remotes.lines().any(|l| l.trim() == "origin") { Some("origin".to_owned()) } else { first })
}

/// `origin/HEAD`'s branch, else `main` or `master` when it exists.
fn default_branch(runner: &dyn Runner, dir: &Path) -> Result<Option<String>, CpdError> {
    let head = run_with(runner, dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])?;
    if head.code == 0 {
        if let Some(name) = head.stdout.trim().strip_prefix("origin/") {
            return Ok(Some(name.to_owned()));
        }
    }
    for candidate in ["main", "master"] {
        if branch_exists_with(runner, dir, candidate)? {
            return Ok(Some(candidate.to_owned()));
        }
    }
    Ok(None)
}

