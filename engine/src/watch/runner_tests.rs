//! The engine's child process on a [`FakeRunner`]: `pr::poll_with` (`cpd pr poll`). The call it makes, and how it maps a
//! scripted success, failure and timeout.

use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::pr::poll_with;
use super::report::Failure;
use cpd_core::clock::ManualClock;
use cpd_core::host::runner::{FakeRunner, Reply};

const EXE: &str = "/opt/cpd/bin/cpd";
const ROOT: &str = "/cpd-home";

fn fake(program: &str, reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on(program, |_| true, reply)
}

fn poll(runner: &FakeRunner) -> Result<Value, Failure> {
    poll_with(runner, Path::new(EXE), Path::new(ROOT))
}

fn printed_ok() -> String {
    json!({"ok": true, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}})
        .to_string()
}

// ---- watch::pr ---------------------------------------------------------------------------------------------------

#[test]
fn poll_with_asks_the_runner_for_the_recorded_executable_with_pr_poll() {
    let runner = fake("cpd", Reply::ok(&printed_ok()));
    poll(&runner).unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "cpd");
    assert_eq!(calls[0].program, EXE);
    assert_eq!(calls[0].args, ["pr", "poll"]);
}

#[test]
fn poll_with_gives_the_child_two_minutes_the_home_as_its_directory_and_the_homes_environment() {
    let runner = fake("cpd", Reply::ok(&printed_ok()));
    poll(&runner).unwrap();
    let call = &runner.calls()[0];
    assert_eq!(call.deadline, Duration::from_secs(120));
    assert_eq!(call.cwd.as_deref(), Some(Path::new(ROOT)));
    assert!(
        call.envs
            .contains(&("CPD_HOME".to_owned(), Some(ROOT.to_owned()))),
        "{:?}",
        call.envs
    );
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert!(
            call.envs.contains(&(removed.to_owned(), None)),
            "{removed}: {:?}",
            call.envs
        );
    }
}

#[test]
fn poll_with_returns_the_value_the_child_printed_on_success() {
    let runner = fake("cpd", Reply::ok(&printed_ok()));
    let value = poll(&runner).unwrap();
    assert_eq!(value["rate_limit"]["remaining"], 5000);
}

#[test]
fn poll_with_returns_the_failure_the_child_printed() {
    let printed = json!({"ok": false, "error": {"code": "gh_failed", "message": "gh broke", "details": {"stderr": "x"}}}).to_string();
    let runner = fake("cpd", Reply::exit(1, &printed, ""));
    let failure = poll(&runner).unwrap_err();
    assert_eq!(failure.code, "gh_failed");
    assert_eq!(failure.message, "gh broke");
    assert_eq!(failure.details, json!({"stderr": "x"}));
}

#[test]
fn poll_with_maps_a_child_that_printed_no_result_to_spawn_failed_with_argv_exit_and_output() {
    let runner = fake("cpd", Reply::exit(2, "not json", "crash"));
    let failure = poll(&runner).unwrap_err();
    assert_eq!(failure.code, "spawn_failed");
    assert_eq!(failure.message, format!("{EXE} pr poll printed no result."));
    assert_eq!(failure.details["argv"], json!([EXE, "pr", "poll"]));
    assert_eq!(failure.details["exit"], 2);
    assert_eq!(failure.details["stdout"], "not json");
    assert_eq!(failure.details["stderr"], "crash");
}

#[test]
fn poll_with_maps_a_child_that_cannot_start_to_spawn_failed() {
    let runner = fake("cpd", Reply::io(ErrorKind::NotFound));
    let failure = poll(&runner).unwrap_err();
    assert_eq!(failure.code, "spawn_failed");
    assert!(
        failure
            .message
            .starts_with(&format!("Cannot run {EXE} pr poll: ")),
        "{}",
        failure.message
    );
    assert_eq!(failure.details["argv"], json!([EXE, "pr", "poll"]));
}

#[test]
fn poll_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        "cpd",
        |_| true,
        Reply::ok(&printed_ok()).takes(Duration::from_secs(121)),
    );
    let failure = poll(&runner).unwrap_err();
    assert_eq!(failure.code, "tool_timeout");
    assert_eq!(failure.details["tool"], "cpd");
    assert_eq!(failure.details["argv"], json!([EXE, "pr", "poll"]));
    assert_eq!(failure.details["deadline_seconds"], 120);
    assert_eq!(clock.elapsed(), Duration::from_secs(120));
}
