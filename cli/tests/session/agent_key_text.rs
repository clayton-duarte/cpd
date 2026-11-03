//! The `session-message` Event text and the `from_name` field of `board take | list` (contract §5.2; commands.md
//! §10.13, §10a; pro-d345, pro-d346): the text names the sender by its `sessions.name` and uuid, never the target.

use crate::common;

use common::fx::Fx;
use common::session::{events, setup_with_fork};
use serde_json::{Value, json};

const KEY: &str = "cpd:plan-x:job-y";

/// `setup_with_fork`, Worker `worker1` named `cpd:plan-x:job-y`, Lead `lead2` named `cpd:plan-x`.
fn world() -> Fx {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env.sql(&format!(
        "UPDATE sessions SET name = '{KEY}' WHERE id = 'worker1';
         UPDATE sessions SET name = 'cpd:plan-x' WHERE id = 'lead2';"
    ));
    fx
}

fn send(fx: &Fx, from: Option<&str>, extra: &[&str]) -> String {
    // `session send` requires `--unblocks`: `extra` overrides the default reason.
    let mut args = vec!["session", "send", "lead1", "hello lead"];
    args.extend_from_slice(if extra.is_empty() {
        &["--unblocks", "r"]
    } else {
        extra
    });
    let env: Vec<(&str, &str)> = from.map(|f| ("CPD_SESSION", f)).into_iter().collect();
    fx.cpd_with(fx.env.root(), &args, &env).ok();
    let found = events(fx, "session-message");
    assert_eq!(found.len(), 1);
    found[0]["text"].as_str().unwrap().to_string()
}

#[test]
fn session_send_event_text_names_the_sender_key_and_uuid() {
    let fx = world();
    let text = send(&fx, Some("worker1"), &[]);
    let prefix = format!("Message from {KEY} (worker1)");
    assert!(text.starts_with(&prefix), "{text}");
    assert!(text.ends_with(": hello lead"), "{text}");
    assert!(!text.contains("Message for"), "{text}");
}

#[test]
fn session_send_event_text_from_the_user_names_the_user() {
    let fx = world();
    let text = send(&fx, None, &[]);
    assert!(text.starts_with("Message from the User"), "{text}");
    assert!(!text.contains("Message for"), "{text}");
}

#[test]
fn session_send_unblocks_event_text_keeps_the_reason() {
    let fx = world();
    let text = send(&fx, Some("worker1"), &["--unblocks", "why"]);
    let prefix = format!("Message from {KEY} (worker1) (unblocks: why): ");
    assert!(text.starts_with(&prefix), "{text}");
}

fn post(fx: &Fx, from: Option<&str>) {
    let env: Vec<(&str, &str)> = from.map(|f| ("CPD_SESSION", f)).into_iter().collect();
    fx.cpd_with(
        fx.env.root(),
        &["board", "post", "PR opened", "--to", "pay"],
        &env,
    )
    .ok();
}

#[test]
fn board_take_carries_the_sender_name() {
    let fx = world();
    post(&fx, Some("lead2"));
    let listed = fx.cpd(&["board", "list", "--session", "lead1"]).ok()["posts"].clone();
    assert_eq!(listed[0]["from_session"], "lead2");
    assert_eq!(listed[0]["from_name"], "cpd:plan-x");
    let taken = fx.cpd(&["board", "take", "--session", "lead1"]).ok()["taken"].clone();
    assert_eq!(taken[0]["from_session"], "lead2");
    assert_eq!(taken[0]["from_name"], "cpd:plan-x");
}

#[test]
fn board_take_carries_a_null_sender_name_for_the_user() {
    let fx = world();
    post(&fx, None);
    let taken = fx.cpd(&["board", "take", "--session", "lead1"]).ok()["taken"].clone();
    assert_eq!(taken[0]["from_session"], Value::Null);
    assert_eq!(taken[0].get("from_name"), Some(&json!(null)));
}
