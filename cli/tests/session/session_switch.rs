//! `cpd session switch-failed` and `cpd session switched` (commands.md §10.16, the internal commands the pi adapter runs), the
//! column `sessions.switch_failed` and the key `switch` of `session show`. The real binary on a temp store: no tmux, no pi.
//!
//! Every test here fails before the commands exist: the binary refuses them (`usage`, exit 2).

use crate::common::fx::Fx;
use crate::common::session::{setup, show};
use crate::common::{TestEnv, refused};

use cpd_core::home::CpdHome;
use cpd_core::store::testing::{named_migrations, prefix_database};
use serde_json::{Value, json};

const LEAD: &str = "lead1";
const WORKER: &str = "worker1";

/// `session.rs`'s fixture with the config of `session_retier.rs`: Lead `lead1` on `O3`, whose target is `O2`.
fn fixture(env: &TestEnv) {
    setup(env);
    env.write_models();
    env.write_tmux_config("lead_tier = \"O2\"\nmanager_tier = \"O4\"\n[timeouts]\nfork_quiet_seconds = 1\n");
}

fn all_events(fx: &Fx) -> Value {
    fx.cpd(&["events", "list"]).ok()["events"].clone()
}

fn switch(fx: &Fx, id: &str) -> Value {
    show(fx, id)["switch"].clone()
}

fn failed(fx: &Fx, id: &str) {
    fx.cpd(&["session", "switch-failed", "--session", id, "--code", "model_unavailable", "--message", "p/opus: pi has no authentication"]).ok();
}

/// A failed switch of a retiered Lead: `switch` has the row's tier (the target), the code and message as given, a time
/// and `next`, a `cpd` command; an untouched Session has `switch: null`.
#[test]
fn switch_failed_shows_on_session_show_with_the_target_tier() {
    let fx = Fx::new();
    fixture(&fx.env);
    fx.cpd(&["session", "retier", LEAD]).ok();
    assert_eq!(switch(&fx, LEAD), Value::Null);
    assert_eq!(show(&fx, "lead2").get("switch"), Some(&Value::Null), "the key is there, null");

    failed(&fx, LEAD);
    let s = switch(&fx, LEAD);
    assert_eq!((s["tier"].as_str(), s["code"].as_str(), s["message"].as_str()), (Some("O2"), Some("model_unavailable"), Some("p/opus: pi has no authentication")), "{s}");
    assert!(s["at"].as_str().is_some_and(|at| at.ends_with('Z') && at.len() >= 20), "an RFC 3339 UTC time: {s}");
    assert!(s["next"].as_str().is_some_and(|n| n.starts_with("cpd ")), "next is a cpd command: {s}");
    assert_eq!(show(&fx, LEAD)["tier"], "O2", "the row's tier stays the target");
    assert_eq!(switch(&fx, "lead2"), Value::Null, "only that Session");
}

/// `switched` clears it; both commands, run twice, leave the same row and append no Event.
#[test]
fn switched_clears_the_failure_and_both_are_idempotent() {
    let fx = Fx::new();
    fixture(&fx.env);
    fx.cpd(&["session", "retier", LEAD]).ok();
    let events = all_events(&fx);

    failed(&fx, LEAD);
    let once = switch(&fx, LEAD);
    assert_ne!(once, Value::Null);
    failed(&fx, LEAD);
    let twice = switch(&fx, LEAD);
    assert_eq!((twice["tier"].clone(), twice["code"].clone(), twice["message"].clone()), (once["tier"].clone(), once["code"].clone(), once["message"].clone()));
    assert_eq!(all_events(&fx), events, "switch-failed appends no Event");

    fx.cpd(&["session", "switched", "--session", LEAD]).ok();
    assert_eq!(switch(&fx, LEAD), Value::Null);
    fx.cpd(&["session", "switched", "--session", LEAD]).ok();
    assert_eq!(switch(&fx, LEAD), Value::Null);
    assert_eq!(all_events(&fx), events, "switched appends no Event");
}

/// A Worker is `not_a_lead` (exit 3), an unknown id `session_not_found` (exit 4); neither writes anything.
#[test]
fn switch_commands_refuse_a_worker_and_an_unknown_session() {
    let fx = Fx::new();
    fixture(&fx.env);
    let events = all_events(&fx);
    for (id, code, exit) in [(WORKER, "not_a_lead", 3), ("no-such-session", "session_not_found", 4)] {
        refused(&fx.cpd(&["session", "switch-failed", "--session", id, "--code", "model_unavailable", "--message", "x"]), code, exit);
        refused(&fx.cpd(&["session", "switched", "--session", id]), code, exit);
    }
    assert_eq!(switch(&fx, WORKER), Value::Null, "nothing was written");
    assert_eq!(all_events(&fx), events);
}

/// `--code` must match `^[a-z][a-z0-9_]{0,63}$` and `--message` is 1-2000 characters: `usage`, exit 2, nothing written.
#[test]
fn switch_commands_refuse_a_bad_code_or_message() {
    let fx = Fx::new();
    fixture(&fx.env);
    let run = |code: &str, message: &str| fx.cpd(&["session", "switch-failed", "--session", LEAD, "--code", code, "--message", message]);
    let long_code = format!("a{}", "b".repeat(64));
    for code in ["", "Model", "1abc", "a-b", "a b", long_code.as_str()] {
        refused(&run(code, "m"), "usage", 2);
    }
    refused(&run("model_unavailable", ""), "usage", 2);
    refused(&run("model_unavailable", &"x".repeat(2001)), "usage", 2);
    assert_eq!(switch(&fx, LEAD), Value::Null, "nothing was written");

    // The limits themselves are accepted.
    let ok_code = format!("a{}", "b".repeat(63));
    assert!(run(&ok_code, &"x".repeat(2000)).exit == 0, "63+1 character code and a 2000 character message are valid");
}

/// A retier that moves the Session clears the recorded failure (a new target is a new attempt); one that moves nothing leaves it.
#[test]
fn retier_that_moves_a_session_clears_its_failed_switch() {
    let fx = Fx::new();
    fixture(&fx.env);
    fx.cpd(&["session", "retier", LEAD]).ok();
    failed(&fx, LEAD);
    assert_ne!(switch(&fx, LEAD), Value::Null);

    // lead1 is on its target: nothing moves.
    let again = fx.cpd(&["session", "retier", LEAD]).ok();
    assert_eq!(again["retiered"], json!([]));
    assert_ne!(switch(&fx, LEAD), Value::Null, "a retier that moves nothing leaves the failure");

    // A new target moves it.
    fx.env.write_tmux_config("lead_tier = \"O1\"\nmanager_tier = \"O4\"\n[timeouts]\nfork_quiet_seconds = 1\n");
    let moved = fx.cpd(&["session", "retier", LEAD]).ok();
    assert_eq!(moved["retiered"].as_array().map(Vec::len), Some(1), "{moved}");
    assert_eq!(switch(&fx, LEAD), Value::Null, "a moved Session starts a new attempt");
}

/// A store from before the migration that adds `switch_failed` keeps every session row and has the column NULL.
#[test]
fn migration_adds_switch_failed_as_null() {
    let list = named_migrations();
    let index = list.iter().position(|(_, _, sql)| sql.contains("switch_failed")).expect("a migration adds switch_failed");
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let conn = prefix_database(index, &home).unwrap();
    conn.execute_batch(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/', 'O3', 'idle', 't', 't'),
           ('l-ops', 'lead', 'ctx', 'ops', 'lead', '/', 'O3', 'idle', 't', 't');",
    )
    .unwrap();
    conn.execute_batch(list[index].2).unwrap();
    let kept: i64 = conn.query_row("SELECT count(*) FROM sessions WHERE id IN ('l-pay', 'l-ops') AND switch_failed IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, 2);
}
