//! The engine's two child processes on a [`FakeRunner`]: `pr::poll_with` (`cpd pr poll`) and `team::start_with` (the
//! Slack helper under pi's Node). The call each makes, and how it maps a scripted success, failure and timeout.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::pr::poll_with;
use super::report::Failure;
use super::team::start_with;
use super::testkit::Fx;
use cpd_core::clock::ManualClock;
use cpd_core::config::PiInstall;
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
    json!({"ok": true, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}}).to_string()
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
    assert!(call.envs.contains(&("CPD_HOME".to_owned(), Some(ROOT.to_owned()))), "{:?}", call.envs);
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert!(call.envs.contains(&(removed.to_owned(), None)), "{removed}: {:?}", call.envs);
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
    assert!(failure.message.starts_with(&format!("Cannot run {EXE} pr poll: ")), "{}", failure.message);
    assert_eq!(failure.details["argv"], json!([EXE, "pr", "poll"]));
}

#[test]
fn poll_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("cpd", |_| true, Reply::ok(&printed_ok()).takes(Duration::from_secs(121)));
    let failure = poll(&runner).unwrap_err();
    assert_eq!(failure.code, "tool_timeout");
    assert_eq!(failure.details["tool"], "cpd");
    assert_eq!(failure.details["argv"], json!([EXE, "pr", "poll"]));
    assert_eq!(failure.details["deadline_seconds"], 120);
    assert_eq!(clock.elapsed(), Duration::from_secs(120));
}

// ---- watch::team -------------------------------------------------------------------------------------------------

fn pi() -> PiInstall {
    PiInstall {
        agent_dir: PathBuf::from("/pi/agent"),
        version: "9.9.9".into(),
        coding_agent_entry: PathBuf::from("/pi/agent/coding-agent.js"),
        pi_ai_entry: PathBuf::from("/pi/agent/pi-ai.js"),
    }
}

const NODE: &str = "/pi-node/bin/node";
/// An engine whose config names the team channel and a pi adapter directory (which must hold a `package.json`).
fn engine() -> (Fx, tempfile::TempDir) {
    let adapter = tempfile::tempdir().unwrap();
    std::fs::write(adapter.path().join("package.json"), "{}").unwrap();
    let extra = format!(
        "pi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C123ABC\"\nslack_user_id = \"U123ABC\"\n",
        adapter.path().display()
    );
    (Fx::new(&extra), adapter)
}

fn script(adapter: &tempfile::TempDir) -> String {
    adapter.path().join("lib").join("slack-channel.ts").display().to_string()
}

/// Starts the helper on `runner` and waits for its answer (a channel receive, no clock).
fn helper(runner: &FakeRunner, fx: &Fx) -> Result<Value, Failure> {
    let rx = start_with(Arc::new(runner.clone()), &fx.engine, &pi(), Path::new(NODE)).unwrap();
    rx.recv().expect("the helper's thread answers")
}

#[test]
fn start_with_asks_the_runner_for_node_running_the_slack_helper_with_the_channel_and_the_user() {
    let (fx, adapter) = engine();
    let runner = fake("node", Reply::ok(&json!({"ok": true, "posts": [], "read": 0}).to_string()));
    helper(&runner, &fx).unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "node");
    assert_eq!(calls[0].program, NODE);
    assert_eq!(
        calls[0].args,
        [script(&adapter).as_str(), "--coding-agent", "/pi/agent/coding-agent.js", "--channel", "C123ABC", "--me", "U123ABC"]
    );
}

#[test]
fn start_with_gives_the_helper_two_minutes_the_home_as_its_directory_and_pis_environment() {
    let (fx, _adapter) = engine();
    let runner = fake("node", Reply::ok(&json!({"ok": true}).to_string()));
    helper(&runner, &fx).unwrap();
    let call = &runner.calls()[0];
    let root = fx.engine.home.root().to_owned();
    assert_eq!(call.deadline, Duration::from_secs(120));
    assert_eq!(call.cwd.as_deref(), Some(root.as_path()));
    assert!(call.envs.contains(&("PI_CODING_AGENT_DIR".to_owned(), Some("/pi/agent".to_owned()))), "{:?}", call.envs);
    assert!(call.envs.contains(&("CPD_HOME".to_owned(), Some(root.display().to_string()))), "{:?}", call.envs);
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert!(call.envs.contains(&(removed.to_owned(), None)), "{removed}: {:?}", call.envs);
    }
}

#[test]
fn start_with_answers_the_value_the_helper_printed_on_success() {
    let (fx, _adapter) = engine();
    let runner = fake("node", Reply::ok(&json!({"ok": true, "posts": [{"ts": "1"}], "read": 3}).to_string()));
    let value = helper(&runner, &fx).unwrap();
    assert_eq!(value["read"], 3);
    assert_eq!(value["posts"].as_array().unwrap().len(), 1);
}

#[test]
fn start_with_answers_the_failure_the_helper_printed() {
    let (fx, _adapter) = engine();
    let printed = json!({"ok": false, "error": {"code": "slack_helper_failed", "message": "no token", "details": {}}}).to_string();
    let runner = fake("node", Reply::exit(1, &printed, ""));
    let failure = helper(&runner, &fx).unwrap_err();
    assert_eq!(failure.code, "slack_helper_failed");
    assert_eq!(failure.message, "no token");
}

#[test]
fn start_with_maps_a_helper_that_printed_no_result_to_spawn_failed() {
    let (fx, _adapter) = engine();
    let runner = fake("node", Reply::exit(3, "", "boom"));
    let failure = helper(&runner, &fx).unwrap_err();
    assert_eq!(failure.code, "spawn_failed");
    assert!(failure.message.ends_with("printed no result."), "{}", failure.message);
    assert_eq!(failure.details["exit"], 3);
    assert_eq!(failure.details["stderr"], "boom");
}

#[test]
fn start_with_names_a_helper_that_exited_zero_with_no_output() {
    let (fx, _adapter) = engine();
    let runner = fake("node", Reply::exit(0, "", ""));
    let failure = helper(&runner, &fx).unwrap_err();
    assert_eq!(failure.code, "spawn_failed");
    assert!(failure.message.ends_with(" exited 0 but printed no result."), "{}", failure.message);
    assert_eq!(failure.details["exit"], 0);

    let runner = fake("node", Reply::exit(3, "", "boom"));
    let failure = helper(&runner, &fx).unwrap_err();
    assert!(!failure.message.contains("exited 0"), "{}", failure.message);
    assert!(failure.message.ends_with("printed no result."), "{}", failure.message);
}

#[test]
fn start_with_maps_a_node_that_cannot_start_to_spawn_failed_with_the_argv() {
    let (fx, adapter) = engine();
    let runner = fake("node", Reply::io(ErrorKind::NotFound));
    let failure = helper(&runner, &fx).unwrap_err();
    assert_eq!(failure.code, "spawn_failed");
    assert!(failure.message.starts_with(&format!("Cannot run {NODE} {} ", script(&adapter))), "{}", failure.message);
    assert_eq!(failure.details["argv"][0], NODE);
    assert_eq!(failure.details["argv"][1], script(&adapter).as_str());
}

#[test]
fn start_with_maps_a_scripted_timeout_to_tool_timeout() {
    let (fx, _adapter) = engine();
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("node", |_| true, Reply::ok("{}").takes(Duration::from_secs(500)));
    let failure = helper(&runner, &fx).unwrap_err();
    assert_eq!(failure.code, "tool_timeout");
    assert_eq!(failure.details["tool"], "node");
    assert_eq!(failure.details["deadline_seconds"], 120);
    assert_eq!(clock.elapsed(), Duration::from_secs(120));
}
