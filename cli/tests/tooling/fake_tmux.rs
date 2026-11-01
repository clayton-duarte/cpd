//! The fake tmux binary and its harness hooks (docs/implementing.md, Tests, "The fake tmux"). Two groups: the built
//! `fake-tmux` run as a process (`bin_*`) and `TestEnv`'s hooks (`harness_*`). The conformance of the adapter with the
//! fake (`adapter_*`) runs in-process: `core/src/fakes/tmux/conformance_tests.rs`. No test uses a real tmux, sleeps
//! or leaves a process: every child is waited for, a hung one is killed after a bound by a channel timeout.

use crate::common;

use common::TestEnv;
use cpd::fakes::tmux::State;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-tmux");
/// What a hung call is bounded by: far above a call's time, so only a deadlock reaches it.
const BOUND: Duration = Duration::from_secs(60);

// ---- the bin ----

/// `fake-tmux -u -L <socket> -f /dev/null <args>` with `TMUX_TMPDIR` `dir`.
fn tmux_cmd(dir: &Path, socket: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(FAKE);
    cmd.args(["-u", "-L", socket, "-f", "/dev/null"])
        .args(args)
        .env("TMUX_TMPDIR", dir)
        .env("FAKE_TMUX_SERVER_PID", "4242");
    cmd.stdin(Stdio::null());
    cmd
}

/// Runs the call; a call that outlasts [`BOUND`] is killed and fails the test.
fn bounded(mut cmd: Command) -> Output {
    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run fake-tmux");
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || tx.send(child.wait_with_output()).expect("send"));
    let out = match rx.recv_timeout(BOUND) {
        Ok(out) => out.expect("wait for fake-tmux"),
        Err(_) => {
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
            waiter.join().expect("waiter");
            panic!("fake-tmux did not return within {BOUND:?}: a deadlock");
        }
    };
    waiter.join().expect("waiter");
    out
}

fn call(dir: &Path, socket: &str, args: &[&str]) -> Output {
    bounded(tmux_cmd(dir, socket, args))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8 output")
}

/// A call that must exit 0; its stdout.
fn ok(dir: &Path, socket: &str, args: &[&str]) -> String {
    let out = call(dir, socket, args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{args:?}: stderr {:?}",
        text(&out.stderr)
    );
    text(&out.stdout)
}

/// A temp `TMUX_TMPDIR` with a server on socket `t` (session `s`, window `w`: `@0`, pane `%0`).
fn server() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    ok(
        dir.path(),
        "t",
        &["new-session", "-d", "-s", "s", "-n", "w"],
    );
    dir
}

#[test]
fn bin_accepts_the_global_flags_and_starts_a_server() {
    let dir = tempfile::tempdir().unwrap();
    let out = call(dir.path(), "x", &["new-session", "-d", "-s", "s"]);
    assert_eq!(
        (out.status.code(), text(&out.stderr)),
        (Some(0), String::new())
    );
    assert!(
        State::open(dir.path(), "x").is_some(),
        "no server state after new-session"
    );
}

#[test]
fn bin_has_session_exits_0_for_a_session_that_exists() {
    let dir = server();
    assert_eq!(
        call(dir.path(), "t", &["has-session", "-t", "=s"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn bin_has_session_exits_1_for_a_session_that_does_not_exist() {
    let dir = server();
    assert_eq!(
        call(dir.path(), "t", &["has-session", "-t", "=nope"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn bin_without_a_server_says_no_server_running_on_the_socket_path_and_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let out = call(dir.path(), "x", &["has-session", "-t", "=s"]);
    let uid = std::fs::metadata(dir.path()).unwrap().uid();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "no server running on {}/tmux-{uid}/x\n",
            dir.path().display()
        )
    );
}

#[test]
fn bin_kill_server_ends_the_server_so_the_next_call_says_no_server_running() {
    let dir = server();
    ok(dir.path(), "t", &["kill-server"]);
    let out = call(dir.path(), "t", &["list-sessions"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).starts_with("no server running on "),
        "{:?}",
        text(&out.stderr)
    );
}

#[test]
fn bin_refuses_a_missing_tmux_tmpdir() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone");
    let out = call(&gone, "x", &["new-session", "-d", "-s", "s"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains(&gone.display().to_string()),
        "{:?}",
        text(&out.stderr)
    );
    assert!(!gone.exists(), "the fake created the missing directory");
}

#[test]
fn bin_runs_ctl_when_it_is_the_first_argument() {
    let dir = server();
    ok(
        dir.path(),
        "t",
        &["__ctl", "pane-exit", "%0", "--status", "7"],
    );
    assert_eq!(
        ok(
            dir.path(),
            "t",
            &[
                "display-message",
                "-p",
                "-t",
                "%0",
                "#{pane_dead} #{pane_dead_status}"
            ]
        ),
        "1 7\n"
    );
}

#[test]
fn bin_refuses_ctl_anywhere_but_first() {
    let dir = server();
    let out = call(
        dir.path(),
        "t",
        &[
            "list-sessions",
            ";",
            "__ctl",
            "pane-exit",
            "%0",
            "--status",
            "7",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), "__ctl must be the first argument\n");
}

#[test]
fn bin_answers_an_unknown_subcommand_with_unknown_command_and_exit_1() {
    let dir = server();
    let out = call(dir.path(), "t", &["frobnicate"]);
    assert_eq!(
        (out.status.code(), text(&out.stderr)),
        (Some(1), "unknown command: frobnicate\n".to_string())
    );
}

#[test]
fn bin_prints_the_ids_of_a_chained_new_session() {
    let dir = tempfile::tempdir().unwrap();
    let out = ok(
        dir.path(),
        "t",
        &[
            "set",
            "-g",
            "mouse",
            "on",
            ";",
            "new-session",
            "-d",
            "-s",
            "s",
            "-P",
            "-F",
            "#{window_id} #{pane_id}",
        ],
    );
    assert_eq!(out, "@0 %0\n");
}

#[test]
fn bin_display_pid_prints_fake_tmux_server_pid() {
    let dir = server();
    assert_eq!(ok(dir.path(), "t", &["display", "-p", "#{pid}"]), "4242\n");
}

#[test]
fn bin_eight_concurrent_new_windows_give_eight_distinct_pane_ids() {
    let dir = server();
    let path = dir.path().to_path_buf();
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || {
                ok(
                    &path,
                    "t",
                    &[
                        "new-window",
                        "-d",
                        "-t",
                        "=s:",
                        "-n",
                        &format!("w{n}"),
                        "-P",
                        "-F",
                        "#{pane_id}",
                    ],
                )
            })
        })
        .collect();
    let mut ids: Vec<String> = threads
        .into_iter()
        .map(|t| t.join().expect("thread").trim().to_owned())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 8, "{ids:?}");
    assert!(ids.iter().all(|id| id.starts_with('%')), "{ids:?}");
}

#[test]
fn bin_a_hook_that_appends_to_a_file_fires_in_order_across_two_calls() {
    let dir = server();
    ok(dir.path(), "t", &["split-window", "-t", "%0"]);
    let log = dir.path().join("order.log");
    let hook = format!("run-shell 'echo #{{pane_id}} >> {}'", log.display());
    ok(dir.path(), "t", &["set-hook", "-g", "pane-died", &hook]);
    ok(
        dir.path(),
        "t",
        &["__ctl", "pane-exit", "%1", "--status", "0"],
    );
    ok(
        dir.path(),
        "t",
        &["__ctl", "pane-exit", "%0", "--status", "0"],
    );
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "%1\n%0\n");
}

#[test]
fn bin_a_run_shell_b_hook_that_calls_the_fake_again_does_not_deadlock() {
    let dir = server();
    let hook = format!(
        "run-shell -b 'TMUX_TMPDIR={} {FAKE} -u -L t -f /dev/null set -g @again yes'",
        dir.path().display()
    );
    ok(dir.path(), "t", &["set-hook", "-g", "pane-died", &hook]);
    ok(
        dir.path(),
        "t",
        &["__ctl", "pane-exit", "%0", "--status", "0"],
    );
    assert_eq!(ok(dir.path(), "t", &["show", "-gv", "@again"]), "yes\n");
}

// ---- the harness ----

#[test]
fn harness_fake_tmux_makes_tmux_on_the_envs_path_the_fake() {
    let env = TestEnv::new();
    env.fake_tmux();
    let link = env.bin().join("tmux");
    assert_eq!(
        std::fs::read_link(&link).expect("bin/tmux is a symlink"),
        PathBuf::from(FAKE)
    );
    let found = env
        .command("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .unwrap();
    assert_eq!(text(&found.stdout).trim(), link.display().to_string());
}

#[test]
fn harness_tmux_state_is_none_before_and_some_after_new_session() {
    let env = TestEnv::new();
    env.fake_tmux();
    assert!(env.tmux_state().is_none(), "a server state before any call");
    let out = env.tmux(&["new-session", "-d", "-s", "s"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        env.tmux_state().is_some(),
        "no server state after new-session"
    );
}

#[test]
fn harness_command_sets_fake_tmux_server_pid_to_the_test_process() {
    let env = TestEnv::new();
    let out = env
        .command("sh")
        .args(["-c", "printf %s \"${FAKE_TMUX_SERVER_PID-unset}\""])
        .output()
        .unwrap();
    assert_eq!(text(&out.stdout), std::process::id().to_string());
}

#[test]
fn harness_teardown_removes_the_fake_state_files_of_the_socket_from_a_tmux_dir_outside_the_env() {
    let env = TestEnv::new();
    let outside = tempfile::tempdir().unwrap();
    let socket = env.tmux_socket().to_owned();
    let (mine, derived, other) = (
        outside.path().join(format!("fake-tmux-{socket}.db")),
        outside
            .path()
            .join(format!("fake-tmux-{socket}-outer-x.db")),
        outside.path().join("fake-tmux-someone-else.db"),
    );
    for file in [&mine, &derived, &other] {
        std::fs::write(file, "").unwrap();
    }
    // The teardown script of the env, with its tmux dir moved outside the env (the `/tmp` case of `tmux_tmpdir`).
    // Written beside it and renamed over it, so the reaper's running copy keeps the old file.
    let script = env.root().join("teardown.sh");
    let moved: String = std::fs::read_to_string(&script)
        .unwrap()
        .lines()
        .map(|l| {
            if l.starts_with("tmuxdir=") {
                format!("tmuxdir='{}'", outside.path().display())
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tmp = env.root().join("teardown.sh.new");
    std::fs::write(&tmp, moved + "\n").unwrap();
    std::fs::rename(&tmp, &script).unwrap();
    env.teardown();
    assert!(
        !mine.exists(),
        "the socket's state file survived the teardown"
    );
    assert!(
        !derived.exists(),
        "a derived socket's state file survived the teardown"
    );
    assert!(
        other.exists(),
        "the teardown removed another socket's state file"
    );
}
