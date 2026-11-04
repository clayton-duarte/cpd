//! The `durable_workers` flag and the Attempt's `runner` and `worker_pid` (contract §3.4 "The Worker runner and its seam", §4.4, §8.2;
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
fn config_defaults_durable_workers_to_off() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(
        out["config"]["experimental"]["durable_workers"], false,
        "{out}"
    );
    assert_eq!(
        out["sources"]["experimental.durable_workers"], "default",
        "{out}"
    );
}

#[test]
fn config_accepts_durable_workers() {
    let fx = Fx::new();
    for on in [false, true] {
        let out = config_with(
            &fx,
            &format!("{MINIMAL}[experimental]\ndurable_workers = {on}\n"),
        )
        .ok();
        assert_eq!(
            out["config"]["experimental"]["durable_workers"], on,
            "{out}"
        );
        assert_eq!(
            out["sources"]["experimental.durable_workers"], "file",
            "{out}"
        );
    }
}

#[test]
fn config_refuses_durable_workers_that_is_not_a_boolean_and_the_old_top_level_key() {
    let fx = Fx::new();
    for bad in ["\"durable\"", "\"\"", "1", "[true]"] {
        let run = config_with(
            &fx,
            &format!("{MINIMAL}[experimental]\ndurable_workers = {bad}\n"),
        );
        let err = run.error();
        assert_eq!(run.exit, 1, "{bad}");
        assert_eq!(err["code"], "config_invalid", "{bad}: {err}");
        assert_eq!(
            err["details"]["key"], "experimental.durable_workers",
            "{bad}: {err}"
        );
    }
    let run = config_with(&fx, &format!("{MINIMAL}worker_runner = \"pi\"\n"));
    let err = run.error();
    assert_eq!(err["code"], "config_invalid", "{err}");
    assert_eq!(err["details"]["key"], "worker_runner", "{err}");
}

fn runner_of(live: &Live, id: &str) -> Value {
    live.attempt(id)["runner"].clone()
}

#[test]
fn a_dispatched_attempt_records_the_runner_the_config_names() {
    for (extra, runner) in [
        ("", "pi"),
        ("[experimental]\ndurable_workers = false", "pi"),
        ("[experimental]\ndurable_workers = true", "durable"),
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
    let live = live("[experimental]\ndurable_workers = false");
    let id = live.dispatch(JOB).ok()["attempt"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(runner_of(&live, &id), "pi");

    // The flag moves after the Attempt exists: the Attempt does not.
    seed_home(&live.fx.env, "[experimental]\ndurable_workers = true");
    assert_eq!(
        live.fx.cpd(&["config"]).ok()["config"]["experimental"]["durable_workers"],
        true
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
    let live = live("[experimental]\ndurable_workers = true");
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
