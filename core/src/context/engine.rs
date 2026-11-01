//! The holographic engine rail (contract §4.6; `docs/research/context-holographic.md`,
//! `## Bridge` and `## Config and failure`).
//!
//! cpd reaches the holographic memory engine through `contrib/holographic/bridge.py`,
//! spawned per operation: JSON lines in on stdin, one JSON line out on stdout per
//! request, in order. `cpd.db` stays the source of truth; the engine is a derived
//! per-Context index under `context_engine_dir`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::{Value, json};

use crate::error::{CpdError, ErrorCode};

/// A holographic engine, reached one JSON request line at a time.
pub trait ContextEngine {
    /// Sends `requests`, one JSON object per line, in order, closes stdin and
    /// returns the responses in order. `engine_failed` when the child exits
    /// non-zero, prints bad JSON, answers no line, or times out. The child is
    /// always killed and reaped.
    fn run(&self, db: &Path, requests: &[Value]) -> Result<Vec<Value>, CpdError>;

    /// `{"cmd":"ping"}`: the engine's health and version (`doctor`, first use).
    fn ping(&self, db: &Path) -> Result<Value, CpdError> {
        Ok(self.run(db, &[json!({"cmd": "ping"})])?.remove(0))
    }

    /// `{"cmd":"stats"}`: fact count and the projection watermark.
    fn stats(&self, db: &Path) -> Result<Value, CpdError> {
        Ok(self.run(db, &[json!({"cmd": "stats"})])?.remove(0))
    }

    /// `{"cmd":"reset"}`: delete every fact of this Context's DB.
    fn reset(&self, db: &Path) -> Result<Value, CpdError> {
        Ok(self.run(db, &[json!({"cmd": "reset"})])?.remove(0))
    }
}

/// The spawning client: one bridge child per [`ContextEngine::run`].
pub struct Bridge {
    script: PathBuf,
    python: PathBuf,
    timeout: Duration,
}

impl Bridge {
    /// The resolved client. `engine_disabled` (exit 3) when `enabled` is false;
    /// `engine_missing` (exit 4) when the bridge or the interpreter is absent.
    pub fn new(
        enabled: bool,
        script: PathBuf,
        python: PathBuf,
        timeout: Duration,
    ) -> Result<Bridge, CpdError> {
        if !enabled {
            return Err(CpdError::new(
                ErrorCode::EngineDisabled,
                "The holographic engine is disabled by context_engine_enabled = false.",
            )
            .with("key", "context_engine_enabled"));
        }
        if !script.is_file() {
            return Err(missing(&script, &python));
        }
        if !interpreter_exists(&python) {
            return Err(missing(&script, &python));
        }
        Ok(Bridge {
            script,
            python,
            timeout,
        })
    }

    /// The resolved bridge script.
    pub fn script(&self) -> &Path {
        &self.script
    }

    /// The resolved interpreter.
    pub fn python(&self) -> &Path {
        &self.python
    }

    /// The resolved deadline.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// The one refusal for missing software: bridge, interpreter and the same hint `doctor` reports.
fn missing(script: &Path, python: &Path) -> CpdError {
    CpdError::new(
        ErrorCode::EngineMissing,
        format!(
            "The holographic engine is not installed: {} or {} is missing.",
            script.display(),
            python.display()
        ),
    )
    .with("bridge", script.display().to_string())
    .with("python", python.display().to_string())
    .with(
        "hint",
        "install the engine with dotfiles (scripts/holographic-install.sh), or set context_engine_bridge and context_engine_python.",
    )
}

/// Whether `python` names an executable file, or a bare command on `PATH`.
fn interpreter_exists(python: &Path) -> bool {
    if python.components().count() > 1 {
        return python.is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let candidate = dir.join(python);
            candidate.is_file()
        })
    })
}

/// `engine_failed` (exit 1): a bridge failure that is not a malformed query.
fn failed(message: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::EngineFailed, message)
}

/// Kills and reaps the child on every path out of [`Bridge::run`].
struct Guard {
    child: Option<Child>,
}

impl Guard {
    fn new(child: Child) -> Guard {
        Guard { child: Some(child) }
    }

    fn child(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("the child is live until it is reaped")
    }

    /// `SIGKILL` the child's process group, then reap the child. Idempotent.
    fn kill_and_wait(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Reaps a child that has exited; does nothing while it runs.
    fn reap_if_exited(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let child = self.child();
        if let Some(status) = child.try_wait()? {
            self.child = None;
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

impl ContextEngine for Bridge {
    fn run(&self, db: &Path, requests: &[Value]) -> Result<Vec<Value>, CpdError> {
        let mut cmd = Command::new(&self.python);
        cmd.arg(&self.script).arg("--db").arg(db);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let child = cmd.spawn().map_err(|e| {
            failed(format!(
                "cannot start the holographic bridge {} with {}: {e}.",
                self.script.display(),
                self.python.display()
            ))
        })?;
        let mut guard = Guard::new(child);
        let until = Instant::now() + self.timeout;

        let mut input = Vec::new();
        for request in requests {
            let line = serde_json::to_string(request).expect("a JSON value always serialises");
            input.extend_from_slice(line.as_bytes());
            input.push(b'\n');
        }
        let mut stdin = guard.child().stdin.take().expect("stdin is piped");
        let writer = thread::spawn(move || {
            let result = stdin.write_all(&input);
            drop(stdin);
            result
        });

        let stdout = guard.child().stdout.take().expect("stdout is piped");
        let lines = read_lines(stdout);
        let stderr = guard.child().stderr.take().expect("stderr is piped");
        let errors = read_all(stderr);

        let mut responses = Vec::with_capacity(requests.len());
        loop {
            if responses.len() == requests.len() {
                break;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                guard.kill_and_wait();
                return Err(self.timeout_error());
            }
            match lines.recv_timeout(left) {
                Ok(Ok(line)) => match serde_json::from_str::<Value>(&line) {
                    Ok(value) => responses.push(value),
                    Err(e) => {
                        guard.kill_and_wait();
                        return Err(failed(format!(
                            "The holographic bridge printed a line that is not JSON ({e}): {line}"
                        )));
                    }
                },
                Ok(Err(done)) => {
                    // stdout ended: the child answered fewer lines than it was asked.
                    let _ = writer.join();
                    let status = wait_or_kill(&mut guard, until)?;
                    let stderr = errors.lock().map(|text| text.clone()).unwrap_or_default();
                    let what = match done {
                        Some(e) => format!("its stdout could not be read: {e}"),
                        None => "it printed no answer for a request".to_owned(),
                    };
                    return Err(failed(format!(
                        "The holographic bridge failed ({what}; exit {}).{}{}",
                        status,
                        tail(&stderr),
                        if status.success() {
                            " It exited 0 without answering."
                        } else {
                            ""
                        }
                    )));
                }
                Err(RecvTimeoutError::Timeout) => {
                    guard.kill_and_wait();
                    return Err(self.timeout_error());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    guard.kill_and_wait();
                    return Err(failed(
                        "The holographic bridge stopped before answering a request.",
                    ));
                }
            }
        }

        let _ = writer.join();
        let status = wait_or_kill(&mut guard, until)?;
        if !status.success() {
            let stderr = errors.lock().map(|text| text.clone()).unwrap_or_default();
            return Err(failed(format!(
                "The holographic bridge exited {status}.{}",
                tail(&stderr)
            )));
        }
        Ok(responses)
    }
}

impl Bridge {
    fn timeout_error(&self) -> CpdError {
        failed(format!(
            "The holographic bridge ran past its deadline of {} ms and was killed.",
            self.timeout.as_millis()
        ))
        .with("timeout_ms", json!(self.timeout.as_millis() as u64))
    }
}

/// Waits for the child until `until`, then kills and reaps it and returns `engine_failed`.
fn wait_or_kill(guard: &mut Guard, until: Instant) -> Result<std::process::ExitStatus, CpdError> {
    loop {
        if let Some(status) = guard.reap_if_exited().map_err(|e| {
            failed(format!(
                "The holographic bridge could not be waited for: {e}."
            ))
        })? {
            return Ok(status);
        }
        if Instant::now() >= until {
            guard.kill_and_wait();
            return Err(failed(
                "The holographic bridge ran past its deadline and was killed.",
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// `Ok(line)` for each line on `pipe`; the final `Err` says why the pipe ended:
/// `None` for EOF, `Some(e)` for a read error.
fn read_lines(
    pipe: impl Read + Send + 'static,
) -> Receiver<Result<String, Option<std::io::Error>>> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(Err(None));
                    return;
                }
                Ok(_) => {
                    let line = line.trim_end_matches(['\n', '\r']).to_owned();
                    if tx.send(Ok(line)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(Some(e)));
                    return;
                }
            }
        }
    });
    rx
}

/// Reads `pipe` to its end on a thread, for the failure message.
fn read_all(pipe: impl Read + Send + 'static) -> std::sync::Arc<std::sync::Mutex<String>> {
    let text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink = text.clone();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = BufReader::new(pipe).read_to_end(&mut bytes);
        if let Ok(mut text) = sink.lock() {
            *text = String::from_utf8_lossy(&bytes).trim_end().to_owned();
        }
    });
    text
}

fn tail(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(" Its stderr: {stderr}")
    }
}
