//! `cpd job addendum` (contract §3.4 "Addenda", §4.4, §5.2; commands.md §5.7, §5.10): the Lead's append-only note to a Job.
//! Driven through `cpd` as a process of the flow harness; Plans, Attempts and Worker Sessions are fixtures.

use serde_json::{Value, json};

use super::job::{store, count, engine, events, fixture, job, job_add, job_row, lock, plan, setup};
use crate::common::fx::Fx;
use crate::common::{Run, TestEnv, refused};
use cpd_core::ids::PlanId;
use cpd_core::plan::{jobs, locked_fingerprint, plan_fingerprint};

/// A locked Plan `pl` with the queued Jobs `keys`.
fn locked(fx: &Fx, keys: &[&str]) {
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    for key in keys {
        job_add(fx, "pl", key, "S2", &[]).ok();
    }
    lock(&fx.env, "pl");
}

/// The Worker Session `w-<key>` of Attempt `<key>-a1`, the Attempt's `worker_session`.
fn worker(env: &TestEnv, key: &str) {
    fixture(
        env,
        &format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
             VALUES ('w-{key}', 'worker', 'ctx', 'pl', '{key}-a1', '{key}-a1', '/w', 'S2', 'working', 't', 't');
             UPDATE attempts SET worker_session = 'w-{key}' WHERE plan = 'pl' AND id = '{key}-a1';"
        ),
    );
}

/// Job `key` running with its Attempt `<key>-a1` running and its Worker Session.
fn running(fx: &Fx, key: &str) {
    engine(&fx.env, "pl", key, "running");
    worker(&fx.env, key);
}

fn addendum(fx: &Fx, key: &str, text: &str) -> Run {
    fx.cpd(&["job", "addendum", key, text, "--plan", "pl"])
}

/// The `txns`, `events` and `job_addenda` counts.
fn writes(env: &TestEnv) -> (i64, i64, i64) {
    (count(env, "txns"), count(env, "events"), count(env, "job_addenda"))
}

#[test]
fn an_addendum_to_a_running_attempt_is_stored_and_addressed_to_its_worker_session() {
    let fx = Fx::new();
    locked(&fx, &["a"]);
    running(&fx, "a");

    let out = addendum(&fx, "a", "Use the v2 endpoint.").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": {"key": "a", "status": "running", "kind": "work"}, "addendum": {"number": 1, "attempt": "a-a1", "delivered": true}})
    );

    assert_eq!(count(&fx.env, "job_addenda"), 1);
    let row: (i64, String, String, String, Option<String>) = fx.env.read(|c| {
        c.query_row("SELECT number, attempt, session, text, created_at FROM job_addenda WHERE plan = 'pl' AND job = 'a'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
    });
    assert_eq!((row.0, row.1.as_str(), row.2.as_str(), row.3.as_str()), (1, "a-a1", "w-a", "Use the v2 endpoint."));
    assert!(row.4.is_some_and(|t| !t.is_empty()), "created_at is set");

    let all = events(&fx, "job-addendum");
    assert_eq!(all.len(), 1, "{all:?}");
    let e = &all[0];
    assert_eq!((&e["job"], &e["attempt"], &e["session"]), (&json!("a"), &json!("a-a1"), &json!("w-a")));
    assert_eq!(e["data"], json!({"text": "Use the v2 endpoint."}));
    let text = e["text"].as_str().unwrap();
    assert!(text.starts_with("Addendum 1 from the Lead on Job a (Attempt a-a1)."), "{text}");
    assert!(text.contains("replaces nothing"), "{text}");
    assert!(text.ends_with("Use the v2 endpoint."), "{text}");
}

#[test]
fn an_addendum_changes_no_spec_column_and_no_fingerprint() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    lock(&fx.env, "pl");
    // A draft Job of a later batch, appended after the lock.
    job_add(&fx, "pl", "later", "S2", &["a"]).ok();
    let id = PlanId::parse("pl").unwrap();
    let plan_fp = || store(&fx.env).read(|c| plan_fingerprint(c, &id)).unwrap();
    let before = (job(&fx, &["show", "a"], "pl").ok()["job"].clone(), plan_fp(), count(&fx.env, "attempts"));

    addendum(&fx, "a", "A pointer.").ok();

    let after = (job(&fx, &["show", "a"], "pl").ok()["job"].clone(), plan_fp(), count(&fx.env, "attempts"));
    assert_eq!(before, after);
    let all = store(&fx.env).read(|c| jobs::load(c, &id)).unwrap();
    let a = all.iter().find(|j| j.key == "a").unwrap();
    assert_eq!(job_row(&fx.env, "pl", "a", "fingerprint"), json!(locked_fingerprint(a)));
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), json!("queued"));
}

#[test]
fn an_addendum_takes_its_text_from_an_argument_or_a_file_and_refuses_both_or_neither() {
    let fx = Fx::new();
    locked(&fx, &["a"]);
    running(&fx, "a");

    let text = "Café, naïve — ünïcode.\n";
    let path = fx.env.root().join("note.txt");
    std::fs::write(&path, text).unwrap();
    let path = path.to_str().unwrap();
    fx.cpd(&["job", "addendum", "a", "--file", path, "--plan", "pl"]).ok();
    let stored: String = fx.env.query("SELECT text FROM job_addenda WHERE number = 1");
    assert_eq!(stored, text);

    let before = writes(&fx.env);
    refused(&fx.cpd(&["job", "addendum", "a", "Text.", "--file", path, "--plan", "pl"]), "usage", 2);
    refused(&fx.cpd(&["job", "addendum", "a", "--plan", "pl"]), "usage", 2);
    refused(&addendum(&fx, "a", ""), "usage", 2);
    refused(&addendum(&fx, "a", &"x".repeat(20001)), "usage", 2);
    refused(&addendum(&fx, "a", "has\0nul"), "usage", 2);
    refused(&fx.cpd(&["job", "addendum", "a", "--file", "/no/such/file", "--plan", "pl"]), "file_unreadable", 2);
    assert_eq!(writes(&fx.env), before);
}

#[test]
fn an_addendum_to_a_queued_or_failed_job_is_stored_and_addressed_to_nobody() {
    let fx = Fx::new();
    locked(&fx, &["q", "f"]);
    engine(&fx.env, "pl", "f", "failed");
    for key in ["q", "f"] {
        let status = if key == "q" { "queued" } else { "failed" };
        let out = addendum(&fx, key, "Note.").ok();
        assert_eq!(
            out,
            json!({"ok": true, "job": {"key": key, "status": status, "kind": "work"}, "addendum": {"number": 1, "attempt": null, "delivered": false}})
        );
        let row: (Option<String>, Option<String>) = fx.env.read(|c| {
            c.query_row("SELECT attempt, session FROM job_addenda WHERE job = ?1 AND number = 1", [key], |r| Ok((r.get(0)?, r.get(1)?)))
        });
        assert_eq!(row, (None, None));
        let all: Vec<Value> = events(&fx, "job-addendum").into_iter().filter(|e| e["job"] == key).collect();
        assert_eq!(all.len(), 1, "{all:?}");
        assert!(all[0].get("attempt").is_none_or(Value::is_null) && all[0].get("session").is_none_or(Value::is_null), "{:?}", all[0]);
        let text = all[0]["text"].as_str().unwrap();
        assert!(text.starts_with(&format!("Addendum 1 added to Job {key}: no Attempt is running")), "{text}");
    }
}

#[test]
fn addenda_of_a_job_are_numbered_from_one() {
    let fx = Fx::new();
    locked(&fx, &["a", "b"]);
    let numbers: Vec<i64> = ["one", "two", "three"].iter().map(|t| addendum(&fx, "a", t).ok()["addendum"]["number"].as_i64().unwrap()).collect();
    assert_eq!(numbers, [1, 2, 3]);
    assert_eq!(addendum(&fx, "b", "other").ok()["addendum"]["number"], 1);
}

#[test]
fn an_addendum_is_the_leads_and_not_a_workers() {
    let fx = Fx::new();
    locked(&fx, &["a"]);
    running(&fx, "a");
    let args = ["job", "addendum", "a", "Note.", "--plan", "pl"];
    let before = writes(&fx.env);
    refused(&fx.cpd_with(fx.env.root(), &args, &[("CPD_ATTEMPT", "a-a1")]), "not_a_lead", 3);
    assert_eq!(writes(&fx.env), before);
    fx.cpd_with(fx.env.root(), &args, &[]).ok();
}

#[test]
fn an_addendum_refuses_a_draft_merged_or_cancelled_job_and_an_attempt_that_is_not_running_and_writes_nothing() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    for key in ["gone", "m", "s", "g", "e"] {
        job_add(&fx, "pl", key, "S2", &[]).ok();
    }
    lock(&fx.env, "pl");
    job_add(&fx, "pl", "d", "S2", &[]).ok(); // a draft appended after the lock
    job(&fx, &["cancel", "gone", "--reason", "Not needed."], "pl").ok();
    engine(&fx.env, "pl", "m", "merged");
    // A running Job whose latest Attempt is `starting`, `merging` or `ended`, inserted in that status.
    for (key, status) in [("s", "starting"), ("g", "merging"), ("e", "ended")] {
        let (ended, ended_values) = match status {
            "ended" => (", outcome, cause, ended_at", ", 'failed', 'timed_out', 't'"),
            "merging" => (", submitted_sha", ", 'abc123'"),
            _ => ("", ""),
        };
        fixture(
            &fx.env,
            &format!(
                "UPDATE jobs SET status = 'running' WHERE plan = 'pl' AND key = '{key}';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at{ended})
                 VALUES ('pl', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', 'cpd/pl--{key}-a1', '/w', 't'{ended_values});"
            ),
        );
    }

    let before = writes(&fx.env);
    for key in ["d", "m", "gone"] {
        refused(&addendum(&fx, key, "Note."), "illegal_transition", 3);
    }
    for key in ["s", "g", "e"] {
        refused(&addendum(&fx, key, "Note."), "attempt_not_running", 3);
    }
    refused(&addendum(&fx, "ghost", "Note."), "job_not_found", 4);
    refused(&fx.cpd(&["job", "addendum", "d", "Note.", "--plan", "nope"]), "plan_not_found", 4);
    assert_eq!(writes(&fx.env), before);
}

#[test]
fn job_show_lists_the_addenda_oldest_first() {
    let fx = Fx::new();
    locked(&fx, &["a"]);
    assert_eq!(job(&fx, &["show", "a"], "pl").ok()["addenda"], json!([]));
    addendum(&fx, "a", "First.").ok();
    running(&fx, "a");
    addendum(&fx, "a", "Second.").ok();

    let shown = job(&fx, &["show", "a"], "pl").ok();
    let list = shown["addenda"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{shown}");
    assert_eq!((&list[0]["number"], &list[0]["attempt"], &list[0]["session"], &list[0]["text"]), (&json!(1), &Value::Null, &Value::Null, &json!("First.")));
    assert_eq!((&list[1]["number"], &list[1]["attempt"], &list[1]["session"], &list[1]["text"]), (&json!(2), &json!("a-a1"), &json!("w-a"), &json!("Second.")));
    assert!(list.iter().all(|a| a["created_at"].as_str().is_some_and(|t| !t.is_empty())), "{list:?}");
}
