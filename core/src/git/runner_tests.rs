//! `git::run::run_with` on a [`FakeRunner`]: what it asks the runner for and how it maps the answer.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::run::{LOCK_TIMEOUTS, run_with};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::host::runner::{FakeRunner, Reply};

fn fake(reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on("git", |_| true, reply)
}

const DIR: &str = "/work/repo";

#[test]
fn run_with_asks_the_runner_for_git_c_dir_and_the_args() {
    let runner = fake(Reply::ok(""));
    run_with(&runner, Path::new(DIR), &["status", "--short"]).unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "git");
    assert_eq!(calls[0].program, "git");
    assert_eq!(calls[0].args, ["-C", DIR, "status", "--short"]);
}

#[test]
fn run_with_gives_a_local_call_the_local_deadline() {
    let runner = fake(Reply::ok(""));
    run_with(&runner, Path::new(DIR), &["status"]).unwrap();
    assert_eq!(runner.calls()[0].deadline, Duration::from_secs(60));
}

#[test]
fn run_with_gives_fetch_push_and_ls_remote_the_network_deadline() {
    for call in ["fetch", "push", "ls-remote"] {
        let runner = fake(Reply::ok(""));
        run_with(&runner, Path::new(DIR), &[call, "origin"]).unwrap();
        assert_eq!(
            runner.calls()[0].deadline,
            Duration::from_secs(300),
            "{call}"
        );
    }
}

#[test]
fn run_with_sets_the_environment_of_every_git_call() {
    let runner = fake(Reply::ok(""));
    run_with(&runner, Path::new(DIR), &["status"]).unwrap();
    let envs = runner.calls()[0].envs.clone();
    let has = |k: &str, v: &str| envs.contains(&(k.to_owned(), Some(v.to_owned())));
    assert!(has("GIT_TERMINAL_PROMPT", "0"));
    assert!(has("GIT_CONFIG_COUNT", &LOCK_TIMEOUTS.len().to_string()));
    for (i, (key, value)) in LOCK_TIMEOUTS.iter().enumerate() {
        assert!(has(&format!("GIT_CONFIG_KEY_{i}"), key));
        assert!(has(&format!("GIT_CONFIG_VALUE_{i}"), value));
    }
}

#[test]
fn run_with_returns_the_scripted_stdout() {
    let runner = fake(Reply::ok("abc123\n"));
    let out = run_with(&runner, Path::new(DIR), &["rev-parse", "HEAD"]).unwrap();
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "abc123\n");
    assert_eq!(out.stderr, "");
}

#[test]
fn run_with_returns_a_scripted_non_zero_exit_as_its_code_and_stderr_like_run() {
    let runner = fake(Reply::exit(128, "", "fatal: not a git repository\n"));
    let out = run_with(&runner, Path::new(DIR), &["status"]).unwrap();
    assert_eq!(out.code, 128);
    assert_eq!(out.stderr, "fatal: not a git repository\n");
}

#[test]
fn run_with_maps_a_git_that_cannot_start_to_git_failed_with_the_argv() {
    let runner = fake(Reply::io(ErrorKind::NotFound));
    let err = run_with(&runner, Path::new(DIR), &["status"]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert!(
        err.message().starts_with("Cannot run git: "),
        "{}",
        err.message()
    );
    assert_eq!(err.details()["argv"], json!(["git", "-C", DIR, "status"]));
    assert_eq!(err.details()["exit"], json!(null));
}

#[test]
fn run_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        "git",
        |_| true,
        Reply::ok("").takes(Duration::from_secs(61)),
    );
    let err = run_with(&runner, Path::new(DIR), &["status"]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["tool"], "git");
    assert_eq!(err.details()["argv"], json!(["git", "-C", DIR, "status"]));
    assert_eq!(err.details()["deadline_seconds"], 60);
    assert_eq!(clock.elapsed(), Duration::from_secs(60));
}

#[test]
fn run_with_a_network_call_times_out_at_the_network_deadline() {
    let runner = fake(Reply::ok("").takes(Duration::from_secs(300)));
    let err = run_with(&runner, Path::new(DIR), &["fetch"]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["deadline_seconds"], 300);
}

#[test]
fn run_with_does_not_run_a_process() {
    let runner = FakeRunner::new(ManualClock::new());
    let dir: PathBuf = "/nonexistent/for/sure".into();
    let err = run_with(&runner, &dir, &["status"]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert_eq!(runner.unscripted().len(), 1);
}
