//! `catalog::read_with` on a [`FakeRunner`]: the Node call it makes and how it maps each answer.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::catalog::{PiInstall, config_models, node, read_with};
use super::file::Config;
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::{FakeRunner, Reply};

const DEADLINE: Duration = Duration::from_secs(30);

fn pi() -> PiInstall {
    PiInstall {
        agent_dir: PathBuf::from("/nonexistent/agent"),
        version: "9.9.9".into(),
        coding_agent_entry: PathBuf::from("/nonexistent/coding-agent.js"),
        pi_ai_entry: PathBuf::from("/nonexistent/pi-ai.js"),
    }
}

fn models() -> crate::policy::model::Models {
    crate::policy::default_policy().model.clone()
}

/// Runs the node program on the scripted reply; the runner is returned to read the record.
fn read(reply: Reply) -> (FakeRunner, Result<super::Catalog, CpdError>) {
    let runner = FakeRunner::new(ManualClock::new()).on("node", |_| true, reply);
    let result = read_with(&runner, &pi(), &models(), DEADLINE);
    (runner, result)
}

fn levels() -> String {
    json!({
        "O": ["off", "low", "medium", "high"],
        "S": ["minimal", "off", "low"],
        "H": ["off", "low", "medium"],
        "G": ["off", "low"],
    })
    .to_string()
}

#[test]
fn read_with_asks_the_runner_for_node_running_the_module_on_pis_entries_and_the_tiers() {
    let (runner, _) = read(Reply::ok(&levels()));
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "node");
    assert_eq!(calls[0].program, node().display().to_string());
    let args = &calls[0].args;
    assert_eq!(&args[..2], ["--input-type=module", "-e"]);
    assert!(args[2].contains("getSupportedThinkingLevels"), "the embedded module");
    let models = models();
    assert_eq!(
        &args[3..],
        [
            "--".to_owned(),
            "/nonexistent/coding-agent.js".to_owned(),
            "/nonexistent/pi-ai.js".to_owned(),
            format!("O={}", models.o),
            format!("S={}", models.s),
            format!("H={}", models.h),
            format!("G={}", models.g),
        ]
    );
}

#[test]
fn read_with_runs_node_in_the_agent_dir_with_the_managed_install_root_and_the_given_deadline() {
    let (runner, _) = read(Reply::ok(&levels()));
    let call = &runner.calls()[0];
    assert_eq!(call.deadline, DEADLINE);
    assert_eq!(call.cwd.as_deref(), Some(Path::new("/nonexistent/agent")));
    assert!(call.envs.contains(&("PI_MANAGED_INSTALL_ROOT".to_owned(), Some("/nonexistent/agent/install".to_owned()))), "{:?}", call.envs);
}

#[test]
fn read_with_builds_the_catalog_from_a_scripted_success() {
    let (_, result) = read(Reply::ok(&levels()));
    let catalog = result.unwrap();
    assert_eq!(catalog.pi_version, "9.9.9");
    assert_eq!(catalog.o.efforts, vec![0, 1, 2, 3]);
    assert_eq!(catalog.s.efforts, vec![0, 1], "minimal has no digit");
    assert_eq!(catalog.h.efforts, vec![0, 1, 2]);
    assert!(catalog.g.efforts.is_empty(), "G has no effort levels");
}

#[test]
fn read_with_maps_a_node_that_cannot_start_to_catalog_failed_naming_it() {
    let (_, result) = read(Reply::io(ErrorKind::NotFound));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert!(err.message().starts_with(&format!("cannot run {}: ", node().display())), "{}", err.message());
    assert_eq!(err.details()["node"], node().display().to_string());
}

#[test]
fn read_with_maps_a_failing_module_to_catalog_failed_with_node_exit_and_stderr() {
    let stderr: String = (1..=70).map(|n| format!("e{n}\n")).collect();
    let (_, result) = read(Reply::exit(1, "", &stderr));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert_eq!(err.message(), "the catalog module failed under pi 9.9.9.");
    assert_eq!(err.details()["node"], node().display().to_string());
    assert_eq!(err.details()["exit"], 1);
    let tail = err.details()["stderr"].as_str().unwrap();
    assert_eq!(tail.lines().count(), 60);
    assert!(tail.ends_with("e70"), "{tail}");
}

#[test]
fn read_with_maps_stdout_that_is_not_json_to_catalog_failed_with_stdout_and_stderr() {
    let (_, result) = read(Reply::exit(0, "garbage", "warn"));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert!(err.message().starts_with("the catalog module printed invalid JSON: "), "{}", err.message());
    assert_eq!(err.details()["stdout"], "garbage");
    assert_eq!(err.details()["stderr"], "warn");
}

#[test]
fn read_with_maps_a_missing_tier_letter_to_catalog_failed() {
    let (_, result) = read(Reply::ok(&json!({"O": ["low"], "S": ["low"], "H": ["low"]}).to_string()));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert_eq!(err.message(), "the catalog module printed nothing for Tier G.");
    assert_eq!(err.details()["letter"], "G");
}

#[test]
fn read_with_maps_a_model_pi_does_not_have_to_tier_model_unknown() {
    let (_, result) = read(Reply::ok(&json!({"O": ["low"], "S": null, "H": ["low"], "G": ["low"]}).to_string()));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::TierModelUnknown);
    assert_eq!(err.details()["missing"][0]["letter"], "S");
    assert_eq!(err.details()["pi_version"], "9.9.9");
}

#[test]
fn read_with_maps_an_unknown_thinking_level_to_catalog_failed() {
    let (_, result) = read(Reply::ok(&json!({"O": ["warp"], "S": ["low"], "H": ["low"], "G": []}).to_string()));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert!(err.message().starts_with("pi reports the unknown thinking level \"warp\""), "{}", err.message());
}

#[test]
fn read_with_maps_a_model_with_no_usable_level_to_catalog_failed() {
    let (_, result) = read(Reply::ok(&json!({"O": ["minimal"], "S": ["low"], "H": ["low"], "G": []}).to_string()));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert!(err.message().starts_with("pi reports no usable thinking level for "), "{}", err.message());
}

#[test]
fn read_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("node", |_| true, Reply::ok(&levels()).takes(Duration::from_secs(31)));
    let err = read_with(&runner, &pi(), &models(), DEADLINE).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["tool"], "node");
    assert_eq!(err.details()["deadline_seconds"], 30);
    assert_eq!(clock.elapsed(), DEADLINE);
}
