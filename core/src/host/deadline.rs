//! The one subprocess runner with a deadline (contract §1.2 `tool_timeout`):
//! `git` (§7), `tmux` (§6.3), `node` (§9) and the engine's `cpd pr poll` child
//! (§3.7). The child starts in its own process group; at the deadline the group
//! gets `SIGTERM`, then `SIGKILL` after [`GRACE`] to those still alive, and the
//! call fails with `tool_timeout`. Each deadline is a constant at its caller.
//!
//! Tests shorten a deadline with `CPD_TEST_DEADLINE_<TOOL>=<seconds>` (`<TOOL>`
//! upper-cased, a decimal, e.g. `CPD_TEST_DEADLINE_GIT=0.5`; [`super::limit`]).
//! Only a debug build reads it: a release build (the installed `cpd`) has no way
//! to change a deadline.

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::json;

use super::limit::limit;
use crate::error::{CpdError, ErrorCode};

/// How long the group has between `SIGTERM` and `SIGKILL` (contract §1.2).
pub const GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);
/// How long the pipes are read after a killed group: a process that left the
/// group (`setsid`) may hold them open, and must not hold the caller.
const DRAIN: Duration = Duration::from_secs(1);

/// Why [`output`] returned no [`Output`].
#[derive(Debug)]
pub enum Failed {
    /// The child could not be started or waited for: the caller's own error code.
    Io(io::Error),
    /// The child ran past its deadline and its group was killed: `tool_timeout`.
    Timeout(CpdError),
}

/// Runs `cmd` (stdout and stderr captured, stdin as the caller set it) in its own
/// process group for at most `deadline`, like [`Command::output`]. `tool` names it
/// in `details.tool` and selects the test override.
pub fn output(cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
    output_with_grace(cmd, tool, deadline, GRACE)
}

/// [`output`] with `grace` between `SIGTERM` and `SIGKILL` instead of [`GRACE`]: for this module's tests.
fn output_with_grace(
    cmd: &mut Command,
    tool: &str,
    deadline: Duration,
    grace: Duration,
) -> Result<Output, Failed> {
    cmd.stderr(Stdio::piped());
    run(cmd, tool, deadline, grace, Stdin::AsSet, false)
}

/// [`output`] with `stdin` written to the child's stdin by a thread, which then closes it (`None`: a null stdin),
/// and, when `stderr_set` is true, the child's stderr as the caller set it on `cmd` (a file it is appended to)
/// instead of captured. With `reap_group`, the child's group is killed (`SIGKILL`) when the child has exited: nothing it
/// left behind outlives the call. A write that fails is `Failed::Io` when the child exited successfully, except a
/// broken pipe: the child (or its group) closed its stdin without reading it, which is the child's choice, not a
/// start failure.
pub(super) fn output_fed(
    cmd: &mut Command,
    tool: &str,
    deadline: Duration,
    stdin: Option<Vec<u8>>,
    stderr_set: bool,
    reap_group: bool,
) -> Result<Output, Failed> {
    if !stderr_set {
        cmd.stderr(Stdio::piped());
    }
    run(
        cmd,
        tool,
        deadline,
        GRACE,
        stdin.map_or(Stdin::Null, Stdin::Bytes),
        reap_group,
    )
}

/// Where the stdin of the child comes from.
enum Stdin {
    /// As the caller set it on the `Command`.
    AsSet,
    Null,
    /// Written by a thread, then closed.
    Bytes(Vec<u8>),
}

fn run(
    cmd: &mut Command,
    tool: &str,
    deadline: Duration,
    grace: Duration,
    stdin: Stdin,
    reap_group: bool,
) -> Result<Output, Failed> {
    let deadline = limit(&format!("DEADLINE_{}", tool.to_ascii_uppercase()), deadline);
    let argv: Vec<String> = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    match &stdin {
        Stdin::AsSet => {}
        Stdin::Null => {
            cmd.stdin(Stdio::null());
        }
        Stdin::Bytes(_) => {
            cmd.stdin(Stdio::piped());
        }
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(Failed::Io)?;
    let written = match stdin {
        Stdin::Bytes(bytes) => child
            .stdin
            .take()
            .map(|mut pipe| thread::spawn(move || pipe.write_all(&bytes))),
        _ => None,
    };
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let until = Instant::now() + deadline;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= until => {
                kill_group(&mut child, grace);
                let _ = stdout.recv_timeout(DRAIN);
                let _ = stderr.recv_timeout(DRAIN);
                return Err(Failed::Timeout(timeout(tool, argv, deadline)));
            }
            Ok(None) => thread::sleep(POLL),
            Err(e) => {
                kill_group(&mut child, grace);
                return Err(Failed::Io(e));
            }
        }
    };
    if reap_group {
        // ESRCH, an empty group, is ignored.
        let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
    }
    // The child exited: its output is complete once the pipes close, which the
    // deadline also bounds (a daemon it left behind may hold them).
    let left = until.saturating_duration_since(Instant::now());
    let read = |rx: Receiver<Vec<u8>>| rx.recv_timeout(left.max(DRAIN)).unwrap_or_default();
    let output = Output {
        status,
        stdout: read(stdout),
        stderr: read(stderr),
    };
    match written.map(|w| {
        w.join()
            .expect("the writer of the child's stdin does not panic")
    }) {
        Some(Err(e)) if status.success() && e.kind() != io::ErrorKind::BrokenPipe => {
            Err(Failed::Io(e))
        }
        _ => Ok(output),
    }
}

/// The `tool_timeout` error (contract §1.2): `tool`, `argv`, `deadline_seconds`
/// (an integer; a decimal only for a test's sub-second override).
pub(crate) fn timeout(tool: &str, argv: Vec<String>, deadline: Duration) -> CpdError {
    let what = argv.get(1..).map(|rest| rest.join(" ")).unwrap_or_default();
    let seconds = if deadline.subsec_nanos() == 0 {
        json!(deadline.as_secs())
    } else {
        json!(deadline.as_secs_f64())
    };
    CpdError::new(
        ErrorCode::ToolTimeout,
        format!("{tool} ran past its deadline of {seconds}s and was killed: {tool} {what}"),
    )
    .with("tool", tool)
    .with("argv", json!(argv))
    .with("deadline_seconds", seconds)
}

/// Reads a pipe to its end on a thread; the bytes arrive on the channel.
fn drain(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    if let Some(mut pipe) = pipe {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = tx.send(bytes);
        });
    } else {
        let _ = tx.send(Vec::new());
    }
    rx
}

/// `SIGTERM` to the child's group; `SIGKILL` to what is left of it after
/// `grace`; reaps the child. The group id is the child's pid, which stays
/// reserved while any member of the group lives.
fn kill_group(child: &mut Child, grace: Duration) {
    let group = Pid::from_raw(child.id() as i32);
    let _ = killpg(group, Signal::SIGTERM);
    let until = Instant::now() + grace;
    let mut status: Option<ExitStatus> = None;
    while Instant::now() < until {
        if status.is_none() {
            status = child.try_wait().ok().flatten();
        }
        // The reaped child is out of the group: ESRCH means no member is left.
        if status.is_some() && killpg(group, None) == Err(Errno::ESRCH) {
            return;
        }
        thread::sleep(POLL);
    }
    let _ = killpg(group, Signal::SIGKILL);
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::spawn::{Pid as SpawnPid, ProcessSpawn, Spawn};

    fn wait_gone(pid: i32) -> bool {
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if !ProcessSpawn.alive(SpawnPid(pid.unsigned_abs())) {
                return true;
            }
            thread::sleep(POLL);
        }
        false
    }

    fn sh(script: &str, pidfile: &std::path::Path) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(script)
            .arg("sh")
            .arg(pidfile)
            .stdin(Stdio::null());
        cmd
    }

    #[test]
    fn a_child_that_ends_in_time_returns_its_output() {
        let out = output(
            Command::new("sh").args(["-c", "echo out; echo err >&2; exit 3"]),
            "sh",
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
    }

    #[test]
    fn a_child_that_closes_its_stdin_before_reading_it_is_not_an_io_failure() {
        // More than a pipe buffer, so the writer blocks and then sees the closed read end: the child's choice.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exec 0<&-"]);
        let out = output_fed(
            &mut cmd,
            "hook",
            Duration::from_secs(10),
            Some(vec![b'x'; 8 << 20]),
            false,
            false,
        )
        .unwrap();
        assert_eq!(out.status.code(), Some(0));
    }

    #[test]
    fn a_missing_program_is_an_io_failure_not_a_timeout() {
        let err = output(
            &mut Command::new("/nonexistent/tool"),
            "tool",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(matches!(err, Failed::Io(_)), "{err:?}");
    }

    #[test]
    fn at_the_deadline_the_whole_group_is_killed_and_the_call_is_tool_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        // A grandchild in the group: it must die with the child.
        let mut cmd = sh(r#"sleep 30 & echo $! > "$1"; wait"#, &pidfile);
        let started = Instant::now();
        let Err(Failed::Timeout(err)) = output(&mut cmd, "git", Duration::from_secs(1)) else {
            panic!("no timeout")
        };
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "took {:?}",
            started.elapsed()
        );
        assert_eq!(err.code(), ErrorCode::ToolTimeout);
        assert_eq!(err.exit().code(), 5);
        assert_eq!(err.details()["tool"], "git");
        assert_eq!(err.details()["deadline_seconds"], 1);
        assert_eq!(err.details()["argv"][0], "sh");
        let pid: i32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(wait_gone(pid), "the grandchild {pid} outlived the deadline");
    }
}
