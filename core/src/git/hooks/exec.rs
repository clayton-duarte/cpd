//! Running rendered hook commands (contract §7.3): `sh -c`, concurrent within
//! a block, blocks in order, one deadline, process groups killed at it (by the `Runner`).

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::clock::{Clock, SystemClock};
use crate::host::deadline::Failed;
use crate::host::runner::{Call, HOOK_TOOL, Runner};

/// A command ready to run: rendered, with its stdin.
pub struct Rendered {
    pub name: Option<String>,
    pub command: String,
    /// The hook's template variables as one JSON object (the command's stdin).
    pub context: String,
}

/// How one command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    Exit(i32),
    Signal(i32),
    TimedOut,
    NotStarted(String),
}

/// Everything a hook run produced: how it ended and the end of its output (the
/// whole of it went to the log as it arrived).
pub struct Executed {
    /// The first command that did not exit 0, in file order; `None` if all did
    /// (and for a pipeline stopped by a block, that block's).
    pub failure: Option<Ended>,
    /// The rendered text of the command `failure` is of; `None` exactly when `failure` is.
    pub failed_command: Option<String>,
    /// The last `tail_lines` lines of the output, one per element.
    pub tail: Vec<String>,
    /// The I/O error that stopped the log being written, if any.
    pub log_error: Option<String>,
}

/// Where the output goes: each line to the log at once, the last `keep` in memory.
struct Output {
    log: File,
    tail: VecDeque<String>,
    keep: usize,
    log_error: Option<String>,
}

impl Output {
    fn push(&mut self, line: String) {
        if self.log_error.is_none() {
            if let Err(e) = writeln!(self.log, "{line}") {
                self.log_error = Some(e.to_string());
            }
        }
        if self.tail.len() == self.keep {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

/// `TMUX`, `TMUX_PANE`, `PI_*` and `CPD_*` are not passed to hooks, so a test
/// suite run by the Gate cannot reach the real cockpit, pi session or database.
pub fn is_withheld(key: &str) -> bool {
    key == "TMUX" || key == "TMUX_PANE" || key.starts_with("PI_") || key.starts_with("CPD_")
}

/// Runs the blocks in order in `cwd` over `runner`, each hook with `env` added to its environment: one
/// `Runner::run` per hook, `sh -c <hook>` with the context as stdin, the hooks of a block on a thread each; a
/// failing block stops the pipeline. A hook's output (stdout, then stderr) reaches the log and the tail when the
/// hook ends; only the last `tail_lines` lines are kept. One `timeout` for the whole run: each call's deadline is
/// what is left of it.
pub fn run_with(
    runner: &dyn Runner,
    blocks: &[Vec<Rendered>],
    cwd: &Path,
    timeout: Duration,
    env: &[(&str, &str)],
    log: File,
    tail_lines: usize,
) -> Executed {
    run_on(runner, &SystemClock, blocks, cwd, timeout, env, log, tail_lines)
}

/// [`run_with`] with the time `timeout` is measured on read from `clock`: what a block takes (a reply's `takes` on a
/// `FakeRunner` is virtual) is what the blocks after it have left.
pub fn run_on(
    runner: &dyn Runner,
    clock: &dyn Clock,
    blocks: &[Vec<Rendered>],
    cwd: &Path,
    timeout: Duration,
    env: &[(&str, &str)],
    log: File,
    tail_lines: usize,
) -> Executed {
    let mut started: Option<Instant> = None;
    let mut output = Output { log, tail: VecDeque::new(), keep: tail_lines, log_error: None };
    let removed: Vec<String> =
        std::env::vars_os().map(|(k, _)| k.to_string_lossy().into_owned()).filter(|k| is_withheld(k)).collect();
    let mut failure = None;
    let mut failed_command = None;
    for block in blocks.iter().filter(|b| !b.is_empty()) {
        let prefixed = block.len() > 1;
        // The first block has all of it: the clock starts with it.
        let left = started.map_or(timeout, |at| timeout.saturating_sub(clock.now().saturating_duration_since(at)));
        started.get_or_insert_with(|| clock.now());
        let ended: Vec<(Ended, Vec<u8>)> = std::thread::scope(|scope| {
            let threads: Vec<_> = block
                .iter()
                .map(|hook| {
                    let call = call_of(hook, cwd, left, env, &removed);
                    scope.spawn(move || ran(runner.run(&call)))
                })
                .collect();
            threads.into_iter().map(|t| t.join().expect("a hook's thread does not panic")).collect()
        });
        failure = None;
        failed_command = None;
        for (hook, (end, text)) in block.iter().zip(ended) {
            let prefix = match (&hook.name, prefixed) {
                (Some(name), true) => format!("{name} │ "),
                _ => String::new(),
            };
            for line in String::from_utf8_lossy(&text).lines() {
                output.push(format!("{prefix}{line}"));
            }
            if failure.is_none() && end != Ended::Exit(0) {
                failure = Some(end);
                failed_command = Some(hook.command.clone());
            }
        }
        if failure.is_some() {
            break;
        }
    }
    Executed { failure, failed_command, tail: output.tail.drain(..).collect(), log_error: output.log_error.take() }
}

/// The call of one hook: `TMUX`, `TMUX_PANE` and the other withheld variables removed, then `env` added.
fn call_of(hook: &Rendered, cwd: &Path, deadline: Duration, env: &[(&str, &str)], removed: &[String]) -> Call {
    let named = ["TMUX", "TMUX_PANE"].map(str::to_owned);
    let gone = named.iter().chain(removed.iter().filter(|k| !named.contains(k)));
    let envs = gone
        .map(|k| (k.clone(), None))
        // After the removal: only the variables the caller names pass, an inherited `CPD_*` never does.
        .chain(env.iter().map(|(k, v)| ((*k).to_owned(), Some((*v).to_owned()))))
        .collect();
    Call {
        tool: HOOK_TOOL.to_owned(),
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), hook.command.clone()],
        envs,
        cwd: Some(cwd.to_path_buf()),
        deadline,
        stdin: Some(hook.context.clone().into_bytes()),
        stderr_to: None,
    }
}

/// How a call ended and its output, stdout then stderr.
fn ran(result: Result<std::process::Output, Failed>) -> (Ended, Vec<u8>) {
    match result {
        Ok(out) => {
            let ended = match (out.status.code(), out.status.signal()) {
                (Some(code), _) => Ended::Exit(code),
                (None, Some(sig)) => Ended::Signal(sig),
                (None, None) => Ended::Signal(0),
            };
            (ended, [out.stdout, out.stderr].concat())
        }
        Err(Failed::Timeout(_)) => (Ended::TimedOut, Vec::new()),
        Err(Failed::Io(e)) => (Ended::NotStarted(format!("Cannot start sh: {e}.")), Vec::new()),
    }
}

/// The hook's template variables as the JSON text of a command's stdin.
pub fn context_json(vars: &Map<String, Value>) -> String {
    Value::Object(vars.clone()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn withholds_the_cockpit_pi_and_cpd() {
        for k in ["TMUX", "TMUX_PANE", "PI_SESSION", "CPD_HOME", "CPD_PLAN"] {
            assert!(is_withheld(k), "{k}");
        }
        for k in ["PATH", "HOME", "TMUXX", "PIPE", "CPDX", "GIT_DIR", "MY_CPD_X"] {
            assert!(!is_withheld(k), "{k}");
        }
    }
}
