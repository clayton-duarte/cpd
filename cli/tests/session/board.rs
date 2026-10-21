//! `cpd board post | list | take` (commands.md §10a, hs-d514), run as the built binary.

use crate::common::{TestEnv, refused};
use crate::session_send::{cpd_with, exec_sql, messages, setup};
use serde_json::{Value, json};

const FROM_LEAD2: &[(&str, &str)] = &[("CPD_SESSION", "lead2")];

fn with_manager(env: &TestEnv) {
    exec_sql(
        env,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/', 'O5', 'idle', 't5', 't5', NULL)",
    );
}

fn post(env: &TestEnv, text: &str, to: &str) -> Value {
    cpd_with(env, FROM_LEAD2, &["board", "post", text, "--to", to]).ok()
}

fn take(env: &TestEnv, session: &str) -> Vec<Value> {
    env.cpd(&["board", "take", "--session", session]).ok()["taken"].as_array().unwrap().clone()
}

fn list(env: &TestEnv, session: &str, all: bool) -> Vec<Value> {
    let mut args = vec!["board", "list", "--session", session];
    if all {
        args.push("--all");
    }
    env.cpd(&args).ok()["posts"].as_array().unwrap().clone()
}

#[test]
fn a_post_to_a_plan_reaches_its_lead() {
    let env = TestEnv::new();
    setup(&env);
    let out = post(&env, "PR opened", "pay");
    assert_eq!(out["session"], "lead1");
    assert_eq!(out["context"], "ctx");
    assert!(out["posted_at"].is_string(), "{out}");
    let posts = list(&env, "lead1", false);
    assert_eq!(posts.len(), 1);
    assert_eq!((&posts[0]["from_session"], &posts[0]["text"], &posts[0]["read_at"]), (&json!("lead2"), &json!("PR opened"), &Value::Null));
    assert!(list(&env, "lead2", false).is_empty());
}

#[test]
fn a_post_to_manager_reaches_the_context_manager() {
    let env = TestEnv::new();
    setup(&env);
    with_manager(&env);
    let out = post(&env, "Need a decision", "manager");
    assert_eq!(out["session"], "mgr1");
    let posts = list(&env, "mgr1", false);
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["text"], "Need a decision");
}

#[test]
fn a_post_to_the_lead_of_a_done_plan_is_refused_plan_inactive() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(&env, "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
         UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'pay'");
    let error = refused(&cpd_with(&env, FROM_LEAD2, &["board", "post", "hi", "--to", "pay"]), "plan_inactive", 3);
    assert_eq!((&error["details"]["plan"], &error["details"]["phase"]), (&json!("pay"), &json!("done")));
    assert!(list(&env, "lead1", true).is_empty());
}

#[test]
fn take_returns_the_pending_posts_once_and_marks_them_read() {
    let env = TestEnv::new();
    setup(&env);
    post(&env, "first", "pay");
    post(&env, "second", "pay");
    let taken = take(&env, "lead1");
    assert_eq!(taken.iter().map(|p| p["text"].clone()).collect::<Vec<_>>(), ["first", "second"]);
    assert!(take(&env, "lead1").is_empty());
    assert!(list(&env, "lead1", false).is_empty());
}

#[test]
fn list_all_shows_read_posts_too() {
    let env = TestEnv::new();
    setup(&env);
    post(&env, "first", "pay");
    take(&env, "lead1");
    post(&env, "second", "pay");
    assert_eq!(list(&env, "lead1", false).len(), 1);
    let all = list(&env, "lead1", true);
    assert_eq!(all.iter().map(|p| p["text"].clone()).collect::<Vec<_>>(), ["first", "second"]);
    assert!(all[0]["read_at"].is_string() && all[1]["read_at"].is_null(), "{all:?}");
}

#[test]
fn a_post_appends_no_event() {
    let env = TestEnv::new();
    setup(&env);
    let before = env.cpd(&["events", "list"]).ok()["events"].as_array().unwrap().len();
    post(&env, "quiet", "pay");
    let after = env.cpd(&["events", "list"]).ok()["events"].as_array().unwrap().len();
    assert_eq!(before, after);
    assert!(messages(&env).is_empty());
}
