//! `cpd session report` and the adapter `error` Event for the Manager (commands.md §10.18,
//! contract §5.2; hs-d573 cause A), the command tree in this process (`Fx`).

use crate::common;

use crate::common::session::setup_with_fork;
use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

fn errors(fx: &Fx) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", "error"]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// `events follow` owns its stdout, which `Fx` cannot read: this one call runs the binary (`fx.env.cpd`).
fn follow(fx: &Fx, session: &str) -> Vec<Value> {
    let run = fx.env.cpd(&[
        "events",
        "follow",
        "--session",
        session,
        "--until-idle",
        "--since",
        "0",
        "--kind",
        "error",
    ]);
    assert_eq!(run.exit, 0, "{run:?}");
    run.stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn report_appends_one_error_event_for_the_managers_digest() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let out = fx
        .cpd_with(
            fx.env.root(),
            &[
                "session",
                "report",
                "--what",
                "follower",
                "--code",
                "store_busy",
                "--message",
                "the store is busy",
                "--details",
                "{\"n\": 3}",
            ],
            &[("CPD_SESSION", "lead1")],
        )
        .ok();
    let events = errors(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(out, json!({"ok": true, "event": event["id"]}));
    assert_eq!(event["kind"], "error");
    assert_eq!(event["context"], "ctx");
    assert_eq!(
        (&event["plan"], &event["session"]),
        (&Value::Null, &json!("lead1"))
    );
    assert_eq!(event["data"]["source"], "adapter");
    assert_eq!(event["data"]["code"], "store_busy");
    assert_eq!(event["data"]["message"], "the store is busy");
    assert_eq!(
        event["data"]["details"],
        json!({"n": 3, "what": "follower", "session": "lead1", "plan": "pay"})
    );
    assert_eq!(
        event["text"],
        "φ adapter: follower: store_busy: the store is busy"
    );
}

#[test]
fn a_reported_error_names_its_session_and_no_follower_returns_it() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    for session in ["lead1", "fork1"] {
        fx.cpd(&[
            "session",
            "report",
            "--session",
            session,
            "--what",
            "w",
            "--code",
            "boom",
            "--message",
            "m",
        ])
        .ok();
    }
    let events = errors(&fx);
    assert_eq!(events.len(), 2, "{events:?}");
    let named: Vec<(&Value, &Value)> = events.iter().map(|e| (&e["plan"], &e["session"])).collect();
    assert_eq!(
        named,
        [
            (&Value::Null, &json!("lead1")),
            (&Value::Null, &json!("fork1"))
        ]
    );
    for session in ["lead1", "fork1", "lead2", "worker1"] {
        assert!(
            follow(&fx, session).is_empty(),
            "{session} followed an adapter error"
        );
    }
}

#[test]
fn events_list_by_session_finds_the_reported_error() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd(&[
        "session",
        "report",
        "--session",
        "lead1",
        "--what",
        "w",
        "--code",
        "boom",
        "--message",
        "m",
    ])
    .ok();
    let listed = fx
        .cpd(&["events", "list", "--session", "lead1", "--kind", "error"])
        .ok();
    let events = listed["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["session"], "lead1");
    assert_eq!(events[0]["data"]["code"], "boom");
    let other = fx
        .cpd(&["events", "list", "--session", "lead2", "--kind", "error"])
        .ok();
    assert!(other["events"].as_array().unwrap().is_empty());
}

#[test]
fn a_non_adapter_error_addressed_to_a_session_is_still_followed() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env
        .store()
        .write("fixture", |tx| {
            let txn = tx.id();
            tx.execute_batch(&format!(
                "INSERT INTO events (txn, at, kind, context, session, text, data) VALUES
                 ({txn}, '2026-02-01T00:00:00.000Z', 'error', 'ctx', 'lead1', 'φ session run: adapter_missing',
                  '{{\"source\": \"session run\", \"code\": \"adapter_missing\", \"details\": {{\"session\": \"lead1\"}}}}');"
            ))?;
            Ok(())
        })
        .unwrap();
    // The adapter's own error for the same Session is not returned, the other one is.
    fx.cpd(&[
        "session",
        "report",
        "--session",
        "lead1",
        "--what",
        "w",
        "--code",
        "boom",
        "--message",
        "m",
    ])
    .ok();
    assert_eq!(
        errors(&fx)
            .iter()
            .filter(|e| e["session"] == "lead1")
            .count(),
        2,
        "both errors name lead1"
    );
    let got = follow(&fx, "lead1");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["data"]["code"], "adapter_missing");
    assert_eq!(got[0]["data"]["source"], "session run");
}

#[test]
fn report_refuses_an_unknown_session_a_bad_code_and_a_bad_details_and_writes_nothing() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let report = |session: &str, code: &str, details: Option<&str>| {
        let mut args = vec![
            "session",
            "report",
            "--session",
            session,
            "--what",
            "w",
            "--code",
            code,
            "--message",
            "m",
        ];
        if let Some(d) = details {
            args.extend(["--details", d]);
        }
        fx.cpd(&args)
    };
    refused(&report("nope", "boom", None), "session_not_found", 4);
    let long = "a".repeat(65);
    for code in ["Boom", "1boom", "", long.as_str()] {
        refused(&report("lead1", code, None), "usage", 2);
    }
    for details in ["[1]", "\"s\"", "not json", "3"] {
        refused(&report("lead1", "boom", Some(details)), "usage", 2);
    }
    assert!(errors(&fx).is_empty());
}

fn report(fx: &Fx, session: &str, code: &str, message: &str) -> Value {
    fx.cpd(&[
        "session",
        "report",
        "--session",
        session,
        "--what",
        "w",
        "--code",
        code,
        "--message",
        message,
    ])
    .ok()
}

#[test]
fn a_report_repeated_for_the_session_inside_the_window_appends_one_event() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let first = report(&fx, "lead1", "boom", "m");
    let second = report(&fx, "lead1", "boom", "m");
    let events = errors(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(first, json!({"ok": true, "event": events[0]["id"]}));
    assert_eq!(second, json!({"ok": true, "event": events[0]["id"]}));
    report(&fx, "lead1", "boom", "m2");
    assert_eq!(errors(&fx).len(), 2, "another message appends");
    report(&fx, "fork1", "boom", "m");
    assert_eq!(errors(&fx).len(), 3, "another Session appends");
    report(&fx, "lead1", "boom2", "m");
    assert_eq!(errors(&fx).len(), 4, "another code appends");
}

#[test]
fn a_report_repeated_for_the_session_after_the_window_appends_again() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    report(&fx, "lead1", "boom", "m");
    report(&fx, "lead1", "boom", "m");
    let events = errors(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    fx.env
        .store()
        .write("fixture", |tx| {
            let guard: String = tx.query_row(
                "SELECT sql FROM sqlite_schema WHERE name = 'events_no_update'",
                [],
                |r| r.get(0),
            )?;
            tx.execute_batch(&format!(
                "DROP TRIGGER events_no_update;
                 UPDATE events SET at = '2026-01-01T00:00:00.000Z'
                  WHERE id = (SELECT min(id) FROM events WHERE kind = 'error');
                 {guard};"
            ))?;
            Ok(())
        })
        .unwrap();
    report(&fx, "lead1", "boom", "m");
    assert_eq!(errors(&fx).len(), 2, "older than the window appends again");
}
