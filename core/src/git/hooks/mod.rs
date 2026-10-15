//! The hooks of `.config/wt.toml` (contract §7.3): `pre-start` (setup),
//! `pre-commit` (the quick check) and `pre-merge` (the Gate), read from the
//! worktree at the moment they run, rendered, run, and their output captured.
//!
//! What a result means (`setup_failed`, `check_failed`, `gate_failed`, …) is the
//! caller's: [`HookOutcome`] only says how the hook ended.

mod def;
mod exec;
mod template;

use std::fmt;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::run::{branch_exists, current_branch, ok, rev_parse, run};
use super::worktree::{list, worktree_of_branch};
use super::{TAIL_LINES, tail};
use crate::error::CpdError;
use crate::home::Hook;

pub use def::{HookCommand, Pipeline, load};
pub use exec::is_withheld;
pub use template::shell_escape;

/// Why a hook could not be judged (contract §7.3, the last row of the result table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookError {
    /// `hook_invalid`: `.config/wt.toml` is malformed or a template does not
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
    Failed { exit: i32 },
    Error(HookError),
}

impl fmt::Display for HookOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookOutcome::Passed => f.write_str("passed"),
            HookOutcome::Failed { exit } => write!(f, "exit {exit}"),
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

/// Reads `hook` from `<worktree>/.config/wt.toml`, renders and runs its commands
/// in `worktree`, and writes the whole output to `log`.
///
/// `operation` is needed by every hook cpd runs; without it the `base` and
/// `target` variables are undefined. `timeout` is `setup_minutes` for `pre-start`,
/// `gate_minutes` for the others. `Err` is a git call that failed while building
/// the template variables.
pub fn run_hook(
    hook: Hook,
    worktree: &Path,
    operation: Option<&Operation<'_>>,
    timeout: Duration,
    log: &Path,
) -> Result<HookResult, CpdError> {
    let pipeline = match load(worktree, hook) {
        Ok(p) => p,
        Err(e) => return Ok(HookResult { outcome: HookOutcome::Error(e), tail: None }),
    };
    if pipeline.is_empty() {
        return Ok(HookResult { outcome: HookOutcome::Passed, tail: None });
    }

    let cwd = worktree.canonicalize().unwrap_or_else(|_| worktree.to_path_buf());
    let vars = variables(hook, &cwd, operation)?;
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

    let done = exec::execute(&blocks, &cwd, timeout);
    let text = done.output.iter().map(|l| format!("{l}\n")).collect::<String>();
    let tail = Some(tail(&text, TAIL_LINES));

    let written = log
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(log, &text))
        .map_err(|e| HookError::Io(format!("Cannot write {}: {e}.", log.display())));
    let outcome = match (done.failure, written) {
        (None, Ok(())) => HookOutcome::Passed,
        (Some(exec::Ended::Exit(exit)), _) => HookOutcome::Failed { exit },
        (Some(exec::Ended::Signal(n)), _) => HookOutcome::Error(HookError::Signal(n)),
        (Some(exec::Ended::TimedOut), _) => HookOutcome::Error(HookError::Timeout),
        (Some(exec::Ended::NotStarted(why)), _) => HookOutcome::Error(HookError::Io(why)),
        (None, Err(e)) => HookOutcome::Error(e),
    };
    Ok(HookResult { outcome, tail })
}

/// The template variables of contract §7.3, as worktrunk defines them; also the
/// JSON each command gets on stdin. Unset ones are absent.
fn variables(hook: Hook, cwd: &Path, operation: Option<&Operation<'_>>) -> Result<Map<String, Value>, CpdError> {
    let mut vars = Map::new();
    let mut set = |key: &str, value: Value| {
        vars.insert(key.to_owned(), value);
    };

    if let Some(branch) = current_branch(cwd)? {
        set("branch", json!(branch));
    }
    set("worktree_path", json!(cwd));
    set("worktree_name", json!(cwd.file_name().map(|n| n.to_string_lossy().into_owned())));
    set("commit", json!(rev_parse(cwd, "HEAD")?));
    set("short_commit", json!(ok(cwd, &["rev-parse", "--short", "HEAD"])?.trim()));

    let primary = list(cwd)?.into_iter().next().map(|w| w.path);
    if let Some(primary) = primary {
        set("repo", json!(primary.file_name().map(|n| n.to_string_lossy().into_owned())));
        set("repo_path", json!(primary));
        set("primary_worktree_path", json!(primary));
    }
    if let Some(default_branch) = default_branch(cwd)? {
        set("default_branch", json!(default_branch));
    }
    if let Some(remote) = primary_remote(cwd)? {
        let url = run(cwd, &["remote", "get-url", &remote])?;
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
        if let Some(path) = worktree_of_branch(cwd, op.branch)? {
            set(path_key, json!(path));
        }
    }

    set("cwd", json!(cwd));
    set("hook_type", json!(hook.as_str()));
    set("args", json!([]));
    Ok(vars)
}

/// `origin`, else the first remote, else none.
fn primary_remote(dir: &Path) -> Result<Option<String>, CpdError> {
    let remotes = ok(dir, &["remote"])?;
    let mut names = remotes.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = names.next().map(str::to_owned);
    Ok(if remotes.lines().any(|l| l.trim() == "origin") { Some("origin".to_owned()) } else { first })
}

/// `origin/HEAD`'s branch, else `main` or `master` when it exists.
fn default_branch(dir: &Path) -> Result<Option<String>, CpdError> {
    let head = run(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])?;
    if head.code == 0 {
        if let Some(name) = head.stdout.trim().strip_prefix("origin/") {
            return Ok(Some(name.to_owned()));
        }
    }
    for candidate in ["main", "master"] {
        if branch_exists(dir, candidate)? {
            return Ok(Some(candidate.to_owned()));
        }
    }
    Ok(None)
}

