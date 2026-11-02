//! `cpd session resume` and the `session-resumed` Event (commands.md §10.17, contract §5.2), and
//! `loop_guard_turns` in `session show` (§10.2), the command tree in this process (`Fx`; hs-d552, hs-d571).

use crate::common;

use std::fs;

use crate::common::session::{events, setup_with_fork};
use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

const GUARD: &str = "φ: loop guard";

fn blocked(fx: &Fx, id: &str, label: &str) {
    fx.env.sql(&format!(
        "UPDATE sessions SET state = 'blocked', state_label = '{label}' WHERE id = '{id}'"
    ));
}

fn resumed(fx: &Fx) -> Vec<Value> {
    events(fx, "session-resumed")
}

fn count(fx: &Fx, sql: &str) -> i64 {
    fx.env.query::<i64>(sql)
}

#[test]
fn resume_a_loop_guarded_lead_writes_one_session_resumed_event() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    blocked(&fx, "lead1", GUARD);
    let out = fx.cpd(&["session", "resume", "lead1"]).ok();
    let events = resumed(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(
        out,
        json!({"ok": true, "event": event["id"], "session": "lead1"})
    );
    assert_eq!(event["kind"], "session-resumed");
    assert_eq!(
        (&event["context"], &event["plan"], &event["session"]),
        (&json!("ctx"), &json!("pay"), &json!("lead1"))
    );
    assert_eq!(event["data"], json!({}));
    assert_eq!(event["text"], "Loop guard of Session lead1 resumed.");
    // Nothing is written to the Session: the adapter clears the state.
    let show = fx.cpd(&["session", "show", "lead1"]).ok();
    assert_eq!(
        (&show["session"]["state"], &show["session"]["state_label"]),
        (&json!("blocked"), &json!(GUARD))
    );
    // Each call is one more Event.
    let again = fx.cpd(&["session", "resume", "lead1"]).ok();
    assert_ne!(again["event"], out["event"]);
    assert_eq!(resumed(&fx).len(), 2);
}

#[test]
fn resume_a_loop_guarded_manager_writes_one_session_resumed_event() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env.sql("INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_label, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/', 'O4', 'blocked', 'φ: loop guard', 't5', 't5', NULL)",
    );
    let out = fx.cpd(&["session", "resume", "mgr1"]).ok();
    let events = resumed(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        out,
        json!({"ok": true, "event": events[0]["id"], "session": "mgr1"})
    );
    assert_eq!(
        (
            &events[0]["context"],
            &events[0]["plan"],
            &events[0]["session"]
        ),
        (&json!("ctx"), &Value::Null, &json!("mgr1"))
    );
    assert_eq!(events[0]["data"], json!({}));

    // A side fork is a Lead too.
    blocked(&fx, "fork1", GUARD);
    fx.cpd(&["session", "resume", "fork1"]).ok();
    assert_eq!(resumed(&fx).len(), 2);
}

#[test]
fn resume_refuses_a_session_that_is_not_loop_guarded_and_writes_nothing() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let events = |fx: &Fx| count(fx, "SELECT count(*) FROM events");
    let before = events(&fx);

    // Idle and stopped Sessions.
    for (id, state) in [("lead1", "idle"), ("lead2", "stopped")] {
        let error = refused(&fx.cpd(&["session", "resume", id]), "not_loop_guarded", 3);
        assert_eq!(
            error["details"],
            json!({"state": state, "label": null}),
            "{error}"
        );
    }
    fx.env
        .sql("UPDATE sessions SET state = 'working' WHERE id = 'lead1'");
    let error = refused(
        &fx.cpd(&["session", "resume", "lead1"]),
        "not_loop_guarded",
        3,
    );
    assert_eq!(
        error["details"],
        json!({"state": "working", "label": null}),
        "{error}"
    );

    // Blocked with another label.
    blocked(&fx, "lead1", "cpd: plan review");
    let error = refused(
        &fx.cpd(&["session", "resume", "lead1"]),
        "not_loop_guarded",
        3,
    );
    assert_eq!(
        error["details"],
        json!({"state": "blocked", "label": "cpd: plan review"}),
        "{error}"
    );
    blocked(&fx, "lead1", "Which repo?");
    let error = refused(
        &fx.cpd(&["session", "resume", "lead1"]),
        "not_loop_guarded",
        3,
    );
    assert_eq!(
        error["details"],
        json!({"state": "blocked", "label": "Which repo?"}),
        "{error}"
    );

    // The label with another state.
    fx.env.sql(&format!(
        "UPDATE sessions SET state = 'waiting', state_label = '{GUARD}' WHERE id = 'lead1'"
    ));
    let error = refused(
        &fx.cpd(&["session", "resume", "lead1"]),
        "not_loop_guarded",
        3,
    );
    assert_eq!(
        error["details"],
        json!({"state": "waiting", "label": GUARD}),
        "{error}"
    );

    assert!(resumed(&fx).is_empty());
    assert_eq!(
        count(
            &fx,
            "SELECT count(*) FROM events WHERE kind = 'session-resumed'"
        ),
        0
    );
    // The refusals wrote no Event (the fixture's own updates are direct SQL).
    assert_eq!(events(&fx), before);
}

#[test]
fn resume_refuses_a_worker_an_archived_session_and_an_unknown_id() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    blocked(&fx, "worker1", GUARD);
    blocked(&fx, "fork1", GUARD);
    let error = refused(
        &fx.cpd(&["session", "resume", "nope"]),
        "session_not_found",
        4,
    );
    assert_eq!(error["details"]["session"], "nope", "{error}");
    let error = refused(&fx.cpd(&["session", "resume", "worker1"]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "worker", "{error}");
    fx.env
        .sql("UPDATE sessions SET archived_at = 't' WHERE id = 'fork1'");
    let error = refused(&fx.cpd(&["session", "resume", "fork1"]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd session unarchive fork1");
    assert!(resumed(&fx).is_empty());

    // Unarchived, it is accepted again.
    fx.cpd(&["session", "unarchive", "fork1"]).ok();
    fx.cpd(&["session", "resume", "fork1"]).ok();
    assert_eq!(resumed(&fx).len(), 1);
}

#[test]
fn session_show_reports_loop_guard_turns() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    for id in ["lead1", "worker1"] {
        assert_eq!(
            fx.cpd(&["session", "show", id]).ok()["session"]["loop_guard_turns"],
            20,
            "{id}"
        );
    }
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\nloop_guard_turns = 3\n",
    )
    .unwrap();
    for id in ["lead1", "worker1"] {
        assert_eq!(
            fx.cpd(&["session", "show", id]).ok()["session"]["loop_guard_turns"],
            3,
            "{id}"
        );
    }
}
