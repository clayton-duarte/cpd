//! `cpd session resume` and the `session-resumed` Event (commands.md §10.17, contract §5.2), and
//! `loop_guard_turns` in `session show` (§10.2), run as the built binary (hs-d552, hs-d571).

use crate::common;

use std::fs;

use crate::session_send::{exec_sql, setup};
use common::{TestEnv, refused};
use serde_json::{Value, json};

const GUARD: &str = "cpd: loop guard";

fn blocked(env: &TestEnv, id: &str, label: &str) {
    exec_sql(env, &format!("UPDATE sessions SET state = 'blocked', state_label = '{label}' WHERE id = '{id}'"));
}

fn resumed(env: &TestEnv) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", "session-resumed"]).ok()["events"].as_array().unwrap().clone()
}

fn count(env: &TestEnv, sql: &str) -> i64 {
    env.query::<i64>(sql)
}

#[test]
fn resume_a_loop_guarded_lead_writes_one_session_resumed_event() {
    let env = TestEnv::new();
    setup(&env);
    blocked(&env, "lead1", GUARD);
    let out = env.cpd(&["session", "resume", "lead1"]).ok();
    let events = resumed(&env);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(out, json!({"ok": true, "event": event["id"], "session": "lead1"}));
    assert_eq!(event["kind"], "session-resumed");
    assert_eq!((&event["context"], &event["plan"], &event["session"]), (&json!("ctx"), &json!("pay"), &json!("lead1")));
    assert_eq!(event["data"], json!({}));
    assert_eq!(event["text"], "Loop guard of Session lead1 resumed.");
    // Nothing is written to the Session: the adapter clears the state.
    let show = env.cpd(&["session", "show", "lead1"]).ok();
    assert_eq!((&show["session"]["state"], &show["session"]["state_label"]), (&json!("blocked"), &json!(GUARD)));
    // Each call is one more Event.
    let again = env.cpd(&["session", "resume", "lead1"]).ok();
    assert_ne!(again["event"], out["event"]);
    assert_eq!(resumed(&env).len(), 2);
}

#[test]
fn resume_a_loop_guarded_manager_writes_one_session_resumed_event() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(
        &env,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_label, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/', 'O5', 'blocked', 'cpd: loop guard', 't5', 't5', NULL)",
    );
    let out = env.cpd(&["session", "resume", "mgr1"]).ok();
    let events = resumed(&env);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(out, json!({"ok": true, "event": events[0]["id"], "session": "mgr1"}));
    assert_eq!((&events[0]["context"], &events[0]["plan"], &events[0]["session"]), (&json!("ctx"), &Value::Null, &json!("mgr1")));
    assert_eq!(events[0]["data"], json!({}));

    // A side fork is a Lead too.
    blocked(&env, "fork1", GUARD);
    env.cpd(&["session", "resume", "fork1"]).ok();
    assert_eq!(resumed(&env).len(), 2);
}

#[test]
fn resume_refuses_a_session_that_is_not_loop_guarded_and_writes_nothing() {
    let env = TestEnv::new();
    setup(&env);
    let events = |env: &TestEnv| count(env, "SELECT count(*) FROM events");
    let before = events(&env);

    // Idle and stopped Sessions.
    for (id, state) in [("lead1", "idle"), ("lead2", "stopped")] {
        let error = refused(&env.cpd(&["session", "resume", id]), "not_loop_guarded", 3);
        assert_eq!(error["details"], json!({"state": state, "label": null}), "{error}");
    }
    exec_sql(&env, "UPDATE sessions SET state = 'working' WHERE id = 'lead1'");
    let error = refused(&env.cpd(&["session", "resume", "lead1"]), "not_loop_guarded", 3);
    assert_eq!(error["details"], json!({"state": "working", "label": null}), "{error}");

    // Blocked with another label.
    blocked(&env, "lead1", "cpd: plan review");
    let error = refused(&env.cpd(&["session", "resume", "lead1"]), "not_loop_guarded", 3);
    assert_eq!(error["details"], json!({"state": "blocked", "label": "cpd: plan review"}), "{error}");
    blocked(&env, "lead1", "Which repo?");
    let error = refused(&env.cpd(&["session", "resume", "lead1"]), "not_loop_guarded", 3);
    assert_eq!(error["details"], json!({"state": "blocked", "label": "Which repo?"}), "{error}");

    // The label with another state.
    exec_sql(&env, &format!("UPDATE sessions SET state = 'waiting', state_label = '{GUARD}' WHERE id = 'lead1'"));
    let error = refused(&env.cpd(&["session", "resume", "lead1"]), "not_loop_guarded", 3);
    assert_eq!(error["details"], json!({"state": "waiting", "label": GUARD}), "{error}");

    assert!(resumed(&env).is_empty());
    assert_eq!(count(&env, "SELECT count(*) FROM events WHERE kind = 'session-resumed'"), 0);
    // The refusals wrote no Event (the fixture's own updates are direct SQL).
    assert_eq!(events(&env), before);
}

#[test]
fn resume_refuses_a_worker_an_archived_session_and_an_unknown_id() {
    let env = TestEnv::new();
    setup(&env);
    blocked(&env, "worker1", GUARD);
    blocked(&env, "fork1", GUARD);
    let error = refused(&env.cpd(&["session", "resume", "nope"]), "session_not_found", 4);
    assert_eq!(error["details"]["session"], "nope", "{error}");
    let error = refused(&env.cpd(&["session", "resume", "worker1"]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "worker", "{error}");
    exec_sql(&env, "UPDATE sessions SET archived_at = 't' WHERE id = 'fork1'");
    let error = refused(&env.cpd(&["session", "resume", "fork1"]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd session unarchive fork1");
    assert!(resumed(&env).is_empty());

    // Unarchived, it is accepted again.
    env.cpd(&["session", "unarchive", "fork1"]).ok();
    env.cpd(&["session", "resume", "fork1"]).ok();
    assert_eq!(resumed(&env).len(), 1);
}

#[test]
fn session_show_reports_loop_guard_turns() {
    let env = TestEnv::new();
    setup(&env);
    for id in ["lead1", "worker1"] {
        assert_eq!(env.cpd(&["session", "show", id]).ok()["session"]["loop_guard_turns"], 20, "{id}");
    }
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\nloop_guard_turns = 3\n").unwrap();
    for id in ["lead1", "worker1"] {
        assert_eq!(env.cpd(&["session", "show", id]).ok()["session"]["loop_guard_turns"], 3, "{id}");
    }
}
