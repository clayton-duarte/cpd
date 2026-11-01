//! `gh::run_timed_with` and `gh::run_raw_with` on a `FakeRunner` and a `ManualClock`: the `gh` call they make and
//! how they map the answer. No process.

use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use super::gh::{run_raw_with, run_timed_with};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::host::runner::{FakeRunner, Reply};

const SIXTY: Duration = Duration::from_secs(60);
const CWD: &str = "/cpd-home";
const ARGS: &[&str] = &["pr", "list", "--json", "number"];

fn fake(reply: Reply) -> (FakeRunner, ManualClock) {
    let clock = ManualClock::new();
    (
        FakeRunner::new(clock.clone()).on("gh", |_| true, reply),
        clock,
    )
}

#[test]
fn run_timed_with_calls_gh_with_its_args_cwd_env_and_deadline() {
    let (runner, _) = fake(Reply::ok("[]"));
    run_timed_with(&runner, Path::new(CWD), ARGS, SIXTY)
        .unwrap()
        .ok()
        .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "gh");
    assert_eq!(calls[0].args, ARGS);
    assert_eq!(calls[0].cwd.as_deref(), Some(Path::new(CWD)));
    assert_eq!(calls[0].deadline, SIXTY);
    assert_eq!(calls[0].stdin, None);
    assert!(
        calls[0]
            .envs
            .contains(&("GH_PROMPT_DISABLED".to_owned(), Some("1".to_owned())))
    );
    assert!(
        calls[0]
            .envs
            .contains(&("NO_COLOR".to_owned(), Some("1".to_owned())))
    );
}

#[test]
fn run_timed_with_returns_the_stdout_stderr_and_exit_code() {
    let (runner, _) = fake(Reply::exit(2, "out\n", "err\n"));
    let out = run_timed_with(&runner, Path::new(CWD), ARGS, SIXTY)
        .unwrap()
        .ok()
        .unwrap();
    assert_eq!(
        (out.stdout.as_str(), out.stderr.as_str(), out.exit),
        ("out\n", "err\n", Some(2))
    );
}

#[test]
fn run_timed_with_returns_the_output_of_a_zero_exit() {
    let (runner, _) = fake(Reply::ok("[]"));
    let out = run_timed_with(&runner, Path::new(CWD), ARGS, SIXTY)
        .unwrap()
        .ok()
        .unwrap();
    assert_eq!((out.stdout.as_str(), out.exit), ("[]", Some(0)));
}

#[test]
fn a_gh_that_takes_over_the_timeout_is_killed() {
    let (runner, clock) = fake(Reply::ok("late").takes(SIXTY + Duration::from_secs(1)));
    let killed = run_timed_with(&runner, Path::new(CWD), ARGS, SIXTY).unwrap();
    assert!(killed.is_err(), "the call is a kill, not an output");
    assert_eq!(clock.elapsed(), SIXTY);
}

#[test]
fn a_gh_that_takes_over_the_timeout_is_gh_failed_with_todays_text() {
    let (runner, _) = fake(Reply::ok("late").takes(SIXTY + Duration::from_secs(1)));
    let e = run_raw_with(&runner, Path::new(CWD), ARGS, SIXTY)
        .err()
        .unwrap();
    assert_eq!(e.code(), ErrorCode::GhFailed);
    assert_eq!(e.message(), "gh pr took longer than 60s and was killed.");
    assert_eq!(
        e.details()["argv"],
        serde_json::json!(["gh", "pr", "list", "--json", "number"])
    );
    assert_eq!(e.details()["exit"], serde_json::Value::Null);
}

#[test]
fn a_gh_that_takes_less_than_the_timeout_passes_its_time_and_is_not_killed() {
    let (runner, clock) = fake(Reply::ok("[]").takes(Duration::from_secs(59)));
    assert!(
        run_timed_with(&runner, Path::new(CWD), ARGS, SIXTY)
            .unwrap()
            .is_ok()
    );
    assert_eq!(clock.elapsed(), Duration::from_secs(59));
}

#[test]
fn a_missing_gh_is_gh_failed_with_cannot_run_gh() {
    let (runner, _) = fake(Reply::io(ErrorKind::NotFound));
    let e = run_timed_with(&runner, Path::new(CWD), ARGS, SIXTY)
        .err()
        .unwrap();
    assert_eq!(e.code(), ErrorCode::GhFailed);
    assert!(
        e.message().starts_with("Cannot run gh: "),
        "{}",
        e.message()
    );
    assert_eq!(
        e.details()["argv"],
        serde_json::json!(["gh", "pr", "list", "--json", "number"])
    );
    assert_eq!(e.details()["exit"], serde_json::Value::Null);
}

#[test]
fn a_gh_that_is_not_scripted_is_a_missing_gh() {
    let runner = FakeRunner::new(ManualClock::new());
    let e = run_raw_with(&runner, Path::new(CWD), ARGS, SIXTY)
        .err()
        .unwrap();
    assert_eq!(e.code(), ErrorCode::GhFailed);
    assert!(
        e.message().starts_with("Cannot run gh: "),
        "{}",
        e.message()
    );
}

#[test]
fn run_raw_with_returns_the_output_of_a_non_zero_exit_as_a_result() {
    let (runner, _) = fake(Reply::exit(1, "", "no\n"));
    let out = run_raw_with(&runner, Path::new(CWD), ARGS, SIXTY).unwrap();
    assert_eq!(
        (out.stdout.as_str(), out.stderr.as_str(), out.exit),
        ("", "no\n", Some(1))
    );
}
