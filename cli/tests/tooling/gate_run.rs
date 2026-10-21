//! `scripts/gate-run.sh <command…>`: runs the command with a run id (`GATE_RUN`) and a `TMPDIR` of its own, and fails
//! when the run left a process, a directory or a tmux socket behind (hygiene.md, "The Gate leak check").

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;

use common::{Run, SLEEP_SECONDS, TestEnv, WAIT};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/gate-run.sh")
}

/// An env, and the `TMPDIR` its gate runs make their `gate-run.*` directory in.
struct Fx {
    env: TestEnv,
    tmp: PathBuf,
    _cleanup: Cleanup,
}

impl Fx {
    fn new() -> Fx {
        let env = TestEnv::new();
        let tmp = env.root().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let _cleanup = Cleanup { pid_files: vec![env.root().join("left.pid")], socket_id: env.root().join("run-id") };
        Fx { env, tmp, _cleanup }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.env.root().join(name)
    }

    /// `gate-run.sh sh -c <body>`, with `TMPDIR` the env's `tmp`.
    fn gate_run(&self, body: &str) -> Run {
        let out = common::output(self.env.command(script()).args(["sh", "-c", body]).env("TMPDIR", &self.tmp)).unwrap();
        Run {
            exit: out.status.code().expect("gate-run was killed by a signal"),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// What is left in `tmp`: the run's `gate-run.*` directory must be gone.
    fn tmp_entries(&self) -> Vec<String> {
        fs::read_dir(&self.tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
    }
}

/// Stops what a test's commands leave, whatever the outcome: the process whose pid a command wrote, and the tmux server
/// and socket file named with the run id a command wrote (`/tmp/tmux-<uid>`, tmux's default directory).
struct Cleanup {
    pid_files: Vec<PathBuf>,
    socket_id: PathBuf,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for file in &self.pid_files {
            if let Ok(pid) = fs::read_to_string(file) {
                let _ = std::process::Command::new("kill").args(["-KILL", pid.trim()]).status();
            }
        }
        if let Ok(id) = fs::read_to_string(&self.socket_id) {
            let socket = format!("cpdt-{}-1-0", id.trim());
            let _ = std::process::Command::new("tmux").args(["-L", &socket, "kill-server"]).env_remove("TMUX_TMPDIR").env_remove("TMUX").output();
            let _ = fs::remove_file(socket_file(&socket));
        }
    }
}

fn socket_file(socket: &str) -> PathBuf {
    Path::new("/tmp").join(format!("tmux-{}", { use std::os::unix::fs::MetadataExt; std::fs::metadata("/proc/self").unwrap().uid() })).join(socket)
}

fn alive(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z') && !rest.starts_with('X')))
}

/// A command that leaves a detached process (`setsid`, out of every group) and writes its pid to `left.pid`.
fn leave_a_process(fx: &Fx) -> String {
    format!("setsid sleep {SLEEP_SECONDS} </dev/null >/dev/null 2>&1 & echo $! > '{}'", fx.file("left.pid").display())
}

#[test]
fn a_clean_run_prints_nothing_and_returns_the_commands_status() {
    let fx = Fx::new();
    let ids = fx.file("ids");

    let run = fx.gate_run(&format!("echo \"$GATE_RUN $TMPDIR\" > '{}'; exit 3", ids.display()));

    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (3, "", ""), "{run:?}");
    // The command saw a six-character run id and a TMPDIR of the run's own, `$TMPDIR/gate-run.<id>`.
    let seen = fs::read_to_string(&ids).unwrap();
    let (id, dir) = seen.trim().split_once(' ').unwrap();
    assert_eq!(id.len(), 6, "{seen:?}");
    assert_eq!(Path::new(dir), fx.tmp.join(format!("gate-run.{id}")));
    assert_eq!(fx.tmp_entries(), Vec::<String>::new(), "a clean run removes its empty directory");
}

#[test]
fn a_process_left_with_the_run_id_fails_the_run_and_is_stopped() {
    let fx = Fx::new();

    let run = fx.gate_run(&leave_a_process(&fx));

    let pid: u32 = fs::read_to_string(fx.file("left.pid")).unwrap_or_default().trim().parse().unwrap_or_else(|_| panic!("the command left no process: {run:?}"));
    assert_eq!(run.exit, 1, "{run:?}");
    assert!(run.stderr.contains("gate-run: FAIL"), "{run:?}");
    assert!(run.stderr.contains(&format!("process {pid}")), "the process is not named: {run:?}");
    assert!(common::poll(WAIT, || (!alive(pid)).then_some(())).is_some(), "process {pid} is still running");
}

#[test]
fn a_dir_left_in_the_runs_tmpdir_fails_the_run_and_is_removed() {
    let fx = Fx::new();
    let left = fx.file("left-dir");

    let run = fx.gate_run(&format!("mkdir \"$TMPDIR/cpd-test-left\"; echo \"$TMPDIR/cpd-test-left\" > '{}'", left.display()));

    assert_eq!(run.exit, 1, "{run:?}");
    assert!(run.stderr.contains("gate-run: FAIL"), "{run:?}");
    let path = fs::read_to_string(&left).unwrap_or_else(|_| panic!("the command did not run: {run:?}"));
    assert!(run.stderr.contains(path.trim()), "the directory {path} is not named: {run:?}");
    assert!(!Path::new(path.trim()).exists(), "{path} is still there");
    assert_eq!(fx.tmp_entries(), Vec::<String>::new(), "the run's directory is removed");
}

#[test]
fn a_socket_named_with_the_run_id_fails_the_run() {
    let fx = Fx::new();
    // A server of the run on tmux's default directory, ended at once: tmux leaves the socket file (teardown.md).
    let body = format!(
        "unset TMUX TMUX_TMPDIR; echo \"$GATE_RUN\" > '{id}'; s=\"cpdt-$GATE_RUN-1-0\"; \
         tmux -L \"$s\" -f /dev/null new-session -d 'sleep {SLEEP_SECONDS}' && tmux -L \"$s\" kill-server",
        id = fx.file("run-id").display()
    );

    let run = fx.gate_run(&body);

    let id = fs::read_to_string(fx.file("run-id")).unwrap_or_else(|_| panic!("the command did not run: {run:?}"));
    let socket = format!("cpdt-{}-1-0", id.trim());
    assert_eq!(run.exit, 1, "{run:?}");
    assert!(run.stderr.contains("gate-run: FAIL") && run.stderr.contains(&socket), "the socket is not named: {run:?}");
    assert!(!socket_file(&socket).exists(), "the socket file is still there");
}

#[test]
fn another_runs_leftovers_do_not_fail_this_run() {
    let fx = Fx::new();
    // A second gate run, still running its command, which has left a directory and holds a process: its own id.
    let other_id = fx.file("other-id");
    let body = format!("echo \"$GATE_RUN\" > '{}'; mkdir \"$TMPDIR/other-left\"; exec sleep {SLEEP_SECONDS}", other_id.display());
    let mut other = common::spawn(fx.env.command(script()).args(["sh", "-c", &body]).env("TMPDIR", &fx.tmp).stderr(std::process::Stdio::null()))
        .unwrap();
    let _stop = StopChild(&mut other);
    common::wait_some("the other run's command to start", WAIT, || {
        if let Some(status) = _stop.0.try_wait().unwrap() {
            panic!("the other run ended ({status}) before it ran its command: is there a scripts/gate-run.sh?");
        }
        if other_id.exists() { Ok(()) } else { Err("not yet".into()) }
    });

    let run = fx.gate_run("exit 0");

    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");
}

/// Kills the child when dropped.
struct StopChild<'a>(&'a mut Child);

impl Drop for StopChild<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_failing_command_keeps_its_status_and_still_reports_leaks() {
    let fx = Fx::new();

    let run = fx.gate_run("mkdir \"$TMPDIR/cpd-test-left\"; exit 5");

    assert_eq!(run.exit, 5, "{run:?}");
    assert!(run.stderr.contains("gate-run: FAIL") && run.stderr.contains("cpd-test-left"), "the leak is not reported: {run:?}");
    assert_eq!(fx.tmp_entries(), Vec::<String>::new(), "the leak is removed");
}
