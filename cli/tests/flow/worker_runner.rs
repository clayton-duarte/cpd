//! The `worker_runner` flag and the Attempt's `runner` and `worker_pid` (contract §3.4 "The Worker runner and its seam", §4.4, §8.2;
//! commands.md §6.3, §14.2), through the command tree in this process: `cpd config`, `job dispatch`, `attempt show` and `job show`.

use crate::common;

use std::fs;

use common::Run;
use common::fx::Fx;
use serde_json::{Value, json};

use super::driver::{JOB, PLAN, seed_home};
use super::driver_open::{Live, live};

const MINIMAL: &str = "version = 1\nmachine = \"hs\"\n";

fn config_with(fx: &Fx, text: &str) -> Run {
    fx.env.write_config(text);
    fx.cpd(&["config"])
}

#[test]
fn config_defaults_the_worker_runner_to_pi() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["worker_runner"], "pi", "{out}");
    assert_eq!(out["sources"]["worker_runner"], "default", "{out}");
}

#[test]
fn config_accepts_the_durable_worker_runner() {
    let fx = Fx::new();
    for runner in ["pi", "durable"] {
        let out = config_with(&fx, &format!("{MINIMAL}worker_runner = \"{runner}\"\n")).ok();
        assert_eq!(out["config"]["worker_runner"], runner, "{out}");
        assert_eq!(out["sources"]["worker_runner"], "file", "{out}");
    }
}

#[test]
fn config_refuses_a_worker_runner_that_is_neither_pi_nor_durable() {
    let fx = Fx::new();
    // An unknown word, a different case, an empty string, and values that are not strings.
    for bad in ["\"tmux\"", "\"Durable\"", "\"\"", "1", "true", "[\"pi\"]"] {
        let run = config_with(&fx, &format!("{MINIMAL}worker_runner = {bad}\n"));
        let err = run.error();
        assert_eq!(run.exit, 1, "{bad}");
        assert_eq!(err["code"], "config_invalid", "{bad}: {err}");
        assert_eq!(err["details"]["key"], "worker_runner", "{bad}: {err}");
        assert!(
            !err["message"]
                .as_str()
                .unwrap()
                .contains("is not a known key"),
            "{bad}: {err}"
        );
    }
}

fn runner_of(live: &Live, id: &str) -> Value {
    live.attempt(id)["runner"].clone()
}

#[test]
fn a_dispatched_attempt_records_the_runner_the_config_names() {
    for (extra, runner) in [
        ("", "pi"),
        ("worker_runner = \"pi\"", "pi"),
        ("worker_runner = \"durable\"", "durable"),
    ] {
        let live = live(extra);
        let out = live.dispatch(JOB).ok();
        let id = out["attempt"]
            .as_str()
            .unwrap_or_else(|| panic!("no attempt in {out}"))
            .to_owned();
        assert_eq!(runner_of(&live, &id), runner, "{extra:?}: {out}");
        let stored: String = live.value(&format!("SELECT runner FROM attempts WHERE id = '{id}'"));
        assert_eq!(stored, runner, "{extra:?}");
        // Drop the `Fx` (its process-wide lock) before the next one.
        drop(live);
    }
}

#[test]
fn an_attempt_keeps_its_runner_when_the_config_changes_after_it_started() {
    let live = live("worker_runner = \"pi\"");
    let id = live.dispatch(JOB).ok()["attempt"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(runner_of(&live, &id), "pi");

    // The flag moves after the Attempt exists: the Attempt does not.
    seed_home(&live.fx.env, "worker_runner = \"durable\"");
    assert_eq!(
        live.fx.cpd(&["config"]).ok()["config"]["worker_runner"],
        "durable"
    );
    assert_eq!(
        runner_of(&live, &id),
        "pi",
        "a config change never moves a started Attempt"
    );
    let job = live.fx.cpd(&["job", "show", JOB, "--plan", PLAN]).ok();
    assert_eq!(job["attempts"][0]["runner"], "pi", "{job}");
    // And the file on disk is the one the test wrote.
    assert!(
        fs::read_to_string(live.fx.env.cpd_home().join("config.toml"))
            .unwrap()
            .contains("durable")
    );
}

#[test]
fn attempt_show_and_job_show_carry_runner_and_worker_pid() {
    let live = live("worker_runner = \"durable\"");
    let id = live.dispatch(JOB).ok()["attempt"]
        .as_str()
        .unwrap()
        .to_owned();

    // `worker_pid` is null before a durable Worker starts; a stored pid is shown as the number.
    let shown = live.attempt(&id);
    assert_eq!(
        (&shown["runner"], &shown["worker_pid"]),
        (&json!("durable"), &Value::Null),
        "{shown}"
    );
    assert!(
        shown.as_object().unwrap().contains_key("worker_pid"),
        "the field is present, null: {shown}"
    );
    live.fixture(&format!(
        "UPDATE attempts SET worker_pid = 4242 WHERE id = '{id}';"
    ));

    let shown = live.attempt(&id);
    assert_eq!(
        (&shown["runner"], &shown["worker_pid"]),
        (&json!("durable"), &json!(4242)),
        "{shown}"
    );
    let job = live.fx.cpd(&["job", "show", JOB, "--plan", PLAN]).ok();
    let attempt = &job["attempts"][0];
    assert_eq!(
        (&attempt["runner"], &attempt["worker_pid"]),
        (&json!("durable"), &json!(4242)),
        "{job}"
    );
}
