//! The agent key written into `sessions.name` (contract §2.2 "Agent key"; commands.md `context manager`, `plan new`,
//! `session fork`): a Manager is its Context id, a Lead is `<context>:<plan>`, a fork keeps `<source name> (fork)` cut to
//! 98 characters. Every test goes through the CLI on the fake tmux.

use crate::common;

use common::fx::Fx;
use serde_json::{Value, json};

use super::driver::fixture;

/// The Context `ctx` with its config.
fn setup(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
}

fn new_plan(fx: &Fx) -> Value {
    fx.cpd(&[
        "plan",
        "new",
        "pay",
        "--context",
        "ctx",
        "--title",
        "Retry payouts",
    ])
    .ok()
}

fn registered(fx: &Fx) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", "session-registered"])
        .ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

fn name_of(fx: &Fx, role: &str) -> String {
    fx.env
        .query(&format!("SELECT name FROM sessions WHERE role = '{role}'"))
}

#[test]
fn agent_key_names_manager_is_named_by_its_context() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.cockpit();
    let out = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(out["manager"]["name"], json!("ctx"));
    assert_eq!(name_of(&fx, "manager"), "ctx");
}

#[test]
fn agent_key_names_lead_is_named_context_colon_plan() {
    let fx = Fx::with_ports();
    setup(&fx);
    let out = new_plan(&fx);
    assert_eq!(out["lead"]["name"], json!("ctx:pay"));
    assert_eq!(name_of(&fx, "lead"), "ctx:pay");
}

#[test]
fn agent_key_names_plan_new_has_no_name_flag() {
    let fx = Fx::with_ports();
    setup(&fx);
    let run = fx.cpd(&[
        "plan",
        "new",
        "pay",
        "--context",
        "ctx",
        "--title",
        "T",
        "--name",
        "x",
    ]);
    assert_eq!(run.exit, 2, "{run:?}");
    assert_eq!(run.error()["code"], "usage", "{run:?}");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 0);
}

#[test]
fn agent_key_names_session_registered_event_carries_the_key() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.cockpit();
    fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    new_plan(&fx);
    let names: Vec<(Value, Value)> = registered(&fx)
        .iter()
        .map(|e| (e["data"]["role"].clone(), e["data"]["name"].clone()))
        .collect();
    assert_eq!(
        names,
        [
            (json!("manager"), json!("ctx")),
            (json!("lead"), json!("ctx:pay"))
        ]
    );
}

fn fork_of_lead(fx: &Fx) -> String {
    let lead = fx
        .env
        .query::<String>("SELECT id FROM sessions WHERE role = 'lead'");
    fx.cpd(&["session", "fork", &lead]).ok()["session"]["name"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn agent_key_names_fork_keeps_the_source_name_plus_fork() {
    let fx = Fx::with_ports();
    setup(&fx);
    new_plan(&fx);
    assert_eq!(fork_of_lead(&fx), "ctx:pay (fork)");
}

#[test]
fn agent_key_names_fork_name_is_cut_to_98_characters() {
    let fx = Fx::with_ports();
    setup(&fx);
    new_plan(&fx);
    let long = format!("ctx:pay-{}", "x".repeat(98 - 8));
    assert_eq!(long.len(), 98);
    fixture(
        &fx.env,
        &format!("UPDATE sessions SET name = '{long}' WHERE role = 'lead';"),
    );
    let fork = fork_of_lead(&fx);
    assert_eq!(fork.chars().count(), 98, "{fork}");
    assert_eq!(fork, long);

    // A 90-character source: the suffix is cut after 98.
    let mid = format!("ctx:pay-{}", "y".repeat(90 - 8));
    fixture(
        &fx.env,
        &format!("UPDATE sessions SET name = '{mid}' WHERE role = 'lead';"),
    );
    assert_eq!(fork_of_lead(&fx), format!("{mid} (fork)")[..98]);
}
