//! `plan new --runner <pi|durable>` (commands.md §4.1, contract §6.19; hs-d820, hs-d754 Q10): the runner is
//! chosen at creation, stored on `sessions.runner`, and never changed. With the flag omitted the config key
//! `lead_runner` is the default (default `pi`). Run in this process (`Fx::with_ports`) on the fake tmux.

use crate::common;

use common::fx::Fx;
use serde_json::Value;

/// The Context `ctx` and the cockpit.
fn setup(fx: &Fx, extra: &str) {
    fx.env.write_tmux_config(extra);
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cockpit();
}

/// `plan new <id>` with an optional `--runner`.
fn plan_new(fx: &Fx, id: &str, runner: Option<&str>) -> common::Run {
    let mut args = vec!["plan", "new", id, "--context", "ctx", "--title", "Plan"];
    if let Some(runner) = runner {
        args.extend(["--runner", runner]);
    }
    fx.cpd(&args)
}

/// The stored runner of `session`, read from `session show`.
fn runner_of(fx: &Fx, session: &str) -> String {
    fx.cpd(&["session", "show", session]).ok()["session"]["runner"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn count(fx: &Fx, table: &str) -> i64 {
    fx.env.query(&format!("SELECT count(*) FROM {table}"))
}

#[test]
fn a_new_plan_stores_the_runner_it_was_asked_for() {
    let fx = Fx::with_ports();
    setup(&fx, "lead_runner = \"pi\"");
    let out = plan_new(&fx, "pl", Some("durable")).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    assert_eq!(out["lead"]["pane"], Value::Null, "{out}");
    assert_eq!(runner_of(&fx, &lead), "durable");
}

#[test]
fn a_runner_flag_overrides_the_config_default() {
    let fx = Fx::with_ports();
    setup(&fx, "lead_runner = \"durable\"");
    let out = plan_new(&fx, "pl", Some("pi")).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    assert!(!out["lead"]["pane"].is_null(), "{out}");
    assert_eq!(runner_of(&fx, &lead), "pi");
}

#[test]
fn an_invalid_runner_is_refused_before_anything_is_written() {
    let fx = Fx::with_ports();
    setup(&fx, "");
    // A durable Plan first, so the test is red before the flag exists.
    plan_new(&fx, "pl", Some("durable")).ok();

    let plans = count(&fx, "plans");
    let sessions = count(&fx, "sessions");
    common::refused(&plan_new(&fx, "p2", Some("tcp")), "usage", 2);
    assert_eq!(count(&fx, "plans"), plans);
    assert_eq!(count(&fx, "sessions"), sessions);
}

#[test]
fn a_rerun_with_a_different_runner_is_refused() {
    let fx = Fx::with_ports();
    setup(&fx, "");
    let out = plan_new(&fx, "pl", Some("pi")).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    common::refused(&plan_new(&fx, "pl", Some("durable")), "plan_exists", 3);
    assert_eq!(runner_of(&fx, &lead), "pi");
}

#[test]
fn durable_leads_true_makes_a_new_lead_durable() {
    let fx = Fx::with_ports();
    setup(&fx, "[experimental]\ndurable_leads = true");
    let out = plan_new(&fx, "pl", None).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    assert_eq!(out["lead"]["pane"], Value::Null, "{out}");
    assert_eq!(runner_of(&fx, &lead), "durable");
}

#[test]
fn an_explicit_runner_beats_durable_leads() {
    let fx = Fx::with_ports();
    setup(&fx, "[experimental]\ndurable_leads = true");
    let out = plan_new(&fx, "pl", Some("pi")).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    assert!(!out["lead"]["pane"].is_null(), "{out}");
    assert_eq!(runner_of(&fx, &lead), "pi");
}

#[test]
fn durable_leads_false_uses_lead_runner() {
    let fx = Fx::with_ports();
    setup(
        &fx,
        "lead_runner = \"durable\"\n[experimental]\ndurable_leads = false",
    );
    let out = plan_new(&fx, "pl", None).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    assert_eq!(runner_of(&fx, &lead), "durable");
}
