//! Smoke tests of the real `cpd` binary with `assert_cmd` (ADR 0013): the only tests, besides the named bootstrap
//! scripts', that exec it. Everything external is absent: the environment is cleared, `HOME` and `CPD_HOME` are a
//! temp dir, `PATH` is empty (no tmux, git or pi), and no command here reaches the network or waits.

use assert_cmd::Command;
use serde_json::Value;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The `CPD_COMMIT` the build of these tests was given: the same cargo run built `cpd`.
const BUILD_COMMIT: Option<&str> = option_env!("CPD_COMMIT");

/// `cpd` with a cleared environment: `HOME` and `PATH` explicit, `CPD_HOME` under `home`, `PATH` empty.
fn cpd(home: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("cpd").expect("the cpd binary");
    cmd.env_clear()
        .env("HOME", home.path())
        .env("CPD_HOME", home.path().join("cpd"))
        .env("PATH", "");
    cmd
}

fn temp_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temp dir")
}

/// The one JSON value on stdout of a run that exited with `code`.
fn json(cmd: &mut Command, code: i32) -> Value {
    let out = cmd.assert().code(code).get_output().clone();
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {out:?}"))
}

#[test]
fn version_prints_the_commit_its_build_was_given_or_dev() {
    let home = temp_home();
    let expected = match BUILD_COMMIT {
        Some(commit) if !commit.is_empty() => format!("{VERSION}-{commit}"),
        _ => format!("{VERSION}-dev"),
    };
    let out = json(cpd(&home).arg("--version"), 0);
    assert_eq!(out["ok"], true);
    assert_eq!(
        out["version"],
        expected.as_str(),
        "built with CPD_COMMIT={BUILD_COMMIT:?}"
    );
}

#[test]
fn help_lists_the_commands() {
    let home = temp_home();
    let out = json(cpd(&home).arg("--help"), 0);
    let help = out["help"].as_str().expect("help text");
    for command in [
        "context", "plan", "job", "attempt", "session", "config", "doctor",
    ] {
        assert!(
            help.contains(&format!("\n  {command} ")),
            "no {command} in: {help}"
        );
    }
}

#[test]
fn install_help_lists_the_root_option() {
    let home = temp_home();
    let out = json(cpd(&home).args(["install", "--help"]), 0);
    let help = out["help"].as_str().expect("help text");
    assert!(help.contains("--root"), "no --root in: {help}");
}

#[test]
fn an_unknown_command_is_a_usage_error_with_the_usage_on_stdout() {
    let home = temp_home();
    let out = json(cpd(&home).arg("bogus"), 2);
    assert_eq!(out["ok"], false);
    assert_eq!(out["error"]["code"], "usage");
    assert_eq!(out["error"]["message"], "unrecognized subcommand 'bogus'");
    assert!(
        out["error"]["details"]["help"]
            .as_str()
            .unwrap()
            .contains("Usage: cpd"),
        "{out}"
    );
}

#[test]
fn a_relative_cpd_home_is_refused_as_usage() {
    let home = temp_home();
    let out = json(cpd(&home).env("CPD_HOME", "relative").arg("config"), 2);
    assert_eq!(out["error"]["code"], "usage");
    assert_eq!(
        out["error"]["message"],
        "CPD_HOME must be an absolute path, got \"relative\"."
    );
    assert_eq!(out["error"]["details"]["variable"], "CPD_HOME");
}

#[test]
fn config_in_a_temp_home_prints_the_config() {
    let home = temp_home();
    let cpd_home = home.path().join("cpd");
    std::fs::create_dir_all(&cpd_home).unwrap();
    std::fs::write(
        cpd_home.join("config.toml"),
        "version = 1\nmachine = \"smoke\"\nlead_pi_packages = []\n",
    )
    .unwrap();
    let out = json(cpd(&home).arg("config"), 0);
    assert_eq!(out["ok"], true);
    assert_eq!(out["config"]["machine"], "smoke");
    assert_eq!(out["path"], cpd_home.join("config.toml").to_str().unwrap());
}

#[test]
fn a_command_that_needs_tmux_with_an_empty_path_fails_as_tmux_failed_not_a_panic() {
    let home = temp_home();
    let cpd_home = home.path().join("cpd");
    std::fs::create_dir_all(&cpd_home).unwrap();
    std::fs::write(
        cpd_home.join("config.toml"),
        "version = 1\nmachine = \"smoke\"\nlead_pi_packages = []\n",
    )
    .unwrap();
    let out = json(cpd(&home).args(["session", "park"]), 1);
    assert_eq!(out["error"]["code"], "tmux_failed");
    assert_eq!(out["error"]["details"]["argv"][0], "tmux");
    assert!(
        out["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Cannot run tmux"),
        "{out}"
    );
}
