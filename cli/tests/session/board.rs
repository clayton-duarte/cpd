//! `cpd board post | list | take` (commands.md §10a, hs-d514), the command tree in this process (`Fx`).

use crate::common::fx::Fx;
use crate::common::session::{events, setup_with_fork};
use crate::common::{TestEnv, refused};
use serde_json::{Value, json};

const FROM_LEAD2: &[(&str, &str)] = &[("CPD_SESSION", "lead2")];

fn with_manager(env: &TestEnv) {
    env.sql(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/', 'O5', 'idle', 't5', 't5', NULL)",
    );
}

fn post(fx: &Fx, text: &str, to: &str) -> Value {
    fx.cpd_with(
        fx.env.root(),
        &["board", "post", text, "--to", to],
        FROM_LEAD2,
    )
    .ok()
}

fn take(fx: &Fx, session: &str) -> Vec<Value> {
    fx.cpd(&["board", "take", "--session", session]).ok()["taken"]
        .as_array()
        .unwrap()
        .clone()
}

fn list(fx: &Fx, session: &str, all: bool) -> Vec<Value> {
    let mut args = vec!["board", "list", "--session", session];
    if all {
        args.push("--all");
    }
    fx.cpd(&args).ok()["posts"].as_array().unwrap().clone()
}

#[test]
fn a_post_to_a_plan_reaches_its_lead() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let out = post(&fx, "PR opened", "pay");
    assert_eq!(out["session"], "lead1");
    assert_eq!(out["context"], "ctx");
    assert!(out["posted_at"].is_string(), "{out}");
    let posts = list(&fx, "lead1", false);
    assert_eq!(posts.len(), 1);
    assert_eq!(
        (
            &posts[0]["from_session"],
            &posts[0]["text"],
            &posts[0]["read_at"]
        ),
        (&json!("lead2"), &json!("PR opened"), &Value::Null)
    );
    assert!(list(&fx, "lead2", false).is_empty());
}

#[test]
fn a_post_to_manager_reaches_the_context_manager() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    with_manager(&fx.env);
    let out = post(&fx, "Need a decision", "manager");
    assert_eq!(out["session"], "mgr1");
    let posts = list(&fx, "mgr1", false);
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["text"], "Need a decision");
}

#[test]
fn a_post_to_the_lead_of_a_done_plan_is_refused_plan_inactive() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env.sql(
        "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
         UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'pay'",
    );
    let error = refused(
        &fx.cpd_with(
            fx.env.root(),
            &["board", "post", "hi", "--to", "pay"],
            FROM_LEAD2,
        ),
        "plan_inactive",
        3,
    );
    assert_eq!(
        (&error["details"]["plan"], &error["details"]["phase"]),
        (&json!("pay"), &json!("done"))
    );
    assert!(list(&fx, "lead1", true).is_empty());
}

#[test]
fn take_returns_the_pending_posts_once_and_marks_them_read() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    post(&fx, "first", "pay");
    post(&fx, "second", "pay");
    let taken = take(&fx, "lead1");
    assert_eq!(
        taken.iter().map(|p| p["text"].clone()).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(take(&fx, "lead1").is_empty());
    assert!(list(&fx, "lead1", false).is_empty());
}

#[test]
fn list_all_shows_read_posts_too() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    post(&fx, "first", "pay");
    take(&fx, "lead1");
    post(&fx, "second", "pay");
    assert_eq!(list(&fx, "lead1", false).len(), 1);
    let all = list(&fx, "lead1", true);
    assert_eq!(
        all.iter().map(|p| p["text"].clone()).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(
        all[0]["read_at"].is_string() && all[1]["read_at"].is_null(),
        "{all:?}"
    );
}

#[test]
fn a_post_appends_no_event() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let before = fx.cpd(&["events", "list"]).ok()["events"]
        .as_array()
        .unwrap()
        .len();
    post(&fx, "quiet", "pay");
    let after = fx.cpd(&["events", "list"]).ok()["events"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(before, after);
    assert!(events(&fx, "session-message").is_empty());
}
