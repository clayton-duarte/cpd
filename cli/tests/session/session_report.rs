//! `cpd session report` and the adapter `error` Event for the Manager (commands.md §10.18,
//! contract §5.2; hs-d573 cause A), the command tree in this process (`Fx`).

use crate::common;

use crate::common::session::setup_with_fork;
use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

fn errors(fx: &Fx) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", "error"]).ok()["events"].as_array().unwrap().clone()
}

/// `events follow` owns its stdout, which `Fx` cannot read: this one call runs the binary (`fx.env.cpd`).
fn follow(fx: &Fx, session: &str) -> Vec<Value> {
    let run = fx.env.cpd(&["events", "follow", "--session", session, "--until-idle", "--since", "0", "--kind", "error"]);
    assert_eq!(run.exit, 0, "{run:?}");
    run.stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn report_appends_one_error_event_for_the_managers_digest() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let out = fx.cpd_with(
        fx.env.root(),
        &["session", "report", "--what", "follower", "--code", "store_busy", "--message", "the store is busy", "--details", "{\"n\": 3}"],
        &[("CPD_SESSION", "lead1")],
    )
    .ok();
    let events = errors(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(out, json!({"ok": true, "event": event["id"]}));
    assert_eq!(event["kind"], "error");
    assert_eq!(event["context"], "ctx");
    assert_eq!((&event["plan"], &event["session"]), (&Value::Null, &Value::Null));
    assert_eq!(event["data"]["source"], "adapter");
    assert_eq!(event["data"]["code"], "store_busy");
    assert_eq!(event["data"]["message"], "the store is busy");
    assert_eq!(event["data"]["details"], json!({"n": 3, "what": "follower", "session": "lead1", "plan": "pay"}));
    assert_eq!(event["text"], "cpd adapter: follower: store_busy: the store is busy");
}

#[test]
fn a_reported_error_has_no_plan_and_no_session_so_no_lead_follows_it() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd(&["session", "report", "--session", "lead1", "--what", "w", "--code", "boom", "--message", "m"]).ok();
    assert_eq!(errors(&fx).len(), 1);
    assert!(follow(&fx, "lead1").is_empty());
    assert!(follow(&fx, "lead2").is_empty());
    assert!(follow(&fx, "worker1").is_empty());
}

#[test]
fn report_refuses_an_unknown_session_a_bad_code_and_a_bad_details_and_writes_nothing() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let report = |session: &str, code: &str, details: Option<&str>| {
        let mut args = vec!["session", "report", "--session", session, "--what", "w", "--code", code, "--message", "m"];
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
