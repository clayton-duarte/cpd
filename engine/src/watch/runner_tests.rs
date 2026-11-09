//! The engine's child process on a [`FakeRunner`]: `pr::poll_with` (`cpd pr poll`). The call it makes, and how it maps a
//! scripted success, failure and timeout.

use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::poll_run::Finished;
use super::pr::{apply, due, finish, poll_with, start};
use super::report::Failure;
use super::testkit::Fx;
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

// ---- E5's minimum spacing, coalescing and the poll's measure (gh-budget-and-swr; contract §3.7 E5) -------------------

/// An engine on fakes (no real `gh`, `cpd`, git or tmux), `pr_poll_seconds` = 30 so the spacing is `max(5, 30 / 3)` = 10 s,
/// whose last poll ended now.
fn spaced() -> (Fx, std::time::Instant) {
    let mut fx = Fx::new("[watch]\npr_poll_seconds = 30\n");
    let fakes = cpd_core::ports::Fakes::new();
    fakes.spawn.set_self_exe(EXE);
    let _ = (*fakes.runner)
        .clone()
        .on("cpd", |_| true, Reply::ok(&printed_ok()));
    fx.engine.ports = fakes.ports.clone();
    let ended = fx.engine.ports.clock.now();
    fx.engine.last_poll_end = Some(ended);
    (fx, ended)
}

#[test]
fn poll_now_within_min_spacing_waits_for_spacing() {
    let (fx, ended) = spaced();
    cpd_core::pr::request_poll_now_in(&fx.engine.home);
    assert_eq!(
        due(&fx.engine, ended),
        Some(ended + Duration::from_secs(10)),
        "a request moves the poll no sooner than max(5 s, pr_poll_seconds / 3) after the last poll ended"
    );
}

#[test]
fn burst_of_poll_now_requests_costs_one_poll() {
    let (mut fx, ended) = spaced();
    for _ in 0..5 {
        cpd_core::pr::request_poll_now_in(&fx.engine.home);
    }
    // Inside the spacing no poll starts, and the one flag is kept for the poll that will serve the whole burst.
    start(&mut fx.engine);
    let started_early = fx.engine.poll.is_some();
    finish(&mut fx.engine);
    assert!(!started_early, "no poll starts inside the minimum spacing");
    assert!(
        fx.engine
            .home
            .run_dir()
            .join(cpd_core::pr::POLL_NOW)
            .exists(),
        "the request waits for the spacing"
    );
    assert_eq!(
        due(&fx.engine, ended),
        Some(ended + Duration::from_secs(10))
    );
}

#[test]
fn poll_logs_cost_remaining_and_calls_by_kind() {
    let (mut fx, ended) = spaced();
    let out = json!({
        "ok": true, "polled": 3, "changed": [], "base_changed": [], "feedback": [], "merged": [], "done": [],
        "rate_limit": {"cost": 7, "remaining": 4900, "reset_at": "2030-01-01T00:00:00.000Z"},
        "calls": {"search": 1, "absent": 2, "comments": 3, "checks": 4, "deploys": 5},
    });
    apply(
        &mut fx.engine,
        Finished {
            result: Ok(out),
            ended,
        },
    );
    let log = std::fs::read_to_string(fx.engine.home.watch_log()).unwrap();
    assert!(
        log.contains(
            "E5: polled 3 pull request(s): 0 changed, 0 feedback, 0 merged, 0 plan(s) done; cost 7, remaining 4900, search 1, absent 2, comments 3, checks 4, deploys 5"
        ),
        "a poll that changed nothing still writes its measure line: {log}"
    );
}
