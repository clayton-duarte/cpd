//! `cpd config` and `cpd tiers` (commands.md §14.2, §14.3; contract §8, §9):
//! a temporary `$CPD_HOME`, and a fake pi install. Every test runs in this process (`Fx`); `node` is a reply of the
//! `FakeRunner` and a migration in progress waits on the `ManualClock`. One test, `tiers_with_a_model_unknown_to_real_pi_modules_is_tier_model_unknown`,
//! stays on the built binary and the real `node`: it runs pi's own module API (OS behaviour of an external).

use crate::common;

use std::fs;
use std::os::unix::fs::symlink;
use std::io::ErrorKind;
use std::time::Duration;

use common::fx::Fx;
use common::{CPD, Run, TestEnv, write_executable};
use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

const MINIMAL: &str = "version = 1\nmachine = \"hs\"\n";

fn config_with(fx: &Fx, text: &str) -> Run {
    fx.env.write_config(text);
    fx.cpd(&["config"])
}

// ---------------------------------------------------------------- config

#[test]
fn config_missing_file_is_config_missing() {
    let fx = Fx::new();
    let run = fx.cpd(&["config"]);
    let err = run.error();
    assert_eq!(run.exit, 1);
    assert_eq!(err["code"], "config_missing");
    assert_eq!(err["details"]["path"], fx.env.cpd_home().join("config.toml").to_str().unwrap());
}

#[test]
fn config_defaults_context_distill_tier_to_gx() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["path"], fx.env.cpd_home().join("config.toml").to_str().unwrap());
    assert_eq!(
        out["config"],
        json!({
            "version": 1,
            "machine": "hs",
            "concurrency": 3,
            "retry_cap": 2,
            "lead_tier": "O2",
            "review_lead_tier": "S3",
            "manager_tier": "O4",
            "context_distill_tier": "GX",
            "context_curation_min_records": 25,
            "loop_guard_turns": 20,
            "open_tier": "H3",
            "lead_pi_packages": [],
            "pi_adapter": "",
            "worker_runner": "pi",
            "context_engine_enabled": true,
            "context_engine_bridge": "",
            "context_engine_python": "",
            "context_engine_dir": "",
            "context_engine_timeout_ms": 30000,
            "context_engine_reindex_timeout_ms": 600000,
            "timeouts": {
                "attempt_minutes": 30,
                "ask_minutes": 60,
                "setup_minutes": 15,
                "gate_minutes": 30,
                "store_busy_ms": 5000,
                "fork_quiet_seconds": 5,
                "distill_minutes": 15,
                "stall_seconds": 1200,
                "stall_minutes": 15,
                "explore_minutes": 20,
                "pi_stop_seconds": 10,
                "sleep_seconds": 3600,
                "archive_sleep_seconds": 86400
            },
            "watch": {"tick_seconds": 5, "pr_poll_seconds": 5, "team_review_seconds": 1800, "models_discovery_seconds": 604800, "janitor_minutes": 60, "error_repeat_seconds": 600, "pr_poll_transient_minutes": 10},
            "inbox": {
                "stuck_job_minutes": 10,
                "team_channel": "",
                "slack_user_id": "",
                "bot_reviewers": ["mission-lane-claude-code-assistant", "github-actions", "copilot-pull-request-reviewer"],
                "review_context": ""
            },
            "events": {"poll_ms": 200},
            "tmux": {"socket": "cpd", "cockpit_session": "cpd", "work_session": "cpd-work"}
        })
    );
}

/// hs-d679: the test-slot pool is gone, so `test_concurrency` is not a config key: it is refused
/// like every unknown key, with its key path.
#[test]
fn test_concurrency_is_not_a_config_key() {
    let fx = Fx::new();
    let run = config_with(&fx, &format!("{MINIMAL}test_concurrency = 2\n"));
    let err = run.error();
    assert_eq!(run.exit, 1);
    assert_eq!(err["code"], "config_invalid");
    assert_eq!(err["details"]["key"], "test_concurrency");
}

/// hs-d552, hs-d571 (contract §8.2): `loop_guard_turns` is an integer >= 1, default 20.
#[test]
fn config_defaults_loop_guard_turns_to_20_and_takes_a_positive_integer() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["loop_guard_turns"], 20, "{out}");
    assert_eq!(out["sources"]["loop_guard_turns"], "default", "{out}");

    let out = config_with(&fx, &format!("{MINIMAL}loop_guard_turns = 3\n")).ok();
    assert_eq!(out["config"]["loop_guard_turns"], 3, "{out}");
    assert_eq!(out["sources"]["loop_guard_turns"], "file", "{out}");
}

#[test]
fn config_loop_guard_turns_zero_negative_or_not_an_integer_is_config_invalid() {
    let fx = Fx::new();
    // 1 is the floor and is accepted.
    assert_eq!(config_with(&fx, &format!("{MINIMAL}loop_guard_turns = 1\n")).ok()["config"]["loop_guard_turns"], 1);
    for bad in ["0", "-1", "1.5", "\"20\""] {
        let run = config_with(&fx, &format!("{MINIMAL}loop_guard_turns = {bad}\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{bad}");
        assert_eq!(err["code"], "config_invalid", "{bad}: {err}");
        assert_eq!(err["details"]["key"], "loop_guard_turns", "{bad}: {err}");
    }
}

/// Context decision 'Team review Inbox' (contract §8.2): the channel and the User's Slack id are set together.
#[test]
fn config_takes_the_team_review_keys() {
    let fx = Fx::new();
    let shown = config_with(
        &fx,
        &format!("{MINIMAL}[watch]\nteam_review_seconds = 900\n[inbox]\nteam_channel = \"C02JU6C2Z1D\"\nslack_user_id = \"U04DB4MTUMR\"\nbot_reviewers = [\"ci-bot\"]\n"),
    )
    .ok();
    assert_eq!(shown["config"]["watch"]["team_review_seconds"], 900);
    assert_eq!(shown["config"]["inbox"], json!({"stuck_job_minutes": 10, "team_channel": "C02JU6C2Z1D", "slack_user_id": "U04DB4MTUMR", "bot_reviewers": ["ci-bot"], "review_context": ""}));
    assert_eq!(shown["sources"]["inbox.team_channel"], "file");
    assert_eq!(shown["sources"]["watch.team_review_seconds"], "file");
}

/// engine-W9, tests-build-F4: the tick is a decimal (minimum 0.1) and pi's stop grace is a key.
#[test]
fn config_takes_a_fractional_tick_and_pi_stop_seconds() {
    let fx = Fx::new();
    let shown = |text: &str| config_with(&fx, &format!("{MINIMAL}{text}")).ok()["config"].clone();
    let config = shown("[watch]\ntick_seconds = 0.2\n[timeouts]\npi_stop_seconds = 1\n");
    assert_eq!(config["watch"]["tick_seconds"], 0.2);
    assert_eq!(config["timeouts"]["pi_stop_seconds"], 1);
    // 0.1 is the floor; a whole number is shown as one, as before.
    assert_eq!(shown("[watch]\ntick_seconds = 0.1\n")["watch"]["tick_seconds"], 0.1);
    assert_eq!(shown("[watch]\ntick_seconds = 2\n")["watch"]["tick_seconds"], 2);
    assert_eq!(shown("[watch]\ntick_seconds = 2.0\n")["watch"]["tick_seconds"], 2);
}

#[test]
fn config_has_one_source_per_leaf_key() {
    let fx = Fx::new();
    let out = config_with(&fx, "version = 1\nmachine = \"hs\"\nconcurrency = 5\n[timeouts]\nask_minutes = 10\n").ok();

    // The leaves of `config`, as dotted paths.
    fn leaves(prefix: &str, value: &Value, into: &mut Vec<String>) {
        match value {
            Value::Object(map) => map.iter().for_each(|(k, v)| {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                leaves(&key, v, into);
            }),
            _ => into.push(prefix.to_owned()),
        }
    }
    let mut expected = Vec::new();
    leaves("", &out["config"], &mut expected);
    let mut actual: Vec<String> = out["sources"].as_object().unwrap().keys().cloned().collect();
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);

    let sources = &out["sources"];
    for key in ["version", "machine", "concurrency", "timeouts.ask_minutes"] {
        assert_eq!(sources[key], "file", "{key}");
    }
    for key in ["retry_cap", "lead_tier", "review_lead_tier", "manager_tier", "context_distill_tier", "open_tier", "lead_pi_packages", "pi_adapter", "timeouts.attempt_minutes", "events.poll_ms", "tmux.socket"] {
        assert_eq!(sources[key], "default", "{key}");
    }
    assert_eq!(out["config"]["concurrency"], 5);
    assert_eq!(out["config"]["timeouts"]["ask_minutes"], 10);
}

#[test]
fn config_accepts_every_key_with_a_value_at_its_boundary() {
    let fx = Fx::new();
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    let text = format!(
        r#"# comments are allowed
version = 1
machine = "a1234567"
concurrency = 1
context_curation_min_records = 4294967295
loop_guard_turns = 4294967295
retry_cap = 0
lead_tier = "H0"
review_lead_tier = "S3"
manager_tier = "H1"
context_distill_tier = "GX"
open_tier = "S3"
lead_pi_packages = []
pi_adapter = "{adapter}"
context_engine_enabled = false
context_engine_bridge = "/b/bridge.py"
context_engine_python = "/b/venv/bin/python"
context_engine_dir = "/b/engine"
context_engine_timeout_ms = 4294967295
context_engine_reindex_timeout_ms = 1
worker_runner = "durable"
[timeouts]
attempt_minutes = 1
ask_minutes = 1
setup_minutes = 1
gate_minutes = 1
explore_minutes = 1
store_busy_ms = 1
fork_quiet_seconds = 1
distill_minutes = 1
stall_seconds = 0
pi_stop_seconds = 1
sleep_seconds = 1
archive_sleep_seconds = 1
[watch]
tick_seconds = 1
pr_poll_seconds = 1
team_review_seconds = 1
models_discovery_seconds = 1
janitor_minutes = 1
pr_poll_transient_minutes = 1
error_repeat_seconds = 1
[inbox]
stuck_job_minutes = 1
team_channel = "C1"
slack_user_id = "U1"
bot_reviewers = ["a"]
review_context = "eec-codereview"
[events]
poll_ms = 20
[tmux]
socket = "{socket}"
cockpit_session = "A_b-9"
work_session = "w"
"#,
        adapter = adapter.display(),
        socket = "s".repeat(32),
    );
    let defaults = config_with(&fx, MINIMAL).ok();
    let out = config_with(&fx, &text).ok();
    let source_keys = |v: &Value| -> Vec<String> { v["sources"].as_object().unwrap().keys().cloned().collect() };
    // The table names every key of the schema: the defaults list them all, so a new key cannot be missed.
    assert_eq!(source_keys(&out), source_keys(&defaults));
    assert_eq!(out["config"]["context_engine_enabled"], false);
    assert_eq!(out["config"]["context_engine_bridge"], "/b/bridge.py");
    assert_eq!(out["config"]["context_engine_python"], "/b/venv/bin/python");
    assert_eq!(out["config"]["context_engine_dir"], "/b/engine");
    assert_eq!(out["config"]["context_engine_timeout_ms"], 4294967295u32);
    assert_eq!(out["config"]["context_engine_reindex_timeout_ms"], 1);
    assert_eq!(out["config"]["machine"], "a1234567");
    assert_eq!(out["config"]["context_distill_tier"], "GX");
    assert_eq!(out["config"]["open_tier"], "S3");
    assert_eq!(out["config"]["lead_pi_packages"], json!([]));
    assert_eq!(out["config"]["pi_adapter"], adapter.to_str().unwrap());
    assert!(out["sources"].as_object().unwrap().values().all(|s| s == "file"));
    // 0 turns the stall kill off (contract §8.2), unlike every other timeout.
    assert_eq!(out["config"]["timeouts"]["stall_seconds"], 0);

    fx.env.write_config(&text.replace("poll_ms = 20", "poll_ms = 10000"));
    assert_eq!(fx.cpd(&["config"]).ok()["config"]["events"]["poll_ms"], 10000);
}

#[test]
fn config_refuses_each_bad_file_with_its_key_path() {
    let bad: &[(&str, &str)] = &[
        // unknown keys, at the top and in every table
        ("version = 1\nmachine = \"hs\"\nsurprise = 1\n", "surprise"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\nsurprise = 1\n", "timeouts.surprise"),
        ("version = 1\nmachine = \"hs\"\n[watch]\nsurprise = 1\n", "watch.surprise"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nsurprise = 1\n", "inbox.surprise"),
        ("version = 1\nmachine = \"hs\"\n[events]\nsurprise = 1\n", "events.surprise"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\nsurprise = 1\n", "tmux.surprise"),
        ("version = 1\nmachine = \"hs\"\n[surprise]\nx = 1\n", "surprise"),
        // keys removed by R19, by Plan manager B1 (hs-a1: no Context sync), by hs-d243 (no per-repo lock) and from cpd 1
        ("version = 1\nmachine = \"hs\"\n[timeouts]\nlock_wait_seconds = 900\n", "timeouts.lock_wait_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\ncontext_sync_seconds = 300\n", "watch.context_sync_seconds"),
        ("version = 1\nmachine = \"hs\"\nreview_mode = \"none\"\n", "review_mode"),
        ("version = 1\nmachine = \"hs\"\ndefault_review_tier = \"S3\"\n", "default_review_tier"),
        ("version = 1\nmachine = \"hs\"\ntickSeconds = 5\n", "tickSeconds"),
        // required keys
        ("machine = \"hs\"\n", "version"),
        ("version = 1\n", "machine"),
        // wrong types
        ("version = \"1\"\nmachine = \"hs\"\n", "version"),
        ("version = 1\nmachine = 7\n", "machine"),
        ("version = 1\nmachine = \"hs\"\nconcurrency = \"3\"\n", "concurrency"),
        ("version = 1\nmachine = \"hs\"\nconcurrency = 2.5\n", "concurrency"),
        ("version = 1\nmachine = \"hs\"\ntest_concurrency = \"2\"\n", "test_concurrency"),
        ("version = 1\nmachine = \"hs\"\ntest_concurrency = 1.5\n", "test_concurrency"),
        ("version = 1\nmachine = \"hs\"\nlead_pi_packages = \"npm:x\"\n", "lead_pi_packages"),
        ("version = 1\nmachine = \"hs\"\nlead_pi_packages = [1]\n", "lead_pi_packages"),
        ("version = 1\nmachine = \"hs\"\ntiers = 3\n", "tiers"),
        ("version = 1\nmachine = \"hs\"\ntimeouts = []\n", "timeouts"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\nask_minutes = true\n", "timeouts.ask_minutes"),
        // out of range
        ("version = 2\nmachine = \"hs\"\n", "version"),
        ("version = 1\nmachine = \"hs\"\nconcurrency = 0\n", "concurrency"),
        ("version = 1\nmachine = \"hs\"\nconcurrency = -1\n", "concurrency"),
        ("version = 1\nmachine = \"hs\"\nconcurrency = 99999999999\n", "concurrency"),
        ("version = 1\nmachine = \"hs\"\ntest_concurrency = 0\n", "test_concurrency"),
        ("version = 1\nmachine = \"hs\"\ntest_concurrency = 65\n", "test_concurrency"),
        ("version = 1\nmachine = \"hs\"\ntest_concurrency = -1\n", "test_concurrency"),
        ("version = 1\nmachine = \"hs\"\nretry_cap = -1\n", "retry_cap"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\nattempt_minutes = 0\n", "timeouts.attempt_minutes"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\nstore_busy_ms = 0\n", "timeouts.store_busy_ms"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\nstall_seconds = -1\n", "timeouts.stall_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\ntick_seconds = 0\n", "watch.tick_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\ntick_seconds = 0.05\n", "watch.tick_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\ntick_seconds = 0.2501\n", "watch.tick_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\ntick_seconds = \"0.2\"\n", "watch.tick_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\npr_poll_seconds = 0.5\n", "watch.pr_poll_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\npr_poll_transient_minutes = 0\n", "watch.pr_poll_transient_minutes"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\npi_stop_seconds = 0\n", "timeouts.pi_stop_seconds"),
        ("version = 1\nmachine = \"hs\"\n[timeouts]\npi_stop_seconds = 0.5\n", "timeouts.pi_stop_seconds"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nstuck_job_minutes = 0\n", "inbox.stuck_job_minutes"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nstuck_job_minutes = \"10\"\n", "inbox.stuck_job_minutes"),
        ("version = 1\nmachine = \"hs\"\n[watch]\nteam_review_seconds = 0\n", "watch.team_review_seconds"),
        ("version = 1\nmachine = \"hs\"\n[watch]\nmodels_discovery_seconds = 0\n", "watch.models_discovery_seconds"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nteam_channel = \"c02ju\"\nslack_user_id = \"U1\"\n", "inbox.team_channel"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nteam_channel = \"C02JU6C2Z1D\"\nslack_user_id = \"clayton\"\n", "inbox.slack_user_id"),
        // both or neither: the claim and mention rules need the User's id, and the id is useless without the channel
        ("version = 1\nmachine = \"hs\"\n[inbox]\nteam_channel = \"C02JU6C2Z1D\"\n", "inbox.slack_user_id"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nslack_user_id = \"U04DB4MTUMR\"\n", "inbox.team_channel"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nbot_reviewers = [\"\"]\n", "inbox.bot_reviewers"),
        ("version = 1\nmachine = \"hs\"\n[inbox]\nbot_reviewers = \"github-actions\"\n", "inbox.bot_reviewers"),
        ("version = 1\nmachine = \"hs\"\n[events]\npoll_ms = 19\n", "events.poll_ms"),
        ("version = 1\nmachine = \"hs\"\n[events]\npoll_ms = 10001\n", "events.poll_ms"),
        // text constraints
        ("version = 1\nmachine = \"\"\n", "machine"),
        ("version = 1\nmachine = \"Hs\"\n", "machine"),
        ("version = 1\nmachine = \"1hs\"\n", "machine"),
        ("version = 1\nmachine = \"h-s\"\n", "machine"),
        ("version = 1\nmachine = \"abcdefghi\"\n", "machine"),
        ("version = 1\nmachine = \"hs\"\nlead_tier = \"X3\"\n", "lead_tier"),
        ("version = 1\nmachine = \"hs\"\nlead_tier = \"O6\"\n", "lead_tier"),
        ("version = 1\nmachine = \"hs\"\nlead_tier = \"o3\"\n", "lead_tier"),
        ("version = 1\nmachine = \"hs\"\nmanager_tier = \"X5\"\n", "manager_tier"),
        ("version = 1\nmachine = \"hs\"\nmanager_tier = \"O6\"\n", "manager_tier"),
        ("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"X2\"\n", "context_distill_tier"),
        ("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"S6\"\n", "context_distill_tier"),
        // hs-d506: no effort 5 for a Lead or the Manager; auxiliaries are H only; `open` Jobs are Workers'
        ("version = 1\nmachine = \"hs\"\nlead_tier = \"O5\"\n", "lead_tier"),
        ("version = 1\nmachine = \"hs\"\nlead_tier = \"S5\"\n", "lead_tier"),
        ("version = 1\nmachine = \"hs\"\nmanager_tier = \"O5\"\n", "manager_tier"),
        ("version = 1\nmachine = \"hs\"\nmanager_tier = \"H5\"\n", "manager_tier"),
        ("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"S2\"\n", "context_distill_tier"),
        ("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"O2\"\n", "context_distill_tier"),
        ("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"H5\"\n", "context_distill_tier"),
        ("version = 1\nmachine = \"hs\"\nopen_tier = \"O2\"\n", "open_tier"),
        ("version = 1\nmachine = \"hs\"\nopen_tier = \"S5\"\n", "open_tier"),
        ("version = 1\nmachine = \"hs\"\nopen_tier = \"X2\"\n", "open_tier"),
        ("version = 1\nmachine = \"hs\"\nopen_tier = \"S6\"\n", "open_tier"),
        ("version = 1\nmachine = \"hs\"\nopen_tier = 2\n", "open_tier"),
        ("version = 1\nmachine = \"hs\"\nlead_pi_packages = [\"\"]\n", "lead_pi_packages"),
        ("version = 1\nmachine = \"hs\"\npi_adapter = \"/no/such/adapter\"\n", "pi_adapter"),
        // removed by hs-d340 (contract §8.2): E4 opens the PR through its open Job and no longer escalates, so a config still setting it is unknown
        ("version = 1\nmachine = \"hs\"\npr_open_escalate_minutes = 10\n", "pr_open_escalate_minutes"),
        ("version = 1\nmachine = \"hs\"\npr_open_escalate_minutes = 0\n", "pr_open_escalate_minutes"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\nsocket = \"\"\n", "tmux.socket"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\nsocket = \"a b\"\n", "tmux.socket"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\ncockpit_session = \"a.b\"\n", "tmux.cockpit_session"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\nwork_session = \"a:b\"\n", "tmux.work_session"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\nsocket = \"sssssssssssssssssssssssssssssssss\"\n", "tmux.socket"),
        // the two sessions differ
        ("version = 1\nmachine = \"hs\"\n[tmux]\ncockpit_session = \"x\"\nwork_session = \"x\"\n", "tmux.work_session"),
        ("version = 1\nmachine = \"hs\"\n[tmux]\ncockpit_session = \"cpd-work\"\n", "tmux.work_session"),
    ];
    // One env: `cpd config` reads the file and writes nothing, so each case sees only its own file.
    let fx = Fx::new();
    for (text, key) in bad {
        let run = config_with(&fx, text);
        let err = run.error();
        assert_eq!(run.exit, 1, "{text}");
        assert_eq!(err["code"], "config_invalid", "{text}: {err}");
        assert_eq!(err["details"]["key"], *key, "{text}: {err}");
        assert_eq!(err["details"]["path"], fx.env.cpd_home().join("config.toml").to_str().unwrap(), "{text}");
    }
}

/// pro-d82: `open_tier` is a Worker Tier: H0-H3 or S0-S3.
#[test]
fn config_accepts_an_open_tier_in_the_worker_range() {
    let fx = Fx::new();
    for tier in ["H0", "H1", "H2", "H3", "S0", "S1", "S2", "S3"] {
        let ok = config_with(&fx, &format!("version = 1\nmachine = \"hs\"\nopen_tier = \"{tier}\"\n")).ok();
        assert_eq!(ok["config"]["open_tier"], tier);
    }
    // The range ends at H3: H4 is not part of it.
    assert_eq!(config_with(&fx, "version = 1\nmachine = \"hs\"\nopen_tier = \"H4\"\n").error()["code"], "config_invalid");
}

/// pro-d82 (hs-d554 for the cost reason): `open_tier` outside H0-H3, S0-S3 is refused.
#[test]
fn config_refuses_an_open_tier_outside_the_worker_range() {
    let fx = Fx::new();
    for tier in ["H4", "H5", "S4", "S5", "O2"] {
        let run = config_with(&fx, &format!("version = 1\nmachine = \"hs\"\nopen_tier = \"{tier}\"\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{tier}");
        assert_eq!(err["code"], "config_invalid", "{tier}: {err}");
        assert_eq!(err["details"]["key"], "open_tier", "{tier}: {err}");
    }
}

/// hs-d562: an auxiliary run uses only `GX`.
#[test]
fn config_refuses_a_context_distill_tier_that_is_not_gx() {
    let fx = Fx::new();
    for tier in ["H0", "H4", "S1", "O2", "G0", "G5"] {
        let run = config_with(&fx, &format!("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"{tier}\"\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{tier}");
        assert_eq!(err["code"], "config_invalid", "{tier}: {err}");
        assert_eq!(err["details"]["key"], "context_distill_tier", "{tier}: {err}");
        if tier == "H0" {
            let message = err["message"].as_str().unwrap();
            assert!(message.contains("GX") && message.contains("hs-d562"), "{message}");
        }
    }
    let ok = config_with(&fx, "version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"GX\"\n").ok();
    assert_eq!(ok["config"]["context_distill_tier"], "GX");
}

/// hs-d562: a Lead or the Manager is never on G; `open_tier` is a Worker's Tier (hs-d554), and GX is not one.
#[test]
fn config_refuses_g_for_lead_manager_and_open_tier() {
    let fx = Fx::new();
    for key in ["lead_tier", "manager_tier", "open_tier"] {
        let run = config_with(&fx, &format!("version = 1\nmachine = \"hs\"\n{key} = \"GX\"\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{key}");
        assert_eq!(err["code"], "config_invalid", "{key}: {err}");
        assert_eq!(err["details"]["key"], key, "{key}: {err}");
        if key != "open_tier" {
            let message = err["message"].as_str().unwrap();
            assert!(message.contains('G') && message.contains("hs-d562"), "{key}: {message}");
        }
    }
}

#[test]
fn config_messages_end_in_one_full_stop_and_name_the_bound_that_was_crossed() {
    let fx = Fx::new();
    let message = |text: &str| {
        let run = config_with(&fx, text);
        assert_eq!(run.exit, 1, "{text}");
        run.error()["message"].as_str().unwrap().to_owned()
    };
    // A reason that already ends in a full stop gets no second one.
    for text in ["version = 1\nmachine = \"homeserver\"\n", "version = 1\nmachine = \"hs\"\nlead_tier = \"O9\"\n"] {
        let said = message(text);
        assert!(said.ends_with('.') && !said.ends_with(".."), "{said}");
    }
    // Above the largest value, the message says "at most"; below the smallest, "at least".
    let above = message("version = 1\nmachine = \"hs\"\n[watch]\npr_poll_seconds = 4294967296\n");
    assert!(above.contains("must be at most 4294967295, got 4294967296"), "{above}");
    let below = message("version = 1\nmachine = \"hs\"\n[watch]\npr_poll_seconds = 0\n");
    assert!(below.contains("must be at least 1, got 0"), "{below}");
    let tick = message("version = 1\nmachine = \"hs\"\n[watch]\ntick_seconds = 0\n");
    assert!(tick.contains("must be at least 0.1, got 0"), "{tick}");
}

#[test]
fn config_refuses_a_file_that_is_not_toml() {
    let fx = Fx::new();
    let run = config_with(&fx, "version = = 1\n");
    let err = run.error();
    assert_eq!(run.exit, 1);
    assert_eq!(err["code"], "config_invalid");
    assert_eq!(err["details"]["path"], fx.env.cpd_home().join("config.toml").to_str().unwrap());
}

#[test]
fn config_refuses_a_config_toml_it_cannot_read() {
    let fx = Fx::new();
    fs::create_dir(fx.env.cpd_home().join("config.toml")).unwrap();
    let run = fx.cpd(&["config"]);
    assert_eq!(run.error()["code"], "config_invalid");
    assert_eq!(run.exit, 1);
}

#[test]
fn config_leaves_cpd_1s_config_json_alone() {
    let fx = Fx::new();
    fs::write(fx.env.cpd_home().join("config.json"), "{\"machine\": \"cpd1\"}").unwrap();
    assert_eq!(fx.cpd(&["config"]).error()["code"], "config_missing");
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["machine"], "hs");
    assert_eq!(fs::read_to_string(fx.env.cpd_home().join("config.json")).unwrap(), "{\"machine\": \"cpd1\"}");
}

#[test]
fn config_is_read_only_and_idempotent() {
    let fx = Fx::with(TestEnv::bare());
    fx.env.write_config(MINIMAL);
    let first = fx.cpd(&["config"]);
    let second = fx.cpd(&["config"]);
    assert_eq!(first.stdout, second.stdout);
    first.ok();
    // `policy/` is the test env's own owner file (`TestEnv::new`), not something `cpd config` wrote.
    let names: Vec<_> =
        fs::read_dir(fx.env.cpd_home()).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n != "policy").collect();
    assert_eq!(names, ["config.toml"]);
    assert_eq!(fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap(), MINIMAL);
}

#[test]
fn config_with_a_bad_argument_is_usage() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);
    for args in [&["config", "--surprise"][..], &["config", "extra"], &["tiers", "--surprise"], &["tiers", "extra"]] {
        let run = fx.cpd(args);
        assert_eq!(run.error()["code"], "usage", "{args:?}");
        assert_eq!(run.exit, 2);
    }
}

#[test]
fn an_empty_cpd_home_variable_is_usage() {
    let fx = Fx::new();
    let run = fx.cpd_with(fx.env.root(), &["config"], &[("CPD_HOME", "")]);
    assert_eq!(run.exit, 2);
}

// ----------------------------------------------------------------- tiers

/// The directory holding the real `node`, found on the test process's own `PATH`.
fn real_node() -> std::path::PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("node"))
        .find(|p| p.is_file())
        .expect("node is on PATH: the Gate needs it for the TypeScript checks")
}

/// A fake pi install with its agent directory at the test root: `bin/pi` (on
/// `PATH`; an empty executable file, which `cpd` only locates and never runs), `install/current-version` and a
/// release whose two entry points are the given sources. Returns the release's entry points.
fn install_pi(env: &TestEnv, version: &str, coding_agent_js: &str, pi_ai_js: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    write_executable(&env.bin().join("pi"), "");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install").join("current-version"), format!("{version}\n")).unwrap();
    let scope = env.root().join("install/releases").join(version).join("node_modules/@earendil-works");
    let mut entries = Vec::new();
    for (package, source) in [("pi-coding-agent", coding_agent_js), ("pi-ai", pi_ai_js)] {
        let dist = scope.join(package).join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(scope.join(package).join("package.json"), "{\"type\": \"module\"}").unwrap();
        fs::write(dist.join("index.js"), source).unwrap();
        entries.push(dist.join("index.js"));
    }
    (entries.remove(0), entries.remove(0))
}

/// Fake pi modules shaped like pi's: models `p/m-*` exist; `p/m-low` supports
/// off, minimal, low, medium and every other model all six levels plus minimal.
const FAKE_CODING_AGENT: &str = r#"
export const ModelRuntime = {
  async create(options) {
    if (options.allowModelNetwork !== false) throw new Error("network must be off");
    return { getModel: (provider, id) => (provider === "p" && id.startsWith("m-") ? { provider, id } : undefined) };
  },
};
"#;
const FAKE_PI_AI: &str = r#"
export function getSupportedThinkingLevels(model) {
  return model.id === "m-low" ? ["off", "minimal", "low", "medium"] : ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
}
"#;

/// The machine policy's `[model]` keys of the tests of `cpd tiers`.
const TIERS_MODELS: &str = "O = \"p/m-opus\"\nS = \"p/m-sonnet\"\nH = \"p/m-low\"\nG = \"p/m-g\"\n";

/// [`cpd_isolated`] in this process: `PATH` is the env's `bin/` alone for the one call.
fn fx_isolated(fx: &Fx, args: &[&str]) -> Run {
    fx.cpd_with(fx.env.root(), args, &[("PATH", fx.env.bin().to_str().expect("a UTF-8 path"))])
}

/// `cpd <args>` with `PATH` = the test's `bin/` only, so nothing real is found.
fn cpd_isolated(env: &TestEnv, args: &[&str]) -> Run {
    let out = env.command(CPD).args(args).env("PATH", env.bin()).output().expect("run cpd");
    Run {
        exit: out.status.code().expect("exit code"),
        stdout: String::from_utf8(out.stdout).expect("UTF-8"),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

const ALL_LEVELS_JSON: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","minimal","low"],"H":["off","low","medium","high"],"G":["off","minimal","low","medium","high"]}"#;

/// The levels pi's fake modules ([`FAKE_PI_AI`]) report for the models of [`TIERS_MODELS`]: `p/m-low` has off, minimal,
/// low and medium, the others every level.
const FAKE_MODULES_LEVELS_JSON: &str = r#"{"O":["off","minimal","low","medium","high","xhigh","max"],"S":["off","minimal","low","medium","high","xhigh","max"],"H":["off","minimal","low","medium"],"G":["off","minimal","low","medium","high","xhigh","max"]}"#;

/// The catalog module's answer, as `node` prints it.
fn levels(json: &str) -> Reply {
    Reply::ok(&format!("{json}\n"))
}

/// The minimal `config.toml`, and `models` (the keys of `[model]`) as the machine policy's `[model]` table: `cpd tiers` reads the policy.
fn write_tiers_config(env: &TestEnv, models: &str) {
    env.write_config(MINIMAL);
    let path = env.cpd_home().join("policy/host.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("[model]\n{models}")).unwrap();
}

/// An `Fx` on the fake ports with `config` written and pi installed (version `1.2.0`, no module sources: `node` is scripted).
fn tiers_fx(models: &str) -> Fx {
    let fx = Fx::with_ports();
    write_tiers_config(&fx.env, models);
    install_pi(&fx.env, "1.2.0", "", "");
    fx
}

#[test]
fn tiers_reads_efforts_from_pi_with_pis_node() {
    let fx = Fx::with_ports();
    write_tiers_config(&fx.env, TIERS_MODELS);
    install_pi(&fx.env, "9.9.9", FAKE_CODING_AGENT, FAKE_PI_AI);
    // What pi's modules report (`FAKE_PI_AI`), as the scripted `node` prints it; the real `node` is the smoke below.
    fx.fake_node(levels(FAKE_MODULES_LEVELS_JSON));
    let out = fx.cpd(&["tiers"]).ok();
    assert_eq!(
        out,
        json!({
            "ok": true,
            "pi_version": "9.9.9",
            "tiers": {
                "O": {"model": "p/m-opus", "efforts": [0, 1, 2, 3, 4, 5]},
                "S": {"model": "p/m-sonnet", "efforts": [0, 1, 2, 3, 4, 5]},
                "H": {"model": "p/m-low", "efforts": [0, 1, 2]},
                "G": {"model": "p/m-g", "efforts": []}
            }
        })
    );
}

#[test]
fn tiers_runs_the_catalog_module_from_argv() {
    let fx = Fx::with_ports();
    write_tiers_config(&fx.env, TIERS_MODELS);
    let (coding_agent, pi_ai) = install_pi(&fx.env, "1.2.0", "", "");
    fx.fake_node(levels(ALL_LEVELS_JSON));
    let out = fx.cpd(&["tiers"]).ok();
    assert_eq!(out["pi_version"], "1.2.0");
    // `minimal` is dropped; the levels map to efforts by position.
    assert_eq!(out["tiers"]["O"]["efforts"], json!([0, 1, 2, 3, 4, 5]));
    assert_eq!(out["tiers"]["S"]["efforts"], json!([0, 1]));
    assert_eq!(out["tiers"]["H"]["efforts"], json!([0, 1, 2, 3]));
    assert_eq!(out["tiers"]["G"], json!({"model": "p/m-g", "efforts": []}));

    // The product reports canonical paths: /var is a symlink into /private on a Mac.
    let calls = fx.calls("node");
    assert_eq!(calls.len(), 1);
    let argv = &calls[0].args;
    assert_eq!(argv[0], "--input-type=module");
    assert_eq!(argv[1], "-e");
    assert!(argv[2].contains("ModelRuntime.create({ allowModelNetwork: false })"), "the embedded module");
    assert_eq!(
        argv[3..],
        [
            "--".to_owned(),
            coding_agent.canonicalize().unwrap().to_str().unwrap().to_owned(),
            pi_ai.canonicalize().unwrap().to_str().unwrap().to_owned(),
            "O=p/m-opus".to_owned(),
            "S=p/m-sonnet".to_owned(),
            "H=p/m-low".to_owned(),
            "G=p/m-g".to_owned(),
        ]
    );
}

/// The config of the tests that print G's default model: no `G`.
const TIERS_MODELS_DEFAULT_G: &str = "O = \"github-copilot/m-opus\"\nS = \"github-copilot/m-sonnet\"\nH = \"github-copilot/m-low\"\n";

#[test]
fn tiers_prints_g_without_an_effort() {
    let fx = tiers_fx(TIERS_MODELS_DEFAULT_G);
    fx.fake_node(levels(ALL_LEVELS_JSON));
    let out = fx.cpd(&["tiers"]).ok();
    assert_eq!(out["tiers"]["G"], json!({"model": "github-copilot/gemini-3.8-flash", "efforts": []}));
    assert_eq!(fx.calls("node")[0].args.last().unwrap(), "G=github-copilot/gemini-3.8-flash");
    // An `Fx` holds the process-wide lock: the first goes before the second.
    drop(fx);

    let fx = tiers_fx(&format!("{TIERS_MODELS_DEFAULT_G}G = \"github-copilot/gemini\"\n"));
    fx.fake_node(levels(ALL_LEVELS_JSON));
    assert_eq!(fx.cpd(&["tiers"]).ok()["tiers"]["G"], json!({"model": "github-copilot/gemini", "efforts": []}));
}

#[test]
fn a_g_model_pi_does_not_know_is_tier_model_unknown() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_node(levels(r#"{"O":["off"],"S":["off"],"H":["off"],"G":null}"#));
    let run = fx.cpd(&["tiers"]);
    let err = run.error();
    assert_eq!(err["code"], "tier_model_unknown");
    assert_eq!(run.exit, 1);
    assert_eq!(err["details"]["missing"], json!([{"letter": "G", "model": "p/m-g"}]));
}

#[test]
fn a_cache_written_before_g_still_serves_the_catalog() {
    let fx = tiers_fx(TIERS_MODELS_DEFAULT_G);
    // Only the cold call has a reply: a second call of node would be unscripted and fail the test.
    fx.fake_once("node", levels(ALL_LEVELS_JSON));
    fx.cpd(&["tiers"]).ok();
    assert_eq!(
        cached_rows(&fx.env),
        [
            row("1.2.0", "H", "github-copilot/m-low", "[0,1,2,3]"),
            row("1.2.0", "O", "github-copilot/m-opus", "[0,1,2,3,4,5]"),
            row("1.2.0", "S", "github-copilot/m-sonnet", "[0,1]"),
        ],
        "G is never cached"
    );
    let out = fx.cpd(&["tiers"]).ok();
    assert_eq!(out["tiers"]["G"], json!({"model": "github-copilot/gemini-3.8-flash", "efforts": []}));
    assert_eq!(out["tiers"]["S"]["efforts"], json!([0, 1]));
    assert_eq!(fx.calls("node").len(), 1, "only the cold call ran node");
}

/// hs-n278: `cpd tiers` opens the store (`catalog::load`): it waits out a migration in progress for longer than
/// `store_busy_ms`, says so once on stderr, and prints its one JSON value at the end. The migration is held until
/// 1500 ms of the `ManualClock`'s time have passed, which the waiter's own retries pass.
#[test]
fn tiers_waits_out_a_migration_longer_than_store_busy_ms() {
    let mut fx = Fx::with_ports();
    write_tiers_config(&fx.env, TIERS_MODELS);
    fx.env.write_config(&format!("{MINIMAL}[timeouts]\nstore_busy_ms = 300\n"));
    install_pi(&fx.env, "1.2.0", "", "");
    fx.fake_node(levels(ALL_LEVELS_JSON));
    fx.hold_store_migration(Duration::from_millis(1500));
    let run = fx.cpd_capturing_stderr(&["tiers"]);
    let waited = fx.fakes.clock.elapsed();
    assert_eq!(run.ok()["pi_version"], "1.2.0");
    assert!(waited >= Duration::from_millis(1000), "did not wait: {waited:?}");
    assert_eq!(run.stderr.matches("waiting for cpd pid").count(), 1, "{run:?}");
}

/// `cpd config` reads only the config files: a migration in progress does not delay it, and it says nothing.
#[test]
fn config_does_not_wait_for_a_migration() {
    let mut fx = Fx::with_ports();
    fx.env.write_config(MINIMAL);
    fx.hold_store_migration(Duration::from_millis(3000));
    let run = fx.cpd_capturing_stderr(&["config"]);
    run.ok();
    // `cpd config` does not wait for a 3 s migration: it never naps, so the virtual time has not moved.
    let waited = fx.fakes.clock.elapsed();
    assert!(waited < Duration::from_millis(2500), "waited {waited:?}");
    assert_eq!(fx.fakes.clock.naps(), Vec::<Duration>::new());
    assert_eq!(run.stderr, "");
}

#[test]
fn tiers_prefers_pis_own_node_over_node_on_path() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_once("node", levels(ALL_LEVELS_JSON));

    // ~/.local/share/pi-node/current/bin/node, as pi's launcher looks for it (the runner is scripted: the file is
    // only found, never run).
    let pi_node = fx.env.home().join(".local/share/pi-node/current/bin/node");
    fs::create_dir_all(pi_node.parent().unwrap()).unwrap();
    write_executable(&pi_node, "");
    fx.cpd(&["tiers"]).ok();
    let calls = fx.calls("node");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, pi_node.to_str().unwrap(), "pi's node ran, not `node` on PATH");
    // The catalog is cached now; forget it so the next call reads pi again.
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(fx.env.cpd_home().join(format!("cpd.db{suffix}")));
    }

    // XDG_DATA_HOME moves it.
    let xdg = fx.env.root().join("xdg");
    let xdg_node = xdg.join("pi-node/current/bin/node");
    fs::create_dir_all(xdg_node.parent().unwrap()).unwrap();
    write_executable(&xdg_node, "");
    fx.fake_once("node", Reply::exit(1, "", "xdg node ran\n"));
    let run = fx.cpd_with(fx.env.root(), &["tiers"], &[("XDG_DATA_HOME", xdg.to_str().unwrap())]);
    let err = run.error();
    assert_eq!(err["code"], "catalog_failed");
    assert!(err["details"]["stderr"].as_str().unwrap().contains("xdg node ran"));
    assert_eq!(fx.calls("node")[1].program, xdg_node.to_str().unwrap(), "the node under XDG_DATA_HOME ran");
}

#[test]
fn tiers_without_pi_on_path_is_pi_not_found() {
    let fx = Fx::new();
    write_tiers_config(&fx.env, TIERS_MODELS);
    fx.env.fake("node", &format!("echo '{ALL_LEVELS_JSON}'"));
    let run = fx_isolated(&fx, &["tiers"]);
    assert_eq!(run.error()["code"], "pi_not_found");
    assert_eq!(run.exit, 1);
    assert!(fx.env.calls("node").is_empty());
}

#[test]
fn tiers_with_a_broken_pi_install_is_pi_not_found() {
    // No current-version file.
    let fx = Fx::new();
    write_tiers_config(&fx.env, TIERS_MODELS);
    fx.env.fake_pi("exit 0");
    let run = fx_isolated(&fx, &["tiers"]);
    assert_eq!(run.error()["code"], "pi_not_found");
    assert_eq!(run.exit, 1);

    // A version that is not a plain name.
    install_pi(&fx.env, "1.2.0", "", "");
    for bad in ["", "..", "../x", "1 2"] {
        fs::write(fx.env.root().join("install/current-version"), format!("{bad}\n")).unwrap();
        let run = fx_isolated(&fx, &["tiers"]);
        assert_eq!(run.error()["code"], "pi_not_found", "{bad:?}");
    }

    // A release without its entry points.
    install_pi(&fx.env, "1.2.0", "", "");
    fs::remove_file(fx.env.root().join("install/releases/1.2.0/node_modules/@earendil-works/pi-ai/dist/index.js")).unwrap();
    let run = fx_isolated(&fx, &["tiers"]);
    let err = run.error();
    assert_eq!(err["code"], "pi_not_found");
    assert!(err["details"]["missing"].as_str().unwrap().ends_with("pi-ai/dist/index.js"));
}

#[test]
fn tiers_finds_pi_through_a_symlink() {
    let fx = Fx::with_ports();
    write_tiers_config(&fx.env, TIERS_MODELS);
    let env = &fx.env;
    // The agent directory is `real/agent`; `bin/pi` is a symlink into it.
    let agent = env.root().join("real/agent");
    fs::create_dir_all(agent.join("bin")).unwrap();
    write_executable(&agent.join("bin/pi"), "");
    fs::remove_file(env.bin().join("pi")).unwrap(); // the deny shim
    symlink(agent.join("bin/pi"), env.bin().join("pi")).unwrap();
    let release = agent.join("install/releases/3.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(release.join(package).join("dist")).unwrap();
        fs::write(release.join(package).join("dist/index.js"), "").unwrap();
    }
    fs::write(agent.join("install/current-version"), "3.0.0\n").unwrap();
    fx.fake_node(levels(ALL_LEVELS_JSON));
    assert_eq!(fx.cpd(&["tiers"]).ok()["pi_version"], "3.0.0");
}

#[test]
fn tiers_catalog_failures_are_catalog_failed() {
    let cases: Vec<(&str, Reply)> = vec![
        ("node exits non-zero", Reply::exit(3, "", "boom: cannot import\n")),
        ("node prints nothing", Reply::ok("")),
        ("node prints text", Reply::ok("not json\n")),
        ("node prints a JSON array", levels("[]")),
        ("node omits a Tier", levels(r#"{"O":["off"],"S":["off"],"G":["off"]}"#)),
        ("a level pi does not have", levels(r#"{"O":["off"],"S":["off"],"H":["turbo"],"G":["off"]}"#)),
        ("a model with no usable level", levels(r#"{"O":["off"],"S":["minimal"],"H":["off"],"G":["off"]}"#)),
        ("levels that are not text", levels(r#"{"O":[1],"S":["off"],"H":["off"],"G":["off"]}"#)),
    ];
    // One env: a failure caches nothing, so each case runs node again.
    let fx = tiers_fx(TIERS_MODELS);
    for (n, (name, reply)) in cases.into_iter().enumerate() {
        fx.fake_once("node", reply);
        let run = fx.cpd(&["tiers"]);
        assert_eq!(run.error()["code"], "catalog_failed", "{name}");
        assert_eq!(run.exit, 1, "{name}");
        assert_eq!(fx.calls("node").len(), n + 1, "{name}: node ran");
    }

    // The failing node's exit and stderr are in `details`.
    fx.fake_once("node", Reply::exit(3, "", "boom: cannot import\n"));
    let err = fx.cpd(&["tiers"]).error();
    assert_eq!(err["details"]["exit"], 3);
    assert!(err["details"]["stderr"].as_str().unwrap().contains("boom: cannot import"));
}

#[test]
fn tiers_without_any_node_is_catalog_failed() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_node(Reply::io(ErrorKind::NotFound));
    let run = fx.cpd(&["tiers"]);
    assert_eq!(run.error()["code"], "catalog_failed");
    assert_eq!(run.exit, 1);
}

#[test]
fn tiers_with_a_model_pi_does_not_have_is_tier_model_unknown() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_node(levels(r#"{"O":["off"],"S":null,"H":null,"G":["off"]}"#));
    let run = fx.cpd(&["tiers"]);
    let err = run.error();
    assert_eq!(err["code"], "tier_model_unknown");
    assert_eq!(run.exit, 1);
    assert_eq!(err["details"]["missing"], json!([{"letter": "S", "model": "p/m-sonnet"}, {"letter": "H", "model": "p/m-low"}]));
}

#[test]
fn tiers_with_a_model_unknown_to_real_pi_modules_is_tier_model_unknown() {
    let env = TestEnv::new();
    write_tiers_config(&env, "O = \"p/m-opus\"\nS = \"p/other\"\nH = \"p/none\"\nG = \"p/m-g\"\n");
    install_pi(&env, "1.2.0", FAKE_CODING_AGENT, FAKE_PI_AI);
    symlink(real_node(), env.bin().join("node")).unwrap();
    let err = cpd_isolated(&env, &["tiers"]).error();
    assert_eq!(err["code"], "tier_model_unknown");
    assert_eq!(err["details"]["missing"], json!([{"letter": "S", "model": "p/other"}, {"letter": "H", "model": "p/none"}]));
}

#[test]
fn tiers_checks_pi_before_the_catalog_and_config_before_both() {
    let fx = Fx::new();
    let run = fx_isolated(&fx, &["tiers"]);
    assert_eq!(run.error()["code"], "config_missing");
    fx.env.write_config("version = 1\nmachine = \"hs\"\nconcurrency = 0\n");
    assert_eq!(fx_isolated(&fx, &["tiers"]).error()["code"], "config_invalid");
}

/// The `tier_catalog` rows in `$CPD_HOME/cpd.db`: `(pi_version, letter, model, efforts)`.
fn cached_rows(env: &TestEnv) -> Vec<(String, String, String, String)> {
    let db = rusqlite::Connection::open_with_flags(env.cpd_home().join("cpd.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("cpd.db exists");
    let mut stmt = db.prepare("SELECT pi_version, letter, model, efforts FROM tier_catalog ORDER BY pi_version, letter").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

/// The commands of the `txns` rows, oldest first.
fn txn_commands(env: &TestEnv) -> Vec<String> {
    let db = rusqlite::Connection::open_with_flags(env.cpd_home().join("cpd.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("cpd.db exists");
    let mut stmt = db.prepare("SELECT command FROM txns ORDER BY id").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

fn row(version: &str, letter: &str, model: &str, efforts: &str) -> (String, String, String, String) {
    (version.to_owned(), letter.to_owned(), model.to_owned(), efforts.to_owned())
}

#[test]
fn tiers_with_a_cold_cache_asks_pi_and_fills_the_cache() {
    let fx = Fx::with_ports_empty();
    write_tiers_config(&fx.env, TIERS_MODELS);
    install_pi(&fx.env, "1.2.0", "", "");
    fx.fake_node(levels(ALL_LEVELS_JSON));
    assert!(!fx.env.cpd_home().join("cpd.db").exists());
    fx.cpd(&["tiers"]).ok();
    assert_eq!(fx.calls("node").len(), 1);
    assert_eq!(
        cached_rows(&fx.env),
        [
            row("1.2.0", "H", "p/m-low", "[0,1,2,3]"),
            row("1.2.0", "O", "p/m-opus", "[0,1,2,3,4,5]"),
            row("1.2.0", "S", "p/m-sonnet", "[0,1]"),
        ]
    );
    // One transaction, made through the store's write API, which records it.
    assert_eq!(txn_commands(&fx.env), ["migrate", "tiers"]);
}

#[test]
fn tiers_with_a_warm_cache_answers_from_it_without_node_or_a_write() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_once("node", levels(ALL_LEVELS_JSON));
    let cold = fx.cpd(&["tiers"]);
    cold.ok();
    // Nothing runs node now (a second call would be unscripted): the cache alone answers, identically.
    let warm = fx.cpd(&["tiers"]);
    assert_eq!(warm.stdout, cold.stdout);
    assert_eq!(fx.calls("node").len(), 1, "only the cold call ran node");
    assert_eq!(txn_commands(&fx.env), ["migrate", "tiers"], "the warm call wrote nothing");
}

#[test]
fn tiers_refreshes_the_cache_when_pis_current_version_changes() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_once("node", levels(ALL_LEVELS_JSON));
    fx.cpd(&["tiers"]).ok();

    install_pi(&fx.env, "1.3.0", "", "");
    fx.fake_once("node", levels(r#"{"O":["off","low"],"S":["off","low"],"H":["off"],"G":["off"]}"#));
    let out = fx.cpd(&["tiers"]).ok();
    assert_eq!(out["pi_version"], "1.3.0");
    assert_eq!(out["tiers"]["O"]["efforts"], json!([0, 1]));
    assert_eq!(out["tiers"]["H"]["efforts"], json!([0]));
    // The table is append-only (§4.4): the new version's rows join the old ones.
    assert_eq!(
        cached_rows(&fx.env),
        [
            row("1.2.0", "H", "p/m-low", "[0,1,2,3]"),
            row("1.2.0", "O", "p/m-opus", "[0,1,2,3,4,5]"),
            row("1.2.0", "S", "p/m-sonnet", "[0,1]"),
            row("1.3.0", "H", "p/m-low", "[0]"),
            row("1.3.0", "O", "p/m-opus", "[0,1]"),
            row("1.3.0", "S", "p/m-sonnet", "[0,1]"),
        ]
    );
    assert_eq!(txn_commands(&fx.env), ["migrate", "tiers", "tiers"]);

    // And the new version is warm: node has no reply left, a call of it would be unscripted.
    assert_eq!(fx.cpd(&["tiers"]).ok()["pi_version"], "1.3.0");
}

/// pro-d26 (Plan bifrost, PR catfp; commands.md §14.3): the cache is keyed by the fingerprint of `<agentDir>/models.json` too. The
/// agent directory of [`install_pi`] is the test root. `cpd doctor`'s `tiers` check asks pi each time and never reads the cache, so
/// the command that needs the fingerprint is `cpd tiers`, and every Lead and Worker launch through the same `load`.
#[test]
fn tiers_sees_a_models_json_changed_under_the_same_pi_version() {
    let fx = tiers_fx(TIERS_MODELS);
    let models_json = fx.env.root().join("models.json");
    fs::write(&models_json, r#"{"thinkingLevelMap": {"minimal": null}}"#).unwrap();
    fx.fake_once("node", levels(r#"{"O":["off","low"],"S":["off","low","medium","high"],"H":["off"],"G":["off"]}"#));
    let first = fx.cpd(&["tiers"]).ok();
    assert_eq!(first["tiers"]["S"]["efforts"], json!([0, 1, 2, 3]));

    // The same pi version and the same models: only the file pi reads changed (Sonnet gains xhigh and max).
    fs::write(&models_json, r#"{"thinkingLevelMap": {"minimal": null, "xhigh": "xhigh", "max": "max"}}"#).unwrap();
    fx.fake_once("node", levels(r#"{"O":["off","low"],"S":["off","low","medium","high","xhigh","max"],"H":["off"],"G":["off"]}"#));
    let second = fx.cpd(&["tiers"]).ok();
    assert_eq!(second["pi_version"], "1.2.0");
    assert_eq!(second["tiers"]["S"]["efforts"], json!([0, 1, 2, 3, 4, 5]), "the changed models.json is seen");
    assert_eq!(fx.calls("node").len(), 2);
    // Append-only: the first bytes' rows stay beside the second's.
    let rows = cached_rows(&fx.env);
    assert_eq!(rows.len(), 6, "{rows:?}");
    assert!(rows.contains(&row("1.2.0", "S", "p/m-sonnet", "[0,1,2,3]")), "{rows:?}");
    assert!(rows.contains(&row("1.2.0", "S", "p/m-sonnet", "[0,1,2,3,4,5]")), "{rows:?}");

    // Unchanged bytes are a hit: node has no reply left.
    assert_eq!(fx.cpd(&["tiers"]).ok()["tiers"]["S"]["efforts"], json!([0, 1, 2, 3, 4, 5]));
    assert_eq!(fx.calls("node").len(), 2);
}

/// An unreadable `models.json` (a directory in its place) is `catalog_failed` with the path, and nothing is cached.
#[test]
fn tiers_with_an_unreadable_models_json_is_catalog_failed_with_its_path() {
    let fx = tiers_fx(TIERS_MODELS);
    let models_json = fx.env.root().join("models.json");
    fs::create_dir(&models_json).unwrap();
    fx.fake_node(levels(ALL_LEVELS_JSON));
    let run = fx.cpd(&["tiers"]);
    let err = run.error();
    assert_eq!(err["code"], "catalog_failed");
    assert_eq!(run.exit, 1);
    assert!(err["details"].to_string().contains(models_json.canonicalize().unwrap().to_str().unwrap()), "{err}");
    assert!(fx.calls("node").is_empty(), "no fallback: pi was not asked");
    assert_eq!(cached_rows(&fx.env), []);
}

#[test]
fn tiers_refreshes_the_cache_when_a_configured_model_changes() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_node(levels(ALL_LEVELS_JSON));
    fx.cpd(&["tiers"]).ok();

    write_tiers_config(&fx.env, &TIERS_MODELS.replace("p/m-sonnet", "p/m-other"));
    let out = fx.cpd(&["tiers"]).ok();
    assert_eq!(out["tiers"]["S"]["model"], "p/m-other");
    assert_eq!(fx.calls("node").len(), 2);
    assert!(cached_rows(&fx.env).contains(&row("1.2.0", "S", "p/m-other", "[0,1]")));
    // The unchanged models' rows are kept, not duplicated or rewritten.
    assert_eq!(cached_rows(&fx.env).len(), 4);
}

#[test]
fn tiers_does_not_cache_a_failed_catalog() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_node(levels(r#"{"O":["off"],"S":null,"H":["off"],"G":["off"]}"#));
    assert_eq!(fx.cpd(&["tiers"]).error()["code"], "tier_model_unknown");
    assert_eq!(cached_rows(&fx.env), []);
    assert_eq!(txn_commands(&fx.env), ["migrate"]);
}

#[test]
fn tiers_is_idempotent() {
    let fx = tiers_fx(TIERS_MODELS);
    fx.fake_once("node", levels(ALL_LEVELS_JSON));
    let first = fx.cpd(&["tiers"]);
    let second = fx.cpd(&["tiers"]);
    first.ok();
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(fx.calls("node").len(), 1);
}

#[test]
fn config_accepts_o_tiers_below_effort_5_for_leads_and_h_for_auxiliaries() {
    let fx = Fx::new();
    let out = config_with(
        &fx,
        "version = 1\nmachine = \"hs\"\nlead_tier = \"O0\"\nmanager_tier = \"O4\"\ncontext_distill_tier = \"GX\"\nopen_tier = \"H3\"\n",
    )
    .ok();
    let config = &out["config"];
    assert_eq!(
        (&config["lead_tier"], &config["manager_tier"], &config["context_distill_tier"], &config["open_tier"]),
        (&json!("O0"), &json!("O4"), &json!("GX"), &json!("H3"))
    );
}

/// Contract §8.2 (hs-d592): `context_curation_min_records`, default 25, an integer >= 1.
#[test]
fn context_curation_min_records_defaults_to_25_and_refuses_below_1_and_non_integers() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["context_curation_min_records"], 25, "{out}");
    assert_eq!(out["sources"]["context_curation_min_records"], "default", "{out}");

    let out = config_with(&fx, &format!("{MINIMAL}context_curation_min_records = 7\n")).ok();
    assert_eq!(out["config"]["context_curation_min_records"], 7, "{out}");
    assert_eq!(out["sources"]["context_curation_min_records"], "file", "{out}");

    for bad in ["0", "-3", "2.5", "\"ten\"", "true"] {
        let run = config_with(&fx, &format!("{MINIMAL}context_curation_min_records = {bad}\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{bad}");
        assert_eq!(err["code"], "config_invalid", "{bad}: {err}");
        assert_eq!(err["details"]["key"], "context_curation_min_records", "{bad}: {err}");
    }
}


// ---------------------------------------------------------------- config --migrate

/// The stale config of contract §8.1 "Config migrations": H2 for the distiller and the
/// removed `test_concurrency` key.
const STALE_CONFIG: &str = "version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"H2\"  # stale\ntest_concurrency = 2\nconcurrency = 5\n";

/// `STALE_CONFIG` after the two migrations.
const MIGRATED_CONFIG: &str = "version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"GX\"  # stale\nconcurrency = 5\n";

/// commands.md §14.2: `config --migrate` rewrites a stale config to the valid form, reports
/// the path and the applied names, and leaves the file equal to the migrated text.
#[test]
fn config_migrate_rewrites_a_stale_config_and_reports_it() {
    let fx = Fx::new();
    let path = fx.env.cpd_home().join("config.toml");
    fx.env.write_config(STALE_CONFIG);

    let run = fx.cpd(&["config", "--migrate"]);
    let out = run.ok();
    assert_eq!(out["path"], path.to_str().unwrap(), "{out}");
    assert_eq!(out["migrated"], json!(["distill-tier-gx", "drop-test-concurrency"]), "{out}");
    assert_eq!(fs::read_to_string(&path).unwrap(), MIGRATED_CONFIG);
}

/// A second run changes nothing: it reports `migrated: []` and the file's bytes and mtime are
/// unchanged.
#[test]
fn config_migrate_again_writes_nothing() {
    let fx = Fx::new();
    let path = fx.env.cpd_home().join("config.toml");
    fx.env.write_config(STALE_CONFIG);
    let _ = fx.cpd(&["config", "--migrate"]).ok();

    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();

    let run = fx.cpd(&["config", "--migrate"]);
    let out = run.ok();
    assert_eq!(out["migrated"], json!([]), "{out}");
    assert_eq!(fs::read(&path).unwrap(), before, "the file was rewritten");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified, "the file was written again");
}

/// A config that is still invalid after the migrations is refused and the file is untouched:
/// it keeps the stale value and the unknown key.
#[test]
fn config_migrate_refuses_an_invalid_config_and_writes_nothing() {
    let fx = Fx::new();
    let path = fx.env.cpd_home().join("config.toml");
    let stale = format!("{STALE_CONFIG}unknown_key = 1\n");
    fx.env.write_config(&stale);

    let run = fx.cpd(&["config", "--migrate"]);
    let err = run.error();
    assert_eq!(err["code"], "config_invalid", "{err}");
    assert_eq!(err["details"]["key"], "unknown_key", "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), stale, "the file was written");
}

/// A missing file is `config_missing`, with the config path in `details`.
#[test]
fn config_migrate_on_a_missing_file_is_config_missing() {
    let fx = Fx::new();
    let path = fx.env.cpd_home().join("config.toml");

    let run = fx.cpd(&["config", "--migrate"]);
    let err = run.error();
    assert_eq!(err["code"], "config_missing", "{err}");
    assert_eq!(err["details"]["path"], path.to_str().unwrap(), "{err}");
}

/// Contract §8.2: `watch.janitor_minutes` is 60 unless the file says otherwise.
#[test]
fn watch_janitor_minutes_defaults_to_60() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["watch"]["janitor_minutes"], 60);
    assert_eq!(out["sources"]["watch.janitor_minutes"], "default");
}

/// Contract §8.2: `watch.janitor_minutes` is at least 1, else `config_invalid` with `details.key`.
#[test]
fn watch_janitor_minutes_below_1_is_config_invalid() {
    let fx = Fx::new();
    for value in ["0", "-1"] {
        let run = config_with(&fx, &format!("{MINIMAL}[watch]\njanitor_minutes = {value}\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{value}");
        assert_eq!(err["code"], "config_invalid", "{value}: {err}");
        assert_eq!(err["details"]["key"], "watch.janitor_minutes", "{value}: {err}");
    }
}

fn review_tier_file(value: &str) -> String {
    format!("version = 1\nmachine = \"hs\"\nreview_lead_tier = {value}\n")
}

#[test]
fn config_defaults_review_lead_tier_to_s3_and_takes_a_lead_tier() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["review_lead_tier"], "S3");
    assert_eq!(out["sources"]["review_lead_tier"], "default");
    let out = config_with(&fx, &review_tier_file("\"O3\"")).ok();
    assert_eq!(out["config"]["review_lead_tier"], "O3");
    assert_eq!(out["sources"]["review_lead_tier"], "file");
    for tier in ["O0", "O4", "S4"] {
        let out = config_with(&fx, &review_tier_file(&format!("\"{tier}\""))).ok();
        assert_eq!(out["config"]["review_lead_tier"], tier);
    }
}

#[test]
fn config_refuses_a_g_review_lead_tier() {
    let fx = Fx::new();
    let run = config_with(&fx, &review_tier_file("\"GX\""));
    let err = run.error();
    assert_eq!(run.exit, 1);
    assert_eq!(err["code"], "config_invalid", "{err}");
    assert_eq!(err["details"]["key"], "review_lead_tier", "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("must not be a G Tier") && message.contains("hs-d562"), "{message}");
}

#[test]
fn config_refuses_an_effort_5_review_lead_tier() {
    let fx = Fx::new();
    for tier in ["O5", "S5"] {
        let run = config_with(&fx, &review_tier_file(&format!("\"{tier}\"")));
        let err = run.error();
        assert_eq!(run.exit, 1, "{tier}");
        assert_eq!(err["code"], "config_invalid", "{tier}: {err}");
        assert_eq!(err["details"]["key"], "review_lead_tier", "{tier}: {err}");
        let message = err["message"].as_str().unwrap();
        assert!(message.contains("effort 0-4") && message.contains("hs-d506"), "{tier}: {message}");
    }
}

#[test]
fn config_refuses_an_invalid_review_lead_tier() {
    let fx = Fx::new();
    for value in ["\"X3\"", "\"O6\"", "\"s3\"", "3"] {
        let run = config_with(&fx, &review_tier_file(value));
        let err = run.error();
        assert_eq!(run.exit, 1, "{value}");
        assert_eq!(err["code"], "config_invalid", "{value}: {err}");
        assert_eq!(err["details"]["key"], "review_lead_tier", "{value}: {err}");
        assert!(!err["message"].as_str().unwrap().contains("is not a known key"), "{value}: {err}");
    }
}
