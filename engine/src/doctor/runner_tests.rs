//! The doctor's tool probes (`tools_with`, `gh_auth_with`, `pi_node_with`) on a [`FakeRunner`]: the call each makes and
//! how it maps a scripted success, failure and timeout.

use std::io::ErrorKind;
use std::time::Duration;

use serde_json::json;

use super::DEADLINE;
use super::check::{Check, Severity};
use super::tools::{gh_auth_with, pi_node_with, tools_with};
use cpd_core::clock::ManualClock;
use cpd_core::config::catalog::node;
use cpd_core::host::runner::{FakeRunner, Reply};

fn fake() -> FakeRunner {
    FakeRunner::new(ManualClock::new())
}

/// A runner that answers the four version probes with versions that pass.
fn healthy() -> FakeRunner {
    fake()
        .on("git", |_| true, Reply::ok("git version 2.43.0\n"))
        .on("tmux", |_| true, Reply::ok("tmux 3.6\n"))
        .on("pi", |_| true, Reply::ok("1.2.0\n"))
        .on("gh", |_| true, Reply::ok("gh version 2.45.0 (2025-07-18)\n"))
}

fn named<'a>(checks: &'a [Check], name: &str) -> &'a Check {
    checks.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no check {name}: {checks:?}"))
}

#[test]
fn tools_with_asks_the_runner_for_each_probe_in_order() {
    let runner = healthy();
    tools_with(&runner);
    let seen: Vec<(String, String, Vec<String>)> = runner.calls().into_iter().map(|c| (c.tool, c.program, c.args)).collect();
    let call = |tool: &str, args: &[&str]| (tool.to_owned(), tool.to_owned(), args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());
    assert_eq!(seen, vec![call("git", &["--version"]), call("tmux", &["-u", "-V"]), call("pi", &["--version"]), call("gh", &["--version"])]);
}

#[test]
fn tools_with_gives_every_probe_the_doctor_deadline() {
    let runner = healthy();
    tools_with(&runner);
    assert!(runner.calls().iter().all(|c| c.deadline == DEADLINE), "{:?}", runner.calls());
    assert_eq!(DEADLINE, Duration::from_secs(10));
}

#[test]
fn tools_with_probes_tmux_as_a_utf8_client_without_tmux_in_its_environment() {
    let runner = healthy();
    tools_with(&runner);
    let tmux = runner.calls().into_iter().find(|c| c.tool == "tmux").unwrap();
    assert_eq!(tmux.args.first().map(String::as_str), Some("-u"));
    assert!(tmux.envs.contains(&("TMUX".to_owned(), None)), "{:?}", tmux.envs);
    assert!(tmux.envs.contains(&("TMUX_PANE".to_owned(), None)), "{:?}", tmux.envs);
}

#[test]
fn tools_with_passes_the_versions_that_are_at_or_above_the_minimum() {
    let checks = tools_with(&healthy());
    assert_eq!(checks.len(), 4);
    for check in &checks {
        assert!(check.ok, "{check:?}");
        assert_eq!(check.severity, Severity::Error);
    }
    assert_eq!(named(&checks, "git").found, Some(json!("2.43.0")));
    assert_eq!(named(&checks, "tmux").found, Some(json!("3.6")));
}

#[test]
fn tools_with_fails_a_version_below_the_minimum_with_what_it_found() {
    let runner = fake().on("git", |_| true, Reply::ok("git version 2.30.0\n"));
    let checks = tools_with(&runner);
    let git = named(&checks, "git");
    assert!(!git.ok);
    assert_eq!(git.found, Some(json!("2.30.0")));
    assert_eq!(git.expected, Some(json!(">= 2.43.0")));
    assert_eq!(git.message.as_deref(), Some("git is outside the versions cpd 2 was verified with."));
}

#[test]
fn tools_with_passes_a_tmux_development_build() {
    let runner = fake().on("tmux", |_| true, Reply::ok("tmux next-3.5\n"));
    let checks = tools_with(&runner);
    let tmux = named(&checks, "tmux");
    assert!(tmux.ok);
    assert_eq!(tmux.found, Some(json!("next-3.5")));
    drop(runner);
}

#[test]
fn tools_with_fails_an_output_with_no_version() {
    let runner = fake().on("git", |_| true, Reply::ok("hello\n"));
    let checks = tools_with(&runner);
    assert_eq!(named(&checks, "git").message.as_deref(), Some("no version in the output of `git --version`: hello"));
}

#[test]
fn tools_with_fails_a_tool_that_is_not_found() {
    let checks = tools_with(&fake());
    let git = named(&checks, "git");
    assert!(!git.ok);
    assert_eq!(git.message.as_deref(), Some("`git` was not found."));
    assert_eq!(git.expected, Some(json!(">= 2.43.0")));
}

#[test]
fn tools_with_fails_a_tool_that_cannot_run_with_the_io_error() {
    let runner = fake().on("git", |_| true, Reply::io(ErrorKind::PermissionDenied));
    let checks = tools_with(&runner);
    let message = named(&checks, "git").message.clone().unwrap();
    assert!(message.starts_with("cannot run `git`: "), "{message}");
}

#[test]
fn tools_with_fails_a_tool_that_exits_non_zero_with_its_first_line() {
    let runner = fake().on("git", |_| true, Reply::exit(1, "", "boom\nsecond\n"));
    let checks = tools_with(&runner);
    assert_eq!(named(&checks, "git").message.as_deref(), Some("`git --version` exited 1: boom"));
}

#[test]
fn tools_with_fails_a_probe_that_runs_past_the_deadline_with_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("git", |_| true, Reply::ok("git version 2.43.0").takes(Duration::from_secs(11)));
    let checks = tools_with(&runner);
    let git = named(&checks, "git");
    assert!(!git.ok);
    assert_eq!(
        git.message.as_deref(),
        Some("tool_timeout: git ran past its deadline of 10s and was killed: git --version")
    );
    assert_eq!(clock.elapsed(), DEADLINE);
}

#[test]
fn gh_auth_with_asks_the_runner_for_gh_auth_status_and_passes_on_exit_zero() {
    let runner = fake().on("gh", |_| true, Reply::ok("Logged in\n"));
    let check = gh_auth_with(&runner);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].tool.as_str(), calls[0].args.clone()), ("gh", vec!["auth".to_owned(), "status".to_owned()]));
    assert_eq!(calls[0].deadline, DEADLINE);
    assert_eq!(check, Check::pass("gh_auth", Severity::Warn));
}

#[test]
fn gh_auth_with_is_a_warning_when_gh_is_not_logged_in() {
    let runner = fake().on("gh", |_| true, Reply::exit(1, "", "You are not logged in\n"));
    let check = gh_auth_with(&runner);
    assert_eq!(check.name, "gh_auth");
    assert!(!check.ok);
    assert_eq!(check.severity, Severity::Warn);
    assert_eq!(check.message.as_deref(), Some("`gh auth status` exited 1: You are not logged in"));
}

#[test]
fn gh_auth_with_is_a_warning_on_a_scripted_timeout() {
    let runner = fake().on("gh", |_| true, Reply::ok("").takes(DEADLINE));
    let check = gh_auth_with(&runner);
    assert!(!check.ok);
    assert!(check.message.unwrap().starts_with("tool_timeout: gh ran past its deadline of 10s"));
}

#[test]
fn pi_node_with_asks_the_runner_for_pis_node_version_and_reports_what_it_printed() {
    let runner = fake().on("node", |_| true, Reply::ok("v22.23.2\n"));
    let check = pi_node_with(&runner);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "node");
    assert_eq!(calls[0].program, node().display().to_string());
    assert_eq!(calls[0].args, ["--version"]);
    assert_eq!(calls[0].deadline, DEADLINE);
    assert!(check.ok);
    assert_eq!(check.severity, Severity::Error);
    assert_eq!(check.found, Some(json!("v22.23.2")));
    assert_eq!(check.message, Some(format!("ran {}", node().display())));
}

#[test]
fn pi_node_with_fails_when_node_is_not_found() {
    let check = pi_node_with(&fake());
    assert_eq!(check.name, "pi_node");
    assert!(!check.ok);
    assert_eq!(check.message, Some(format!("`{}` was not found.", node().display())));
}

// ---------- the host_runner check (contract §6.19, P1.10) ----------

use super::host_runner;
use cpd_core::config::{Config, WorkerRunner};

#[test]
fn host_runner_check_fails_without_the_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = dir.path().join("pi");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    let text = format!("version = 1\nmachine = \"m1\"\npi_adapter = {:?}\n", adapter.display().to_string());
    let (mut config, _) = Config::parse(&text, std::path::Path::new("config.toml")).unwrap();
    config.lead_runner = WorkerRunner::Durable;

    let check = host_runner::check(&config);
    assert_eq!(check.name, "host_runner");
    assert_eq!(check.severity, Severity::Error);
    assert!(!check.ok, "durable without {} fails", adapter.join("durable-host").join("main.ts").display());
}
