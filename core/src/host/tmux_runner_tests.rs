//! `Tmux` on a [`FakeRunner`] (`Tmux::with_runner`): the call it makes and how it maps the answer.

use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::Tmux;
use super::runner::{FakeRunner, Reply};
use crate::clock::ManualClock;
use crate::error::ErrorCode;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

fn scripted(reply: Reply) -> (FakeRunner, tempfile::TempDir, Tmux) {
    let runner = FakeRunner::new(ManualClock::new()).on("tmux", |_| true, reply);
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir("sock", dir.path()).with_runner(Arc::new(runner.clone()));
    (runner, dir, tmux)
}

#[test]
fn a_call_is_tmux_u_l_socket_f_dev_null_and_the_args() {
    let (runner, _dir, tmux) = scripted(Reply::ok(""));
    tmux.run(&strings(&["list-sessions", "-F", "#{session_id}"]))
        .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "tmux");
    assert_eq!(calls[0].program, "tmux");
    assert_eq!(
        calls[0].args,
        [
            "-u",
            "-L",
            "sock",
            "-f",
            "/dev/null",
            "list-sessions",
            "-F",
            "#{session_id}"
        ]
    );
}

#[test]
fn a_call_gets_the_tmux_deadline_of_ten_seconds() {
    let (runner, _dir, tmux) = scripted(Reply::ok(""));
    tmux.run(&strings(&["list-sessions"])).unwrap();
    assert_eq!(runner.calls()[0].deadline, Duration::from_secs(10));
}

#[test]
fn a_call_removes_tmux_and_tmux_pane_from_the_environment() {
    let (runner, _dir, tmux) = scripted(Reply::ok(""));
    tmux.run(&strings(&["list-sessions"])).unwrap();
    let envs = runner.calls()[0].envs.clone();
    assert!(envs.contains(&("TMUX".to_owned(), None)), "{envs:?}");
    assert!(envs.contains(&("TMUX_PANE".to_owned(), None)), "{envs:?}");
}

#[test]
fn a_call_sets_tmux_tmpdir_when_the_tmux_has_one() {
    let (runner, dir, tmux) = scripted(Reply::ok(""));
    tmux.run(&strings(&["list-sessions"])).unwrap();
    let envs = runner.calls()[0].envs.clone();
    assert!(
        envs.contains(&(
            "TMUX_TMPDIR".to_owned(),
            Some(dir.path().display().to_string())
        )),
        "{envs:?}"
    );
}

#[test]
fn a_call_returns_the_scripted_stdout() {
    let (_runner, _dir, tmux) = scripted(Reply::ok("$1\n$2\n"));
    assert_eq!(tmux.run(&strings(&["list-sessions"])).unwrap(), "$1\n$2\n");
}

#[test]
fn a_missing_tmux_tmpdir_is_refused_before_the_runner_is_called() {
    let runner = FakeRunner::new(ManualClock::new()).on("tmux", |_| true, Reply::ok(""));
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    let tmux = Tmux::with_tmpdir("sock", &missing).with_runner(Arc::new(runner.clone()));
    let err = tmux.run(&strings(&["list-sessions"])).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert!(
        err.message().contains("is not a directory"),
        "{}",
        err.message()
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn a_scripted_non_zero_exit_is_tmux_failed_with_the_argv_and_the_stderr_tail() {
    let stderr: String = (1..=70).map(|n| format!("line {n}\n")).collect();
    let (_runner, _dir, tmux) = scripted(Reply::exit(1, "", &stderr));
    let err = tmux
        .run(&strings(&["kill-session", "-t", "x"]))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(err.message(), "tmux kill-session failed: line 1");
    assert_eq!(
        err.details()["argv"],
        json!([
            "tmux",
            "-u",
            "-L",
            "sock",
            "-f",
            "/dev/null",
            "kill-session",
            "-t",
            "x"
        ])
    );
    assert_eq!(err.details()["exit"], 1);
    let tail = err.details()["stderr"].as_str().unwrap();
    assert_eq!(tail.lines().count(), 60);
    assert!(tail.ends_with("line 70"), "{tail}");
}

#[test]
fn a_tmux_that_cannot_start_is_tmux_failed_with_the_io_error() {
    let (_runner, _dir, tmux) = scripted(Reply::io(ErrorKind::NotFound));
    let err = tmux.run(&strings(&["list-sessions"])).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert!(
        err.message().starts_with("Cannot run tmux: "),
        "{}",
        err.message()
    );
    assert_eq!(err.details()["exit"], json!(null));
}

#[test]
fn a_scripted_timeout_is_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        "tmux",
        |_| true,
        Reply::ok("").takes(Duration::from_secs(10)),
    );
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir("sock", dir.path()).with_runner(Arc::new(runner));
    let err = tmux.run(&strings(&["list-sessions"])).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["tool"], "tmux");
    assert_eq!(err.details()["deadline_seconds"], 10);
    assert_eq!(clock.elapsed(), Duration::from_secs(10));
}

#[test]
fn has_session_reads_exit_one_as_no_through_the_runner() {
    let (runner, _dir, tmux) = scripted(Reply::exit(1, "", "can't find session"));
    assert!(!tmux.has_session("x").unwrap());
    assert_eq!(runner.calls()[0].args[5..], ["has-session", "-t", "=x"]);
}
