//! Running rendered hook commands (contract §7.3): `sh -c`, concurrent within
//! a block, blocks in order, one deadline, process groups killed at it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::{Map, Value};

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

/// Everything a hook run produced: how it ended and its output, line by line.
pub struct Executed {
    /// The first command that did not exit 0, in file order; `None` if all did
    /// (and for a pipeline stopped by a block, that block's).
    pub failure: Option<Ended>,
    pub output: Vec<String>,
}

/// `TMUX`, `TMUX_PANE`, `PI_*` and `CPD_*` are not passed to hooks, so a test
/// suite run by the Gate cannot reach the real cockpit, pi session or database.
pub fn is_withheld(key: &str) -> bool {
    key == "TMUX" || key == "TMUX_PANE" || key.starts_with("PI_") || key.starts_with("CPD_")
}

const GRACE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

struct Running {
    child: Option<Child>,
    done: Option<Receiver<()>>,
    ended: Option<Ended>,
}

/// Runs the blocks in order in `cwd`; a failing block stops the pipeline.
pub fn execute(blocks: &[Vec<Rendered>], cwd: &Path, timeout: Duration) -> Executed {
    let deadline = Instant::now() + timeout;
    let output = Arc::new(Mutex::new(Vec::new()));
    let mut failure = None;
    for block in blocks.iter().filter(|b| !b.is_empty()) {
        let prefixed = block.len() > 1;
        let mut running: Vec<Running> =
            block.iter().map(|c| start(c, cwd, prefixed, &output)).collect();
        wait(&mut running, deadline);
        failure = running.into_iter().filter_map(|r| r.ended).find(|e| *e != Ended::Exit(0));
        if failure.is_some() {
            break;
        }
    }
    let output = std::mem::take(&mut *output.lock().unwrap());
    Executed { failure, output }
}

fn start(command: &Rendered, cwd: &Path, prefixed: bool, output: &Arc<Mutex<Vec<String>>>) -> Running {
    let not_started = |why: String| Running { child: None, done: None, ended: Some(Ended::NotStarted(why)) };
    let (reader, writer) = match std::io::pipe().and_then(|(r, w)| Ok((r, w.try_clone()?, w))) {
        Ok((r, w1, w2)) => (r, (w1, w2)),
        Err(e) => return not_started(format!("Cannot create a pipe: {e}.")),
    };
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&command.command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(writer.0)
        .stderr(writer.1)
        .process_group(0);
    for (key, _) in std::env::vars_os() {
        if is_withheld(&key.to_string_lossy()) {
            cmd.env_remove(&key);
        }
    }
    let spawned = cmd.spawn();
    drop(cmd); // closes our copies of the pipe's write end, so the reader sees EOF
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return not_started(format!("Cannot start sh: {e}.")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // The context is small, far below the pipe's capacity: this cannot block.
        let _ = stdin.write_all(command.context.as_bytes());
    }
    let prefix = match (&command.name, prefixed) {
        (Some(name), true) => format!("{name} │ "),
        _ => String::new(),
    };
    let (done_tx, done_rx) = channel();
    let output = Arc::clone(output);
    std::thread::spawn(move || {
        let mut lines = BufReader::new(reader);
        let mut buf = Vec::new();
        while let Ok(n) = lines.read_until(b'\n', &mut buf) {
            if n == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_owned();
            output.lock().unwrap().push(format!("{prefix}{line}"));
            buf.clear();
        }
        let _ = done_tx.send(());
    });
    Running { child: Some(child), done: Some(done_rx), ended: None }
}

/// Waits for every command of a block, all of them, even after one fails; at
/// the deadline kills the process groups of those still running; a command whose
/// `sh` exited has its group killed too (ESRCH, an empty group, is ignored).
fn wait(running: &mut [Running], deadline: Instant) {
    loop {
        let mut pending = false;
        for r in running.iter_mut() {
            let Some(child) = r.child.as_mut() else { continue };
            if r.ended.is_some() {
                continue;
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    // The sh is gone: nothing it left in its group may outlive the hook.
                    let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                    r.ended = Some(match (status.code(), status.signal()) {
                        (Some(code), _) => Ended::Exit(code),
                        (None, Some(sig)) => Ended::Signal(sig),
                        (None, None) => Ended::Signal(0),
                    });
                }
                Ok(None) => pending = true,
                Err(e) => r.ended = Some(Ended::NotStarted(format!("Cannot wait for sh: {e}."))),
            }
        }
        if !pending {
            break;
        }
        if Instant::now() >= deadline {
            for r in running.iter_mut().filter(|r| r.ended.is_none()) {
                if let Some(child) = r.child.as_mut() {
                    let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                    let _ = child.wait();
                    r.ended = Some(Ended::TimedOut);
                }
            }
            break;
        }
        std::thread::sleep(POLL);
    }
    // Let the readers drain; a daemon that kept the pipe open does not hold us up.
    for r in running.iter() {
        if let Some(done) = &r.done {
            let _ = done.recv_timeout(GRACE);
        }
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
