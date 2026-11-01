//! `catalog::read_with` on a [`FakeRunner`]: the Node call it makes and how it maps each answer.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::catalog::{PiInstall, node, read_with};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::{FakeRunner, Reply};

/// The deadline the catalog module runs under.
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
    assert!(
        args[2].contains("getSupportedThinkingLevels"),
        "the embedded module"
    );
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
    assert!(
        call.envs.contains(&(
            "PI_MANAGED_INSTALL_ROOT".to_owned(),
            Some("/nonexistent/agent/install".to_owned())
        )),
        "{:?}",
        call.envs
    );
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
    assert!(
        err.message()
            .starts_with(&format!("cannot run {}: ", node().display())),
        "{}",
        err.message()
    );
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
    assert!(
        err.message()
            .starts_with("the catalog module printed invalid JSON: "),
        "{}",
        err.message()
    );
    assert_eq!(err.details()["stdout"], "garbage");
    assert_eq!(err.details()["stderr"], "warn");
}

#[test]
fn read_with_maps_a_missing_tier_letter_to_catalog_failed() {
    let (_, result) = read(Reply::ok(
        &json!({"O": ["low"], "S": ["low"], "H": ["low"]}).to_string(),
    ));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert_eq!(
        err.message(),
        "the catalog module printed nothing for Tier G."
    );
    assert_eq!(err.details()["letter"], "G");
}

#[test]
fn read_with_maps_a_model_pi_does_not_have_to_tier_model_unknown() {
    let (_, result) = read(Reply::ok(
        &json!({"O": ["low"], "S": null, "H": ["low"], "G": ["low"]}).to_string(),
    ));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::TierModelUnknown);
    assert_eq!(err.details()["missing"][0]["letter"], "S");
    assert_eq!(err.details()["pi_version"], "9.9.9");
}

#[test]
fn read_with_maps_an_unknown_thinking_level_to_catalog_failed() {
    let (_, result) = read(Reply::ok(
        &json!({"O": ["warp"], "S": ["low"], "H": ["low"], "G": []}).to_string(),
    ));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert!(
        err.message()
            .starts_with("pi reports the unknown thinking level \"warp\""),
        "{}",
        err.message()
    );
}

#[test]
fn read_with_maps_a_model_with_no_usable_level_to_catalog_failed() {
    let (_, result) = read(Reply::ok(
        &json!({"O": ["minimal"], "S": ["low"], "H": ["low"], "G": []}).to_string(),
    ));
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed);
    assert!(
        err.message()
            .starts_with("pi reports no usable thinking level for "),
        "{}",
        err.message()
    );
}

#[test]
fn read_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        "node",
        |_| true,
        Reply::ok(&levels()).takes(Duration::from_secs(31)),
    );
    let err = read_with(&runner, &pi(), &models(), DEADLINE).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["tool"], "node");
    assert_eq!(err.details()["deadline_seconds"], 30);
    assert_eq!(clock.elapsed(), DEADLINE);
}

// ---------- the runner toggle (pro-d199) ----------

use super::{Config, TOGGLE_COMMENT, WorkerRunner, set_in_text, toggle};
use crate::home::CpdHome;

fn home_with(text: &str) -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::write(home.config(), text).unwrap();
    (dir, home)
}

fn line(to: &str) -> String {
    format!("worker_runner = \"{to}\" # {TOGGLE_COMMENT}")
}

#[test]
fn flipped_swaps_pi_and_durable() {
    assert_eq!(WorkerRunner::Pi.flipped(), WorkerRunner::Durable);
    assert_eq!(WorkerRunner::Durable.flipped(), WorkerRunner::Pi);
}

#[test]
fn set_in_text_replaces_the_whole_line_and_its_comment_and_keeps_every_other_byte() {
    let before = "# mine\r\nversion = 1\nmachine = \"hs\"\nworker_runner = \"pi\"   # old comment\nconcurrency = 5\n\n[timeouts]\nattempt_minutes = 30";
    let want = format!(
        "# mine\r\nversion = 1\nmachine = \"hs\"\n{}\nconcurrency = 5\n\n[timeouts]\nattempt_minutes = 30",
        line("durable")
    );
    assert_eq!(set_in_text(before, WorkerRunner::Durable), want);
}

#[test]
fn set_in_text_inserts_the_line_before_the_first_table_when_it_is_absent() {
    let before = "version = 1\nmachine = \"hs\"  # the machine\n\n# timeouts\n[timeouts]\nattempt_minutes = 30\n";
    let want = format!(
        "version = 1\nmachine = \"hs\"  # the machine\n{}\n\n# timeouts\n[timeouts]\nattempt_minutes = 30\n",
        line("durable")
    );
    assert_eq!(set_in_text(before, WorkerRunner::Durable), want);
}

#[test]
fn set_in_text_inserts_the_line_at_the_end_when_there_is_no_table() {
    let want = format!("version = 1\nmachine = \"hs\"\n{}\n", line("pi"));
    assert_eq!(
        set_in_text("version = 1\nmachine = \"hs\"\n", WorkerRunner::Pi),
        want
    );
    let want = format!("version = 1\nmachine = \"hs\"\n{}\n", line("pi"));
    assert_eq!(
        set_in_text("version = 1\nmachine = \"hs\"", WorkerRunner::Pi),
        want
    );
}

#[test]
fn set_in_text_leaves_a_worker_runner_key_of_a_table_alone() {
    let before = "version = 1\nmachine = \"hs\"\n\n[tmux]\nworker_runner = \"pi\"\n";
    let want = format!(
        "version = 1\nmachine = \"hs\"\n{}\n\n[tmux]\nworker_runner = \"pi\"\n",
        line("durable")
    );
    assert_eq!(set_in_text(before, WorkerRunner::Durable), want);
}

#[test]
fn toggle_writes_the_other_value_atomically_and_returns_it() {
    let (dir, home) =
        home_with("version = 1\nmachine = \"hs\"\n\n[timeouts]\nattempt_minutes = 30\n");
    assert_eq!(toggle(&home).unwrap(), WorkerRunner::Durable);
    let text = std::fs::read_to_string(home.config()).unwrap();
    assert_eq!(
        text,
        set_in_text(
            "version = 1\nmachine = \"hs\"\n\n[timeouts]\nattempt_minutes = 30\n",
            WorkerRunner::Durable
        )
    );
    assert_eq!(
        Config::load(&home).unwrap().worker_runner,
        WorkerRunner::Durable
    );
    // Atomic: the temporary file was renamed away, so the directory holds config.toml alone.
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["config.toml"]);
}

#[test]
fn toggle_flips_back_on_the_second_call() {
    let (_dir, home) = home_with("version = 1\nmachine = \"hs\"\nworker_runner = \"durable\"\n");
    assert_eq!(toggle(&home).unwrap(), WorkerRunner::Pi);
    assert_eq!(Config::load(&home).unwrap().worker_runner, WorkerRunner::Pi);
    assert_eq!(toggle(&home).unwrap(), WorkerRunner::Durable);
    assert_eq!(
        Config::load(&home).unwrap().worker_runner,
        WorkerRunner::Durable
    );
}

#[test]
fn toggle_refuses_a_config_that_does_not_parse_and_leaves_the_file() {
    let broken = "version = 1\nmachine = \"hs\"\nconcurrency = \"many\"\n";
    let (_dir, home) = home_with(broken);
    let err = toggle(&home).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    assert_eq!(std::fs::read_to_string(home.config()).unwrap(), broken);
}

// ---------- lead_runner and manager_runner (contract §6.19, P1.10) ----------

use std::path::Path as StdPath;

#[test]
fn lead_and_manager_runner_default_to_pi_and_parse() {
    let path = StdPath::new("config.toml");
    let (config, _) = Config::parse("version = 1\nmachine = \"m1\"\n", path)
        .expect("a config without the keys parses");
    assert_eq!(config.lead_runner, WorkerRunner::Pi);
    assert_eq!(config.manager_runner, WorkerRunner::Pi);

    // `lead_runner = "durable"` parses; the two keys are independent (Q11).
    let (config, _) = Config::parse(
        "version = 1\nmachine = \"m1\"\nlead_runner = \"durable\"\n",
        path,
    )
    .expect("lead_runner = durable parses");
    assert_eq!(config.lead_runner, WorkerRunner::Durable);
    assert_eq!(config.manager_runner, WorkerRunner::Pi);

    // An unknown key is still refused (`r.finish`).
    let err =
        Config::parse("version = 1\nmachine = \"m1\"\nnot_a_key = \"x\"\n", path).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
}
