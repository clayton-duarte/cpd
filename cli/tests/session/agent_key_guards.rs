//! The agent-key guards (contract §2.2 "Agent key", commands.md §1.2, §3.1, §4.1, §10 `session rename`): the name of a
//! Manager, a Lead that is not a fork and a Worker is its key and `session rename` refuses it (`key_fixed`); a fork is
//! renamed; `plan new` and `context new` refuse an id the other table holds (`id_taken`) unless the row exists (`already`).

use crate::common;

use common::fx::Fx;
use common::refused;
use common::session::{events, setup, show};
use serde_json::{Value, json};

/// A side fork `fork1` of `lead1` (stopped, no pane) in the world of `setup`.
fn add_fork(fx: &Fx) {
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, forked_from, cwd, tier, state, state_at, created_at) VALUES
           ('fork1', 'lead', 'ctx', 'pay', 'Side fork', 'lead1', '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't8')",
    );
}

fn renamed_events(fx: &Fx) -> usize {
    events(fx, "session-renamed").len()
}

fn refuse_rename(fx: &Fx, session: &str) {
    let current = show(fx, session)["name"].as_str().unwrap().to_owned();
    for name in ["a new name", current.as_str()] {
        refused(
            &fx.cpd(&["session", "rename", name, "--session", session]),
            "key_fixed",
            3,
        );
    }
}

#[test]
fn agent_key_guards_rename_of_a_manager_is_refused_key_fixed() {
    let fx = Fx::new();
    setup(&fx.env);
    refuse_rename(&fx, "manager1");
}

#[test]
fn agent_key_guards_rename_of_a_lead_is_refused_key_fixed() {
    let fx = Fx::new();
    setup(&fx.env);
    refuse_rename(&fx, "lead1");
}

#[test]
fn agent_key_guards_rename_of_a_worker_is_refused_key_fixed() {
    let fx = Fx::new();
    setup(&fx.env);
    refuse_rename(&fx, "worker1");
}

#[test]
fn agent_key_guards_rename_of_a_fork_is_allowed() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    add_fork(&fx);
    let out = fx
        .cpd(&["session", "rename", "side quest", "--session", "fork1"])
        .ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": "fork1", "name": "side quest"})
    );
    assert_eq!(show(&fx, "fork1")["name"], "side quest");
    let renamed = events(&fx, "session-renamed");
    assert_eq!(renamed.len(), 1);
    assert_eq!(
        (&renamed[0]["session"], &renamed[0]["data"]),
        (&json!("fork1"), &json!({"name": "side quest"}))
    );
}

#[test]
fn agent_key_guards_a_refused_rename_changes_nothing() {
    let fx = Fx::new();
    setup(&fx.env);
    let names = |fx: &Fx| -> Vec<Value> {
        ["manager1", "lead1", "worker1"]
            .iter()
            .map(|s| show(fx, s)["name"].clone())
            .collect()
    };
    let before = names(&fx);
    let events_before = fx.env.query::<i64>("SELECT count(*) FROM events");
    let txns = fx.env.query::<i64>("SELECT count(*) FROM txns");
    for session in ["manager1", "lead1", "worker1"] {
        refused(
            &fx.cpd(&["session", "rename", "changed", "--session", session]),
            "key_fixed",
            3,
        );
    }
    assert_eq!(names(&fx), before);
    assert_eq!(renamed_events(&fx), 0);
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events"),
        events_before
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
}

/// A Context `ctx` with its Plan `pay`, made by the commands, on fake ports.
fn world() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"hs\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    fx.fake_cpd_views();
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"])
        .ok();
    fx
}

fn count(fx: &Fx, sql: &str) -> i64 {
    fx.env.query::<i64>(sql)
}

#[test]
fn agent_key_guards_plan_new_refuses_a_plan_id_equal_to_a_context_id() {
    let fx = world();
    let plans = count(&fx, "SELECT count(*) FROM plans");
    let sessions = count(&fx, "SELECT count(*) FROM sessions");
    refused(
        &fx.cpd(&["plan", "new", "ctx", "--context", "ctx", "--title", "Clash"]),
        "id_taken",
        3,
    );
    assert_eq!(count(&fx, "SELECT count(*) FROM plans"), plans);
    assert_eq!(count(&fx, "SELECT count(*) FROM plans WHERE id = 'ctx'"), 0);
    assert_eq!(count(&fx, "SELECT count(*) FROM sessions"), sessions);
}

#[test]
fn agent_key_guards_context_new_refuses_a_context_id_equal_to_a_plan_id() {
    let fx = world();
    let contexts = count(&fx, "SELECT count(*) FROM contexts");
    refused(
        &fx.cpd(&[
            "context", "new", "pay", "--title", "Clash", "--owner", "acme",
        ]),
        "id_taken",
        3,
    );
    assert_eq!(count(&fx, "SELECT count(*) FROM contexts"), contexts);
    assert_eq!(
        count(&fx, "SELECT count(*) FROM contexts WHERE id = 'pay'"),
        0
    );
}

#[test]
fn agent_key_guards_a_rerun_of_an_existing_pair_is_still_already() {
    let fx = world();
    // The pair `bifrost` and `gatemac` predate `id_taken`: a Context and a Plan hold the same id.
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on, owner) VALUES ('pay', 'Twin', 't', 'm1', 'acme')",
    );
    let rows = |fx: &Fx| {
        (
            count(fx, "SELECT count(*) FROM contexts"),
            count(fx, "SELECT count(*) FROM plans"),
            count(fx, "SELECT count(*) FROM sessions"),
            count(fx, "SELECT count(*) FROM events"),
        )
    };
    let before = rows(&fx);
    let plan = fx
        .cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"])
        .ok();
    assert_eq!(plan["already"], true, "{plan}");
    let context = fx
        .cpd(&[
            "context", "new", "pay", "--title", "Twin", "--owner", "acme",
        ])
        .ok();
    assert_eq!(context["already"], true, "{context}");
    assert_eq!(rows(&fx), before);
}
