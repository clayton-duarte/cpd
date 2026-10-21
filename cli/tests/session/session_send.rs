//! `cpd session send` and the `session-message` Event (commands.md §10.13,
//! contract §5.2), run as the built binary.

use crate::common;

use std::fs;

use common::{CPD, Run, TestEnv, refused};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{Value, json};

fn store(env: &TestEnv) -> Store {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000).unwrap()
}

pub(crate) fn exec_sql(env: &TestEnv, sql: &str) {
    store(env)
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
}

/// Context `ctx`; Plans `pay` (Lead `lead1`, idle, with a side fork `fork1`) and
/// `other` (Lead `lead2`, stopped); Worker `worker1` of `pay`.
pub(crate) fn setup(env: &TestEnv) {
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n").unwrap();
    let fp = "a".repeat(64);
    let mut store = store(env);
    store
        .write("fixture", |tx| {
            let txn = tx.id();
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from) VALUES
                   ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '/', 'O3', 'idle', 't1', 't1', NULL),
                   ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead two', '/', 'O3', 'stopped', 't2', 't2', NULL),
                   ('fork1', 'lead', 'ctx', 'pay', NULL, 'Side fork', '/', 'O3', 'stopped', 't4', 't4', 'lead1'),
                   ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '/', 'S2', 'working', 't3', 't3', NULL);
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
                   ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't'),
                   ('other', 'ctx', 'Other', 'grounding', 'lead2', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES
                   ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {txn}, 't'),
                   ('other', 'r', '/r', 'main', 'waiting', 'cpd/other', {txn}, 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {txn}, 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
                   VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');"
            ))?;
            Ok(())
        })
        .unwrap();
}

pub(crate) fn cpd_with(env: &TestEnv, vars: &[(&str, &str)], args: &[&str]) -> Run {
    let mut cmd = env.command(CPD);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    let out = cmd.args(args).output().unwrap();
    Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

pub(crate) fn messages(env: &TestEnv) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", "session-message"]).ok()["events"].as_array().unwrap().clone()
}

fn follow(env: &TestEnv, session: &str) -> Vec<Value> {
    let run = env.cpd(&["events", "follow", "--session", session, "--until-idle", "--since", "0", "--kind", "session-message"]);
    assert_eq!(run.exit, 0, "{run:?}");
    run.stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn a_send_appends_one_session_message_from_cpd_session() {
    let env = TestEnv::new();
    setup(&env);
    let out = cpd_with(&env, &[("CPD_SESSION", "lead2")], &["session", "send", "lead1", "hi there", "--unblocks", "u"]).ok();
    let events = messages(&env);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(out, json!({"ok": true, "event": event["id"], "session": "lead1", "from": "lead2", "unblocks": "u"}));
    assert_eq!(event["kind"], "session-message");
    assert_eq!((&event["session"], &event["context"], &event["plan"]), (&json!("lead1"), &json!("ctx"), &json!("pay")));
    assert_eq!((&event["job"], &event["attempt"]), (&Value::Null, &Value::Null));
    assert_eq!(event["data"], json!({"from": "lead2", "text": "hi there", "unblocks": "u"}));
    assert!(event["text"].as_str().unwrap().contains("(unblocks: u)"), "{event}");
}

#[test]
fn from_is_pi_session_id_when_cpd_session_is_unset_and_null_for_the_user() {
    let env = TestEnv::new();
    setup(&env);
    let pi = cpd_with(&env, &[("PI_SESSION_ID", "worker1")], &["session", "send", "lead1", "from pi", "--unblocks", "u"]).ok();
    assert_eq!(pi["from"], "worker1");
    let both = cpd_with(&env, &[("CPD_SESSION", "lead2"), ("PI_SESSION_ID", "worker1")], &["session", "send", "lead1", "both", "--unblocks", "u"]).ok();
    assert_eq!(both["from"], "lead2");
    let user = env.cpd(&["session", "send", "lead1", "from the user", "--unblocks", "u"]).ok();
    assert_eq!(user["from"], Value::Null);
    let events = messages(&env);
    assert_eq!(events.len(), 3);
    assert_eq!(events[2]["data"], json!({"from": null, "text": "from the user", "unblocks": "u"}));
    assert!(events[2]["text"].as_str().unwrap().contains("(unblocks: u)"), "{}", events[2]);
}

#[test]
fn the_event_text_quotes_the_first_60_characters() {
    let env = TestEnv::new();
    setup(&env);
    let message = format!("{}{}", "é".repeat(60), "tail");
    env.cpd(&["session", "send", "lead1", &message, "--unblocks", "u"]).ok();
    let event = &messages(&env)[0];
    assert!(event["text"].as_str().unwrap().contains(&"é".repeat(60)), "{event}");
    assert!(!event["text"].as_str().unwrap().contains("tail"), "{event}");
    assert_eq!(event["data"]["text"], message);
}

#[test]
fn a_message_can_come_from_a_file_as_written() {
    let env = TestEnv::new();
    setup(&env);
    let path = env.root().join("message.md");
    fs::write(&path, "line one\nline two\n").unwrap();
    let out = env.cpd(&["session", "send", "lead1", "--file", path.to_str().unwrap(), "--unblocks", "u"]).ok();
    assert_eq!(out["session"], "lead1");
    assert_eq!(messages(&env)[0]["data"]["text"], "line one\nline two\n");
}

#[test]
fn each_call_is_one_more_message() {
    let env = TestEnv::new();
    setup(&env);
    let a = env.cpd(&["session", "send", "lead1", "same", "--unblocks", "u"]).ok();
    let b = env.cpd(&["session", "send", "lead1", "same", "--unblocks", "u"]).ok();
    assert_ne!(a["event"], b["event"]);
    assert_eq!(messages(&env).len(), 2);
}

#[test]
fn every_kind_of_lead_is_accepted_a_stopped_one_too() {
    let env = TestEnv::new();
    setup(&env);
    for lead in ["lead1", "lead2", "fork1"] {
        let out = env.cpd(&["session", "send", lead, "hello", "--unblocks", "u"]).ok();
        assert_eq!(out["session"], lead);
    }
    let events = messages(&env);
    assert_eq!(events.iter().map(|e| e["session"].clone()).collect::<Vec<_>>(), ["lead1", "lead2", "fork1"]);
    assert_eq!((&events[2]["plan"], &events[2]["context"]), (&json!("pay"), &json!("ctx")));
}

#[test]
fn a_manager_is_a_target_and_an_archived_one_is_refused() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(
        &env,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/', 'O5', 'stopped', 't5', 't5', NULL)",
    );
    let out = cpd_with(&env, &[("CPD_SESSION", "lead1")], &["session", "send", "mgr1", "done", "--unblocks", "u"]).ok();
    assert_eq!(out["session"], "mgr1");
    let events = messages(&env);
    assert_eq!(events.len(), 1);
    assert_eq!((&events[0]["session"], &events[0]["context"], &events[0]["plan"]), (&json!("mgr1"), &json!("ctx"), &Value::Null));
    assert_eq!(events[0]["data"], json!({"from": "lead1", "text": "done", "unblocks": "u"}));
    exec_sql(&env, "UPDATE sessions SET archived_at = 't' WHERE id = 'mgr1'");
    refused(&env.cpd(&["session", "send", "mgr1", "again", "--unblocks", "u"]), "archived", 3);
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn the_refusals_write_nothing() {
    let env = TestEnv::new();
    setup(&env);
    let error = refused(&env.cpd(&["session", "send", "nope", "hi", "--unblocks", "u"]), "session_not_found", 4);
    assert_eq!(error["details"]["session"], "nope");
    let error = refused(&env.cpd(&["session", "send", "worker1", "hi", "--unblocks", "u"]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "worker");
    assert!(error["message"].as_str().unwrap().ends_with("is a worker, not a Lead or a Manager."), "{error}");
    exec_sql(&env, "UPDATE sessions SET archived_at = 't' WHERE id = 'fork1'");
    let error = refused(&env.cpd(&["session", "send", "fork1", "hi", "--unblocks", "u"]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd session unarchive fork1");
    assert!(messages(&env).is_empty());
    // Unarchived, it is accepted again.
    env.cpd(&["session", "unarchive", "fork1"]).ok();
    env.cpd(&["session", "send", "fork1", "hi", "--unblocks", "u"]).ok();
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn exactly_one_of_text_and_file_and_the_text_limits() {
    let env = TestEnv::new();
    setup(&env);
    let path = env.root().join("m.md");
    fs::write(&path, "x").unwrap();
    let file = path.to_str().unwrap();
    refused(&env.cpd(&["session", "send", "lead1", "--unblocks", "u"]), "usage", 2);
    refused(&env.cpd(&["session", "send", "lead1", "hi", "--file", file, "--unblocks", "u"]), "usage", 2);
    refused(&env.cpd(&["session", "send", "lead1", "", "--unblocks", "u"]), "usage", 2);
    refused(&env.cpd(&["session", "send", "lead1", &"a".repeat(20_001), "--unblocks", "u"]), "usage", 2);
    fs::write(&path, "").unwrap();
    refused(&env.cpd(&["session", "send", "lead1", "--file", file, "--unblocks", "u"]), "usage", 2);
    refused(&env.cpd(&["session", "send", "lead1", "--file", "/nonexistent/m.md", "--unblocks", "u"]), "file_unreadable", 2);
    assert!(messages(&env).is_empty());
    env.cpd(&["session", "send", "lead1", &"a".repeat(20_000), "--unblocks", "u"]).ok();
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn the_targets_follower_sees_the_message_and_the_senders_does_not() {
    let env = TestEnv::new();
    setup(&env);
    cpd_with(&env, &[("CPD_SESSION", "lead2")], &["session", "send", "lead1", "ping", "--unblocks", "u"]).ok();
    let seen = follow(&env, "lead1");
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!((&seen[0]["kind"], &seen[0]["session"], &seen[0]["data"]["text"]), (&json!("session-message"), &json!("lead1"), &json!("ping")));
    assert!(follow(&env, "lead2").is_empty());
    // A side fork of the same Plan receives only what is addressed to it.
    assert!(follow(&env, "fork1").is_empty());
    cpd_with(&env, &[("CPD_SESSION", "lead2")], &["session", "send", "fork1", "pong", "--unblocks", "u"]).ok();
    assert_eq!(follow(&env, "fork1").len(), 1);
    assert_eq!(follow(&env, "lead1").len(), 2); // the Plan's Lead sees the Plan's Events
}

const AGENT: &[(&str, &str)] = &[("CPD_SESSION", "lead2")];

#[test]
fn a_send_without_unblocks_is_a_usage_error() {
    let env = TestEnv::new();
    setup(&env);
    env.cpd(&["session", "send", "lead1", "ok", "--unblocks", "u"]).ok();
    refused(&env.cpd(&["session", "send", "lead1", "hi"]), "usage", 2);
    refused(&env.cpd(&["session", "send", "lead1", "hi", "--unblocks", ""]), "usage", 2);
    refused(&env.cpd(&["session", "send", "lead1", "hi", "--unblocks", &"a".repeat(201)]), "usage", 2);
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn every_send_wakes_and_carries_its_unblocks() {
    let env = TestEnv::new();
    setup(&env);
    let out = env.cpd(&["session", "send", "lead1", "x", "--unblocks", "PR #210 opened"]).ok();
    assert_eq!(out["unblocks"], "PR #210 opened");
    assert!(out.get("wake").is_none(), "{out}");
    let event = &messages(&env)[0];
    assert_eq!(event["data"], json!({"from": null, "text": "x", "unblocks": "PR #210 opened"}));
    assert!(event["text"].as_str().unwrap().contains("(unblocks: PR #210 opened)"), "{event}");
}

#[test]
fn the_wake_flag_is_gone() {
    let env = TestEnv::new();
    setup(&env);
    env.cpd(&["session", "send", "lead1", "x", "--unblocks", "u"]).ok();
    refused(&env.cpd(&["session", "send", "lead1", "y", "--unblocks", "u", "--wake"]), "usage", 2);
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn a_send_to_the_lead_of_a_done_plan_is_refused_plan_inactive() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(&env, "UPDATE plans SET phase = 'done' WHERE id = 'pay'");
    let error = refused(&cpd_with(&env, AGENT, &["session", "send", "lead1", "hi", "--unblocks", "u"]), "plan_inactive", 3);
    assert_eq!(error["details"]["plan"], "pay");
    assert_eq!(error["details"]["phase"], "done");
    assert!(messages(&env).is_empty());
}

#[test]
fn the_same_text_to_a_second_session_is_refused_broadcast() {
    let env = TestEnv::new();
    setup(&env);
    cpd_with(&env, AGENT, &["session", "send", "lead1", "news", "--unblocks", "u"]).ok();
    let error = refused(&cpd_with(&env, AGENT, &["session", "send", "fork1", " news ", "--unblocks", "u"]), "broadcast", 3);
    assert_eq!(error["details"]["session"], "lead1");
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn the_same_text_to_the_same_session_within_an_hour_is_refused_duplicate() {
    let env = TestEnv::new();
    setup(&env);
    cpd_with(&env, AGENT, &["session", "send", "lead1", "news", "--unblocks", "u"]).ok();
    let error = refused(&cpd_with(&env, AGENT, &["session", "send", "lead1", "news", "--unblocks", "u"]), "duplicate", 3);
    assert!(error["details"]["at"].is_string(), "{error}");
    assert_eq!(messages(&env).len(), 1);
}

#[test]
fn the_user_without_a_session_is_not_guarded() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(&env, "UPDATE plans SET phase = 'done' WHERE id = 'pay'");
    env.cpd(&["session", "send", "lead1", "same", "--unblocks", "u"]).ok();
    env.cpd(&["session", "send", "lead1", "same", "--unblocks", "u"]).ok();
    env.cpd(&["session", "send", "fork1", "same", "--unblocks", "u"]).ok();
    assert_eq!(messages(&env).len(), 3);
}
