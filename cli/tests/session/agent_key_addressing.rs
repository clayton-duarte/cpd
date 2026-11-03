//! Addressing by agent key (commands.md §1.3, §10a.1): every Session-accepting command and `board post --to` take an
//! agent key (`ctx`, `ctx:pay`, `ctx:pay:api`), a relative key (a Plan id or a Job key, from the caller's
//! `CPD_SESSION`) and the legacy `<context>-manager` alias.

use crate::common::fx::Fx;
use crate::common::refused;
use crate::common::session::{events, setup_with_fork};
use serde_json::{Value, json};

const FROM_LEAD1: &[(&str, &str)] = &[("CPD_SESSION", "lead1")];
const FROM_LEAD2: &[(&str, &str)] = &[("CPD_SESSION", "lead2")];

/// `setup_with_fork`, with every agent named by its key: Manager `mgr1` (`ctx`), Leads `lead1` (`ctx:pay`) and `lead2`
/// (`ctx:other`), and Job `api` of `pay` with Attempt 1 (`worker1`, ended) and Attempt 2 (`worker2`), both `ctx:pay:api`.
fn world() -> Fx {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env.sql(
        "UPDATE attempts SET status = 'ended', outcome = 'cancelled', cause = 'cancel_requested', ended_at = 't' WHERE id = 'api-a1';
         UPDATE sessions SET state = 'stopped' WHERE id = 'worker1';
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
           VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx', '/', 'O4', 'idle', 't5', 't5', NULL);
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
           VALUES ('worker2', 'worker', 'ctx', 'pay', 'api-a2', 'ctx:pay:api', '/', 'S2', 'working', 't6', 't6', NULL);
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'api-a2', 'api', 2, 'S2', 'ladder', 'running', 'cpd/pay--api-a2', '/w2', 'worker2', 't');
         UPDATE sessions SET name = 'ctx:pay' WHERE id = 'lead1';
         UPDATE sessions SET name = 'ctx:other' WHERE id = 'lead2';
         UPDATE sessions SET name = 'ctx:pay:api' WHERE id = 'worker1';",
    );
    fx
}

fn shown(fx: &Fx, reference: &str, caller: &[(&str, &str)]) -> Value {
    fx.cpd_with(fx.env.root(), &["session", "show", reference], caller)
        .ok()["session"]
        .clone()
}

fn assert_shows(reference: &str, caller: &[(&str, &str)], id: &str) {
    let fx = world();
    assert_eq!(shown(&fx, reference, caller)["id"], id, "show {reference}");
}

fn done(fx: &Fx) {
    fx.env.sql(
        "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
         UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'pay'",
    );
}

#[test]
fn session_show_takes_a_manager_key() {
    assert_shows("ctx", &[], "mgr1");
}

#[test]
fn session_show_takes_a_lead_key() {
    assert_shows("ctx:pay", &[], "lead1");
}

#[test]
fn session_show_takes_a_worker_key() {
    assert_shows("ctx:pay:api", &[], "worker2");
}

#[test]
fn session_show_takes_a_relative_plan_id_from_a_lead() {
    // The key `ctx:other` is looked up by name, not through the Plan's `lead_session` link the legacy form follows.
    let fx = world();
    fx.env
        .sql("UPDATE plans SET lead_session = NULL WHERE id = 'other'");
    assert_eq!(shown(&fx, "other", FROM_LEAD1)["id"], "lead2");
}

#[test]
fn session_show_takes_a_relative_job_key_from_a_lead() {
    assert_shows("api", FROM_LEAD1, "worker2");
}

#[test]
fn session_show_takes_the_legacy_manager_alias() {
    assert_shows("ctx-manager", &[], "mgr1");
}

#[test]
fn session_send_takes_a_lead_key() {
    let fx = world();
    fx.cpd_with(
        fx.env.root(),
        &["session", "send", "ctx:pay", "text", "--unblocks", "u"],
        FROM_LEAD2,
    )
    .ok();
    let sent = events(&fx, "session-message");
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["session"], "lead1");
}

#[test]
fn board_post_to_takes_a_worker_key() {
    let fx = world();
    let out = fx
        .cpd_with(
            fx.env.root(),
            &["board", "post", "text", "--to", "ctx:pay:api"],
            FROM_LEAD2,
        )
        .ok();
    assert_eq!(out["session"], "worker2", "{out}");
    assert_eq!(
        fx.env.query::<String>("SELECT to_session FROM board_posts"),
        "worker2"
    );
}

#[test]
fn board_post_to_a_lead_of_a_done_plan_is_plan_inactive() {
    let fx = world();
    done(&fx);
    refused(
        &fx.cpd_with(
            fx.env.root(),
            &["board", "post", "hi", "--to", "ctx:pay"],
            FROM_LEAD2,
        ),
        "plan_inactive",
        3,
    );
}

#[test]
fn session_archive_and_unarchive_take_a_lead_key() {
    let fx = world();
    fx.cpd(&["session", "archive", "ctx:other"]).ok();
    assert!(shown(&fx, "lead2", &[])["archived_at"].is_string());
    fx.cpd(&["session", "unarchive", "ctx:other"]).ok();
    assert_eq!(shown(&fx, "lead2", &[])["archived_at"], Value::Null);
}

#[test]
fn board_list_session_flag_takes_a_manager_key() {
    let fx = world();
    fx.cpd_with(
        fx.env.root(),
        &["board", "post", "Need a decision", "--to", "manager"],
        FROM_LEAD2,
    )
    .ok();
    let out = fx.cpd(&["board", "list", "--session", "ctx"]).ok();
    let posts = out["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 1, "{out}");
    assert_eq!(posts[0]["text"], json!("Need a decision"));
}
