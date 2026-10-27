//! Events (contract §5.1, §5.2, §5.4; commands.md §11): the append API, and `events list` and `events follow` in the
//! test process (`Fx`, and `common/follower.rs` for `follow`, which a test drives on virtual time): no process starts.

use crate::common;

use std::fs;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::home::{HasCpdHome, Home};
use crate::scripted::ScriptedClock;
use common::fx::Fx;
use common::{Run, TestEnv};
#[path = "../common/follower.rs"]
mod follower;
use follower::{Follower, follow_once, follow_once_on};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{Kind, NewEvent, Selection, append};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, ContextId, JobKey, PlanId, SessionId};
use cpd_core::store::{Store, WriteTxn};
use rusqlite::Connection;
use serde_json::{Value, json};

fn home(env: &impl HasCpdHome) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn open(env: &impl HasCpdHome) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

/// The machine's `config.toml`, with a short Events poll and the curation floor at 1 (the default is 25, hs-d592).
const CONFIG: &str = "version = 1\nmachine = \"m1\"\ncontext_curation_min_records = 1\n[events]\npoll_ms = 20\n";

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn ctx() -> ContextId {
    ContextId::parse("ctx").unwrap()
}
fn plan(id: &str) -> PlanId {
    PlanId::parse(id).unwrap()
}
fn job() -> JobKey {
    JobKey::parse("api").unwrap()
}
fn attempt() -> AttemptId {
    AttemptId::parse("api-a1").unwrap()
}
fn session(id: &str) -> SessionId {
    SessionId::parse(id).unwrap()
}

/// Contexts `ctx` and `ctx2`; Plans `pay` (Lead `lead1`, Job `api` with Attempt
/// `api-a1`, Worker `worker1`) and `other` (Lead `lead2`) in `ctx`; and a
/// the Manager `side` of `ctx` (no Plan).
fn fixture(store: &mut Store) {
    store
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm'), ('ctx2', 'Ctx2', 't', 'm');
                     INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
                       ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead', '/', 'O3', 'idle', 't', 't'),
                       ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead', '/', 'O3', 'idle', 't', 't'),
                       ('side', 'manager', 'ctx', NULL, NULL, 'Side', '/', 'O5', 'idle', 't', 't'),
                       ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'Worker', '/', 'S2', 'working', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
                       ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't'),
                       ('other', 'ctx', 'Other', 'grounding', 'lead2', 't');
                     INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES
                       ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {id}, 't'),
                       ('other', 'r', '/r', 'main', 'waiting', 'cpd/other', {id}, 't');
                     INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{fp}', {id}, 't');
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 't');",
                    fp = "a".repeat(64),
                    id = tx.id()
                ),
            )
        })
        .unwrap();
}

/// Eleven Events, ids 1 to 11.
fn seed(store: &mut Store) {
    use Kind::*;
    let events = [
        NewEvent::new(ContextCreated, "Context ctx created.", json!({"title": "Ctx", "jira": null, "created_on": "m"}))
            .context(ctx()),
        NewEvent::new(
            PlanCreated,
            "Plan pay created.",
            json!({"title": "Pay", "jira": null, "lead_session": "lead1", "forked_from": null, "cwd": "/r"}),
        )
        .plan(plan("pay")),
        NewEvent::new(
            SessionRegistered,
            "Session lead1 registered.",
            json!({"role": "lead", "name": "Lead", "forked_from": null, "tier": "O3"}),
        )
        .plan(plan("pay"))
        .session(session("lead1")),
        NewEvent::new(JobMerged, "Job api merged. Next: cpd plan show", json!({"sha": "c0ffee1", "tier": "S2", "kind": "work"}))
            .plan(plan("pay"))
            .job(job())
            .attempt(attempt()),
        NewEvent::new(AttemptNudge, "Submit your work.", json!({"prompt": "Run cpd attempt submit"}))
            .plan(plan("pay"))
            .job(job())
            .attempt(attempt())
            .session(session("worker1")),
        NewEvent::new(
            ContextRecord,
            "A decision was recorded.",
            json!({"record": "m1-d1", "kind": "decision", "machine": "m1", "seq": 1}),
        )
        .context(ctx()),
        NewEvent::new(SessionRenamed, "Session side renamed.", json!({"name": "Side"})).session(session("side")),
        NewEvent::new(
            PlanCreated,
            "Plan other created.",
            json!({"title": "Other", "jira": null, "lead_session": "lead2", "forked_from": null, "cwd": "/r"}),
        )
        .plan(plan("other")),
        NewEvent::new(ContextCreated, "Context ctx2 created.", json!({"title": "Ctx2", "jira": null, "created_on": "m"}))
            .context(ContextId::parse("ctx2").unwrap()),
        NewEvent::new(WatchStarted, "The engine started.", json!({"pid": 1})),
        NewEvent::new(
            Error,
            "watch failed.",
            json!({"source": "watch", "code": "store_failed", "message": "x", "details": {}}),
        ),
    ];
    store
        .write("seed", |tx| {
            for event in events {
                append(tx, event)?;
            }
            Ok(())
        })
        .unwrap();
}

/// A configured home with the fixture and the eleven Events.
fn seeded() -> Fx {
    let fx = Fx::new();
    fx.env.write_config(CONFIG);
    seed_store(&fx.env);
    fx
}

/// The fixture and the eleven Events, in the store of `env`.
fn seed_store(env: &impl HasCpdHome) {
    let mut store = open(env);
    fixture(&mut store);
    seed(&mut store);
}

/// What `events list` prints as `events` (`Selection::fetch`, which `list` runs), for the tests that call the library.
fn fetched(env: &impl HasCpdHome, kinds: Vec<Kind>, since: i64, limit: i64) -> Vec<Value> {
    let mut store = open(env);
    let events = store.read(|conn| Selection::resolve(conn, None, None, None, kinds)?.fetch(conn, since, None, limit)).unwrap();
    events.iter().map(|e| serde_json::to_value(e).unwrap()).collect()
}

fn ids(events: &Value) -> Vec<i64> {
    events.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect()
}

fn list(fx: &Fx, args: &[&str]) -> Value {
    let mut argv = vec!["events", "list"];
    argv.extend_from_slice(args);
    fx.cpd(&argv).ok()
}

/// What `events follow --until-idle` printed: one JSON value per line.
fn follow_lines(run: &Run) -> Vec<Value> {
    assert_eq!(run.exit, 0, "{run:?}");
    run.stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn follow_until_idle(fx: &Fx, args: &[&str]) -> Vec<Value> {
    follow_lines(&follow_once(fx, &[&["--until-idle"], args].concat()))
}

fn refused(fx: &Fx, args: &[&str], code: &str, exit: i32) -> Value {
    common::refused(&fx.cpd(args), code, exit)
}

/// `refused` for `cpd events follow <words>`, run by [`follow_once`].
fn refused_follow(fx: &Fx, words: &[&str], code: &str, exit: i32) -> Value {
    common::refused(&follow_once(fx, words), code, exit)
}

/// `run` of a command on a home with no `config.toml`: the one of `bare`, a second env, with this `fx`'s ports.
fn in_bare(fx: &Fx, bare: &TestEnv, run: impl FnOnce() -> Run) -> Run {
    fx.with_vars(&[("CPD_HOME", bare.cpd_home().to_str().unwrap())], run)
}

// ------------------------------------------------------------------ append

/// §5.2, written out independently of `Kind::spec`:
/// `(kind, columns always set, columns that may be set, data fields, optional data fields)`;
/// columns are `c`ontext, `p`lan, `j`ob, `a`ttempt, `s`ession.
const SPEC: &[(&str, &str, &str, &[&str], &[&str])] = &[
    ("context-created", "c", "", &["title", "jira", "created_on"], &[]),
    ("context-record", "c", "", &["record", "kind", "machine", "seq"], &[]),
    ("context-grounded", "cs", "", &["seq", "forked_from"], &[]),
    ("plan-created", "cp", "", &["title", "jira", "lead_session", "forked_from", "cwd"], &[]),
    ("plan-phase", "cp", "", &["from", "to"], &["jobs", "pr"]),
    ("plan-review", "cp", "", &["review", "fingerprint", "status"], &["feedback"]),
    ("plan-outlined", "cp", "", &["replaced"], &[]),
    ("plan-done", "cp", "", &["prs"], &[]),
    ("job-added", "cpj", "", &["pr", "kind", "status", "appended", "tier", "needs", "review", "urgent"], &["by", "reason"]),
    ("job-edited", "cpj", "", &["fields"], &[]),
    ("job-merged", "cpja", "", &["sha", "tier", "kind"], &["cancel_too_late", "pr", "number", "url"]),
    ("job-failed", "cpj", "a", &["cause", "detail", "tier", "attempts"], &[]),
    ("job-retried", "cpj", "", &["tier"], &[]),
    ("job-cancel-requested", "cpja", "", &["reason"], &[]),
    ("job-cancelled", "cpj", "a", &["reason"], &[]),
    ("attempt-started", "cpja", "", &["number", "tier", "origin"], &[]),
    ("attempt-status", "cpja", "", &["from", "to"], &[]),
    ("attempt-retry", "cpja", "", &["outcome", "cause", "next_tier"], &["feedback"]),
    ("attempt-nudge", "cpjas", "", &["prompt"], &[]),
    ("all-merged", "cp", "", &["pr", "merged", "cancelled"], &[]),
    ("pr-feedback", "cp", "", &["pr", "number", "url", "state", "checks", "review_decision", "mergeable", "merge_state", "conflict", "reasons"], &[]),
    ("pr-added", "cp", "", &["pr", "repo", "base", "needs", "appended"], &[]),
    ("pr-edited", "cp", "", &["pr", "fields"], &[]),
    ("pr-described", "cp", "", &["pr", "fields"], &[]),
    ("pr-cancelled", "cp", "", &["pr"], &["superseded_by"]),
    ("pr-started", "cp", "", &["pr", "repo", "base", "lead_branch", "lead_worktree", "base_sha", "reused", "pushed", "by"], &[]),
    ("pr-base-changed", "cp", "", &["pr", "from", "to", "from_pr", "to_pr", "by"], &["number", "url"]),
    ("pr-opened", "cp", "", &["pr", "number", "url"], &[]),
    ("pr-merged", "cp", "", &["pr", "number", "url", "merged_at"], &[]),
    ("escalation", "cpja", "", &["escalation", "question"], &[]),
    ("escalation-answered", "cpja", "", &["escalation"], &[]),
    ("escalation-expired", "cpja", "", &["escalation", "reason"], &[]),
    ("session-registered", "cs", "p", &["role", "name", "forked_from", "tier"], &[]),
    ("session-state", "cs", "p", &["from", "to", "label"], &[]),
    ("session-moved", "cs", "p", &["pane", "window", "place"], &[]),
    ("session-renamed", "cs", "p", &["name"], &[]),
    ("session-titled", "cs", "p", &["title"], &[]),
    ("session-retiered", "cs", "p", &["from", "to"], &[]),
    ("mcp-attached", "cs", "p", &["name", "server"], &[]),
    ("session-asleep", "cs", "p", &[], &[]),
    ("session-resumed", "cs", "p", &[], &[]),
    ("plan-deleted", "cp", "", &["jobs", "attempts", "sessions", "worktrees", "disk_freed_mb"], &[]),
    ("context-deleted", "c", "", &["plans", "jobs", "attempts", "sessions", "worktrees", "disk_freed_mb"], &[]),
    ("records-retired", "c", "", &["context", "approved", "rejected"], &[]),
    ("docs-decided", "c", "", &["context", "sent", "dismissed"], &[]),
    ("context-curated", "c", "", &["context", "curated", "retire", "docs"], &[]),
    ("context-renamed", "c", "", &["title"], &[]),
    ("context-owner", "c", "", &["owner"], &[]),
    ("plan-renamed", "cp", "", &["title"], &[]),
    ("context-archived", "c", "", &["archived"], &[]),
    ("plan-archived", "cp", "", &["archived"], &[]),
    ("session-archived", "cs", "p", &["archived"], &[]),
    ("session-message", "cs", "p", &["from", "text"], &["unblocks", "wake"]),
    ("inbox-opened", "", "cpjs", &["item", "kind", "urgency", "label"], &[]),
    ("inbox-closed", "", "cpjs", &["item", "kind"], &[]),
    ("inbox-snoozed", "", "cpjs", &["item", "until"], &[]),
    ("inbox-dismissed", "", "cpjs", &["item"], &[]),
    ("inbox-claimed", "", "cpjs", &["item"], &[]),
    ("watch-started", "", "", &["pid"], &[]),
    ("watch-stopped", "", "", &["pid"], &[]),
    ("error", "", "cpjas", &["source", "code", "message", "details"], &[]),
    ("review-opened", "cs", "p", &["pn_id", "kind", "subject", "port"], &[]),
    ("review-closed", "cs", "p", &["pn_id", "ended"], &[]),
    ("incident-started", "", "", &["by", "reason", "started_at", "plans"], &["until_pr"]),
    ("incident-plans", "", "", &["by", "plans"], &["until_pr"]),
    ("incident-ended", "", "", &["by", "ended_at", "cause"], &[]),
];

fn event(kind: Kind, columns: &str, data: &[&str]) -> NewEvent {
    let data: serde_json::Map<String, Value> = data.iter().map(|k| (k.to_string(), json!(1))).collect();
    let mut e = NewEvent::new(kind, "text", Value::Object(data));
    for c in columns.chars() {
        e = match c {
            'c' => e.context(ctx()),
            'p' => e.plan(plan("pay")),
            'j' => e.job(job()),
            'a' => e.attempt(attempt()),
            's' => e.session(session("worker1")),
            _ => unreachable!(),
        };
    }
    e
}

fn append_one(store: &mut Store, e: NewEvent) -> Result<i64, CpdError> {
    store.write("test", |tx| append(tx, e))
}

fn refused_append(store: &mut Store, e: NewEvent, why: &str) {
    let err = append_one(store, e).expect_err(why);
    assert_eq!(err.code(), ErrorCode::Internal, "{why}: {err:?}");
}

#[test]
fn the_kinds_are_exactly_those_of_5_2_and_of_event_kinds() {
    let names: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, SPEC.iter().map(|s| s.0).collect::<Vec<_>>());

    // The rows of `event_kinds` (§4.5: a kind exists when its row does) are the same kinds.
    let env = Home::new();
    drop(open(&env));
    let conn = Connection::open(home(&env).db()).unwrap();
    let mut stmt = conn.prepare("SELECT name FROM event_kinds ORDER BY name").unwrap();
    let in_table: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    let mut expected = names.clone();
    expected.sort();
    assert_eq!(in_table, expected);
}

#[test]
fn every_kind_is_appended_with_its_fields() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    for (name, always, maybe, data, optional) in SPEC {
        let kind = Kind::from_str(name).unwrap();
        let id = append_one(&mut store, event(kind, always, data)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(id > 0);
        // With everything that may be set, and every optional field.
        let all_columns = format!("{always}{maybe}");
        let all_data: Vec<&str> = data.iter().chain(optional.iter()).copied().collect();
        append_one(&mut store, event(kind, &all_columns, &all_data)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
    let events = fetched(&env, vec![], 0, 5000);
    assert_eq!(events.len(), SPEC.len() * 2);
}

#[test]
fn a_missing_field_is_refused() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    for (name, always, _, data, _) in SPEC {
        let kind = Kind::from_str(name).unwrap();
        for missing in *data {
            let rest: Vec<&str> = data.iter().filter(|k| *k != missing).copied().collect();
            let err = append_one(&mut store, event(kind, always, &rest)).expect_err(&format!("{name} without {missing}"));
            assert_eq!(err.code(), ErrorCode::Internal);
            assert_eq!(err.details()["field"], *missing, "{name}");
        }
    }
}

#[test]
fn an_unknown_field_or_a_data_that_is_not_an_object_is_refused() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    for (name, always, _, data, _) in SPEC {
        let kind = Kind::from_str(name).unwrap();
        let more: Vec<&str> = data.iter().copied().chain(["bogus"]).collect();
        refused_append(&mut store, event(kind, always, &more), &format!("{name} with an unknown field"));
        let mut array = NewEvent::new(kind, "text", json!([1]));
        for c in always.chars() {
            array = match c {
                'c' => array.context(ctx()),
                'p' => array.plan(plan("pay")),
                'j' => array.job(job()),
                'a' => array.attempt(attempt()),
                's' => array.session(session("worker1")),
                _ => unreachable!(),
            };
        }
        refused_append(&mut store, array, &format!("{name} with array data"));
    }
}

#[test]
fn a_column_the_kind_does_not_set_is_refused_and_one_it_must_set_is_required() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    for (name, always, maybe, data, _) in SPEC {
        let kind = Kind::from_str(name).unwrap();
        for c in "cpjas".chars() {
            if always.contains(c) || maybe.contains(c) {
                continue;
            }
            refused_append(
                &mut store,
                event(kind, &format!("{always}{c}"), data),
                &format!("{name} must not set {c}"),
            );
        }
        for c in always.chars().filter(|c| *c != 'c') {
            let without: String = always.chars().filter(|x| *x != c).collect();
            refused_append(&mut store, event(kind, &without, data), &format!("{name} without {c}"));
        }
        if always.contains('c') && !always.contains('p') && !always.contains('s') {
            refused_append(&mut store, event(kind, "", data), &format!("{name} without a Context"));
        }
    }
    // A Job needs its Plan; an Attempt needs its Job (of the kinds that allow them).
    refused_append(
        &mut store,
        NewEvent::new(Kind::Error, "x", json!({"source": 1, "code": 1, "message": 1, "details": 1})).job(job()),
        "a Job without a Plan",
    );
    refused_append(
        &mut store,
        NewEvent::new(Kind::Error, "x", json!({"source": 1, "code": 1, "message": 1, "details": 1}))
            .plan(plan("pay"))
            .attempt(attempt()),
        "an Attempt without a Job",
    );
    refused_append(&mut store, NewEvent::new(Kind::WatchStarted, "", json!({"pid": 1})), "empty text");
}

#[test]
fn an_unknown_kind_is_refused() {
    let err = Kind::from_str("job-exploded").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Usage);
    let fx = seeded();
    refused(&fx, &["events", "list", "--kind", "job-exploded"], "usage", 2);
    refused_follow(&fx, &["--kind", "job-exploded"], "usage", 2);
}

#[test]
fn the_new_archive_and_rename_kinds_are_accepted_by_list_and_follow() {
    let env = Home::new();
    seed_store(&env);
    for kind in ["context-renamed", "plan-renamed", "context-archived", "plan-archived", "session-archived"] {
        // `--kind` is parsed with `Kind::from_str`; `list` then selects by it.
        let parsed = Kind::from_str(kind).unwrap_or_else(|e| panic!("{kind}: {e:?}"));
        assert_eq!(ids(&Value::Array(fetched(&env, vec![parsed], 0, 500))), Vec::<i64>::new(), "{kind}");
    }
}

#[test]
fn append_takes_the_context_of_the_plan_else_of_the_session() {
    let env = Home::new();
    seed_store(&env);
    let events = fetched(&env, vec![], 0, 500);
    let by_id = |id: i64| events.iter().find(|e| e["id"] == id).unwrap().clone();
    assert_eq!(by_id(2)["context"], "ctx"); // plan-created: from the Plan
    assert_eq!(by_id(7)["context"], "ctx"); // session-renamed: from the Session
    assert_eq!(by_id(7)["plan"], Value::Null);
    assert_eq!(by_id(10)["context"], Value::Null); // watch-started: none
}

#[test]
fn append_names_a_missing_plan_or_session() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    let e = NewEvent::new(Kind::JobRetried, "x", json!({"tier": 1})).plan(plan("nope")).job(job());
    assert_eq!(append_one(&mut store, e).unwrap_err().code(), ErrorCode::PlanNotFound);
    let e = NewEvent::new(Kind::SessionRenamed, "x", json!({"name": 1})).session(session("nope"));
    assert_eq!(append_one(&mut store, e).unwrap_err().code(), ErrorCode::SessionNotFound);
}

#[test]
fn an_event_belongs_to_its_transaction_and_rolls_back_with_it() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    let (txn, id) = store
        .write("append-test", |tx| {
            let id = append(tx, NewEvent::new(Kind::WatchStarted, "x", json!({"pid": 1})))?;
            Ok((tx.id(), id))
        })
        .unwrap();
    let conn = Connection::open(home(&env).db()).unwrap();
    let (e_txn, e_at, t_at): (i64, String, String) = conn
        .query_row(
            "SELECT e.txn, e.at, t.at FROM events e JOIN txns t ON t.id = e.txn WHERE e.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((e_txn, e_at.as_str()), (txn, t_at.as_str()));
    assert!(e_at.ends_with('Z') && e_at.len() == 24, "{e_at}");

    let before: i64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
    let failed: Result<(), CpdError> = store.write("rollback-test", |tx| {
        append(tx, NewEvent::new(Kind::WatchStopped, "x", json!({"pid": 1})))?;
        Err(CpdError::new(ErrorCode::CasLost, "No."))
    });
    assert_eq!(failed.unwrap_err().code(), ErrorCode::CasLost);
    let after: i64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
    assert_eq!(before, after);
}

// ------------------------------------------------------------- events list

#[test]
fn list_prints_events_in_the_shape_of_11_1() {
    let fx = seeded();
    let out = list(&fx, &[]);
    assert_eq!(ids(&out["events"]), (1..=11).collect::<Vec<_>>());
    assert_eq!(out["next"], 11);
    let merged = &out["events"][3];
    let keys: Vec<&str> = merged.as_object().unwrap().keys().map(String::as_str).collect();
    let mut want = ["id", "at", "kind", "context", "plan", "job", "attempt", "session", "text", "data"];
    want.sort();
    assert_eq!(keys, want); // serde_json sorts keys
    assert_eq!(merged["kind"], "job-merged");
    assert_eq!(merged["context"], "ctx");
    assert_eq!(merged["plan"], "pay");
    assert_eq!(merged["job"], "api");
    assert_eq!(merged["attempt"], "api-a1");
    assert_eq!(merged["session"], Value::Null);
    assert_eq!(merged["text"], "Job api merged. Next: cpd plan show");
    assert_eq!(merged["data"], json!({"sha": "c0ffee1", "tier": "S2", "kind": "work"}));
    assert!(merged["at"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn list_since_is_exclusive_limit_caps_and_next_resumes() {
    let fx = seeded();
    let first = list(&fx, &["--limit", "4"]);
    assert_eq!((ids(&first["events"]), &first["next"]), (vec![1, 2, 3, 4], &json!(4)));
    let second = list(&fx, &["--since", "4", "--limit", "4"]);
    assert_eq!((ids(&second["events"]), &second["next"]), (vec![5, 6, 7, 8], &json!(8)));
    let rest = list(&fx, &["--since", "8"]);
    assert_eq!(ids(&rest["events"]), vec![9, 10, 11]);
    // Nothing more: next is --since; a cursor beyond the end is not an error here.
    let none = list(&fx, &["--since", "11"]);
    assert_eq!((ids(&none["events"]), &none["next"]), (vec![], &json!(11)));
    let beyond = list(&fx, &["--since", "999"]);
    assert_eq!((ids(&beyond["events"]), &beyond["next"]), (vec![], &json!(999)));
}

#[test]
fn list_selects_by_plan_context_session_and_kind() {
    let fx = seeded();
    let sel = |args: &[&str]| ids(&list(&fx, args)["events"]);
    assert_eq!(sel(&["--plan", "pay"]), vec![2, 3, 4, 5]);
    assert_eq!(sel(&["--context", "ctx"]), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    // The Lead of `pay`: its Plan, its Context's Events about no Plan, itself.
    assert_eq!(sel(&["--session", "lead1"]), vec![1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(sel(&["--session", "lead2"]), vec![1, 6, 7, 8]);
    // Any other Session: only Events addressed to itself.
    assert_eq!(sel(&["--session", "worker1"]), vec![5]);
    assert_eq!(sel(&["--session", "side"]), vec![7]);
    assert_eq!(sel(&["--kind", "plan-created"]), vec![2, 8]);
    assert_eq!(sel(&["--kind", "plan-created", "--kind", "error"]), vec![2, 8, 11]);
    assert_eq!(sel(&["--plan", "pay", "--kind", "job-merged", "--since", "3"]), vec![4]);
}

#[test]
fn list_selection_flags_do_not_fall_back_to_the_environment() {
    let fx = seeded();
    let json = fx.cpd_with(fx.env.root(), &["events", "list"], &[("CPD_PLAN", "pay"), ("CPD_SESSION", "worker1")]).ok();
    assert_eq!(ids(&json["events"]), (1..=11).collect::<Vec<_>>());
}

#[test]
fn list_is_read_only_and_repeatable() {
    let fx = seeded();
    let a = fx.cpd(&["events", "list", "--plan", "pay"]);
    let b = fx.cpd(&["events", "list", "--plan", "pay"]);
    assert_eq!(a.stdout, b.stdout);
    let conn = Connection::open(home(&fx.env).db()).unwrap();
    let txns: i64 = conn.query_row("SELECT count(*) FROM txns", [], |r| r.get(0)).unwrap();
    fx.cpd(&["events", "list"]).ok();
    assert_eq!(txns, conn.query_row("SELECT count(*) FROM txns", [], |r| r.get::<_, i64>(0)).unwrap());
}

#[test]
fn list_refusals() {
    let fx = seeded();
    refused(&fx, &["events", "list", "--plan", "pay", "--context", "ctx"], "usage", 2);
    refused(&fx, &["events", "list", "--plan", "pay", "--session", "lead1"], "usage", 2);
    refused(&fx, &["events", "list", "--context", "ctx", "--session", "lead1"], "usage", 2);
    refused(&fx, &["events", "list", "--limit", "0"], "usage", 2);
    refused(&fx, &["events", "list", "--limit", "5001"], "usage", 2);
    refused(&fx, &["events", "list", "--since", "-1"], "usage", 2);
    refused(&fx, &["events", "list", "--since", "x"], "usage", 2);
    refused(&fx, &["events", "list", "--bogus"], "usage", 2);
    refused(&fx, &["events", "list", "--plan", "Bad_Id"], "invalid_plan_id", 2);
    refused(&fx, &["events", "list", "--context", "Bad_Id"], "invalid_context_id", 2);
    refused(&fx, &["events", "list", "--session", "a b"], "invalid_session_id", 2);
    refused(&fx, &["events", "list", "--plan", "nope"], "plan_not_found", 4);
    refused(&fx, &["events", "list", "--context", "nope"], "context_not_found", 4);
    refused(&fx, &["events", "list", "--session", "nope"], "session_not_found", 4);
    let bare = fx.another_env();
    common::refused(&in_bare(&fx, &bare, || fx.cpd(&["events", "list"])), "config_missing", 1);
}

#[test]
fn list_limit_5000_is_accepted() {
    let fx = seeded();
    assert_eq!(ids(&list(&fx, &["--limit", "5000"])["events"]).len(), 11);
}

// ----------------------------------------------------------- events follow

/// hs-n278: `locks/store.lock` held as a process that migrates the store does (contract §1.4: the lock, and its marker in
/// the file); released by the returned function, as the last step of that process's setup: the marker emptied, then the
/// lock let go.
fn migrating(fx: &Fx) -> impl FnOnce() + Send + 'static {
    use nix::fcntl::{Flock, FlockArg};
    use std::io::Write;
    let locks = fx.env.cpd_home().join("locks");
    fs::create_dir_all(&locks).unwrap();
    let file = fs::OpenOptions::new().create(true).append(true).open(locks.join("store.lock")).unwrap();
    let mut lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    // The wall clock, not the fake one: the store reads a marker's age from it (`store/lock.rs` `unix_ms`).
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let marker = json!({"migrating": true, "pid": std::process::id(), "started_unix_ms": now, "from": 3, "to": 20, "command": "cpd migrating-holder"});
    lock.write_all(marker.to_string().as_bytes()).unwrap();
    move || {
        let _ = lock.set_len(0);
        drop(lock);
    }
}

/// hs-n278: a follower that opens the store while another cpd migrates it waits (stderr says so once), and
/// does not fail `lock_timeout` after `store_busy_ms`.
#[test]
fn follow_waits_out_a_migration_longer_than_store_busy_ms() {
    let fx = seeded();
    fx.env.write_config(&format!("{CONFIG}[timeouts]\nstore_busy_ms = 300\n"));
    let release = migrating(&fx);
    // The migration ends 1500 ms of the open's virtual time after it began: the open outlasts store_busy_ms (300 ms)
    // while a migration holds the lock.
    let clock = Arc::new(ScriptedClock::new().at(Duration::from_millis(1500), release));
    let ports = cpd_core::ports::Ports { clock: clock.clone(), runner: fx.fakes.runner.clone(), spawn: fx.fakes.spawn.clone() };
    let run = follow_once_on(&ports, &["--until-idle", "--since", "0"]);
    assert_eq!(follow_lines(&run).len(), 11, "{run:?}");
    assert!(clock.elapsed() >= Duration::from_millis(1000), "did not wait: {:?}", clock.elapsed());
    assert_eq!(run.stderr.matches("waiting for cpd pid").count(), 1, "{run:?}");
    assert!(run.stderr.contains("3 -> 20"), "{run:?}");
}

#[test]
fn follow_until_idle_replays_since_zero_one_event_per_line() {
    let fx = seeded();
    let lines = follow_until_idle(&fx, &["--since", "0"]);
    assert_eq!(lines, list(&fx, &["--since", "0"])["events"].as_array().unwrap().clone());
    let run = follow_once(&fx, &["--until-idle", "--since", "0"]);
    assert_eq!(run.stdout.lines().count(), 11);
    assert!(run.stdout.ends_with('\n'));
    assert_eq!(run.stderr, "");
}

#[test]
fn follow_without_since_starts_after_the_highest_id() {
    let fx = seeded();
    assert!(follow_until_idle(&fx, &[]).is_empty());
}

#[test]
fn follow_since_is_exclusive_and_the_same_since_replays_the_same_lines() {
    let fx = seeded();
    let a = follow_until_idle(&fx, &["--since", "8"]);
    assert_eq!(ids(&Value::Array(a.clone())), vec![9, 10, 11]);
    assert_eq!(a, follow_until_idle(&fx, &["--since", "8"]));
    assert!(follow_until_idle(&fx, &["--since", "11"]).is_empty());
}

#[test]
fn follow_selects_like_list() {
    let fx = seeded();
    let sel = |args: &[&str]| ids(&Value::Array(follow_until_idle(&fx, &[&["--since", "0"], args].concat())));
    assert_eq!(sel(&["--plan", "pay"]), vec![2, 3, 4, 5]);
    assert_eq!(sel(&["--context", "ctx"]), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(sel(&["--session", "lead1"]), vec![1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(sel(&["--session", "worker1"]), vec![5]);
    assert_eq!(sel(&["--kind", "plan-created", "--kind", "error"]), vec![2, 8, 11]);
}

#[test]
fn follow_refusals() {
    let fx = seeded();
    refused_follow(&fx, &["--plan", "pay", "--context", "ctx"], "usage", 2);
    refused_follow(&fx, &["--session", "lead1", "--plan", "pay"], "usage", 2);
    refused_follow(&fx, &["--plan", "nope"], "plan_not_found", 4);
    refused_follow(&fx, &["--context", "nope"], "context_not_found", 4);
    refused_follow(&fx, &["--session", "nope"], "session_not_found", 4);
    refused_follow(&fx, &["--plan", "Bad_Id"], "invalid_plan_id", 2);
    let err = refused_follow(&fx, &["--since", "12", "--until-idle"], "cursor_ahead", 3);
    assert_eq!(err["details"]["highest"], 11);
    refused_follow(&fx, &["--since", "-3"], "usage", 2);
    let bare = fx.another_env();
    common::refused(&in_bare(&fx, &bare, || follow_once(&fx, &[])), "config_missing", 1);
}

#[test]
fn follow_since_zero_on_an_empty_store_is_fine_and_since_one_is_ahead() {
    let fx = Fx::new();
    fx.env.write_config(CONFIG);
    assert!(follow_until_idle(&fx, &["--since", "0"]).is_empty());
    refused_follow(&fx, &["--since", "1", "--until-idle"], "cursor_ahead", 3);
}

#[test]
fn follow_reads_full_batches_until_it_has_everything() {
    let fx = Fx::new();
    fx.env.write_config(CONFIG);
    let mut store = open(&fx.env);
    store
        .write("bulk", |tx| {
            for n in 0..1203 {
                append(tx, NewEvent::new(Kind::WatchStarted, format!("start {n}"), json!({"pid": n})))?;
            }
            Ok(())
        })
        .unwrap();
    let lines = follow_until_idle(&fx, &["--since", "0"]);
    assert_eq!(ids(&Value::Array(lines)), (1..=1203).collect::<Vec<_>>());
    let listed = list(&fx, &["--limit", "5000"]);
    assert_eq!(ids(&listed["events"]).len(), 1203);
    assert_eq!(list(&fx, &[])["events"].as_array().unwrap().len(), 500);
}

fn append_watch(store: &mut Store, n: i64) -> i64 {
    append_one(store, NewEvent::new(Kind::WatchStarted, format!("start {n}"), json!({"pid": n}))).unwrap()
}

fn append_edit(store: &mut Store, field: &str) -> i64 {
    let e = NewEvent::new(Kind::JobEdited, "edit", json!({"fields": [field]})).plan(plan("pay")).job(job());
    append_one(store, e).unwrap()
}

#[test]
fn follow_streams_new_events_in_commit_order_and_exits_zero_when_stdout_closes() {
    let fx = seeded();
    let mut store = open(&fx.env);
    let mut follower = Follower::start(&fx, &["--plan", "pay", "--since", "11"]);
    let a = append_edit(&mut store, "goal");
    append_watch(&mut store, 1); // not selected
    let b = append_edit(&mut store, "tier");
    let first = follower.next().unwrap();
    assert_eq!((first["id"].as_i64().unwrap(), &first["kind"]), (a, &json!("job-edited")));
    assert_eq!(first["data"], json!({"fields": ["goal"]}));
    let second = follower.next().unwrap();
    assert_eq!(second["id"].as_i64().unwrap(), b);
    // It keeps running; a later Event still arrives.
    let c = append_edit(&mut store, "touches");
    assert_eq!(follower.next().unwrap()["id"].as_i64().unwrap(), c);
    // Close stdout: the next Event it prints finds the pipe closed (EPIPE), and it exits 0.
    follower.close_stdout();
    append_edit(&mut store, "acceptance");
    append_edit(&mut store, "context");
    assert_eq!(follower.exit(), 0);
}

#[test]
fn follow_exits_zero_when_stdout_closes_with_no_event_to_write() {
    let fx = seeded();
    let mut store = open(&fx.env);
    let mut follower = Follower::start(&fx, &["--session", "lead1", "--since", "11"]);
    // It is in its poll loop once it prints an Event committed after it started; then close its stdout and append nothing.
    append_one(&mut store, NewEvent::new(Kind::SessionRenamed, "Session lead1 renamed.", json!({"name": "L"})).session(session("lead1"))).unwrap();
    assert_eq!(follower.next().unwrap()["kind"], "session-renamed");
    follower.close_stdout();
    assert_eq!(follower.exit(), 0, "the follower exits when its reader goes away");
}

#[test]
fn follow_without_since_prints_only_events_committed_after_it_started() {
    let fx = seeded();
    let mut store = open(&fx.env);
    let mut follower = Follower::start(&fx, &[]);
    let probe = append_watch(&mut store, 7);
    let first = follower.next().unwrap();
    // Nothing of the eleven seeded Events, which were committed before it started.
    assert!(first["id"].as_i64().unwrap() > 11, "{first}");
    assert_eq!((first["id"].as_i64().unwrap(), &first["kind"]), (probe, &json!("watch-started")));
}

#[test]
fn follow_since_prints_the_backlog_then_the_new_events() {
    let fx = seeded();
    let mut store = open(&fx.env);
    let mut follower = Follower::start(&fx, &["--since", "9"]);
    assert_eq!(follower.next().unwrap()["id"], 10);
    assert_eq!(follower.next().unwrap()["id"], 11);
    let n = append_watch(&mut store, 2);
    assert_eq!(follower.next().unwrap()["id"].as_i64().unwrap(), n);
}

/// What a newer cpd does to the store: a migration this binary lacks (`compat` `additive` or
/// `breaking`) that changes the schema and is recorded under `cpd_txn()`, stood in by the newest `txns` id.
fn newer_cpd_migrates(env: &TestEnv, compat: &str) {
    use rusqlite::functions::FunctionFlags;
    let conn = Connection::open(home(env).db()).unwrap();
    let txn: i64 = conn.query_row("SELECT max(id) FROM txns", [], |r| r.get(0)).unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn)).unwrap();
    // One transaction, as a migration is: the follower recomputes the verdict when `schema_version`
    // moves, so the row must not commit after the DDL.
    conn.execute_batch("BEGIN; CREATE TABLE newer_things (id INTEGER PRIMARY KEY) STRICT;").unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
         VALUES ('29991231T2359-newer', ?1, lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?2, 0)",
        rusqlite::params![compat, txn],
    )
    .unwrap();
    conn.execute_batch("COMMIT;").unwrap();
}

#[test]
fn follow_a_failure_after_the_first_line_is_the_last_line() {
    let fx = seeded();
    let mut follower = Follower::start(&fx, &["--since", "10"]);
    assert_eq!(follower.next().unwrap()["id"], 11);
    // An upgrade by a newer cpd while the follower runs.
    newer_cpd_migrates(&fx.env, "breaking");
    let last = follower.next().unwrap();
    assert_eq!(last["ok"], false);
    assert_eq!(last["error"]["code"], "store_too_new");
    assert_eq!(follower.next(), None);
    assert_eq!(follower.exit(), 1);
}

#[test]
fn follow_exits_on_a_store_too_new_before_it_has_printed_anything() {
    let fx = seeded();
    newer_cpd_migrates(&fx.env, "breaking");
    let mut follower = Follower::start(&fx, &["--since", "11"]);
    let last = follower.next().unwrap();
    assert_eq!(last["ok"], false);
    assert_eq!(last["error"]["code"], "store_too_new");
    assert_eq!(follower.next(), None);
    assert_eq!(follower.exit(), 1);
}

#[test]
fn follow_exits_non_zero_when_the_store_becomes_newer_while_it_is_idle() {
    let fx = seeded();
    let mut store = open(&fx.env);
    let mut follower = Follower::start(&fx, &["--plan", "pay", "--since", "11"]);
    // It is idle, in its poll loop, once it has printed an Event committed after it started.
    append_edit(&mut store, "goal");
    assert_eq!(follower.next().unwrap()["kind"], "job-edited");
    newer_cpd_migrates(&fx.env, "breaking");
    let last = follower.next().unwrap();
    assert_eq!(last["error"]["code"], "store_too_new");
    assert_eq!(follower.exit(), 1);
}

#[test]
fn follow_goes_on_when_the_store_gains_only_an_additive_migration_this_binary_lacks() {
    let fx = seeded();
    let mut follower = Follower::start(&fx, &["--since", "10"]);
    assert_eq!(follower.next().unwrap()["id"], 11);
    // Inside the window (contract §4.5): the verdict is recomputed, still opens, nothing ends.
    newer_cpd_migrates(&fx.env, "additive");
    let mut store = open(&fx.env);
    // The Event committed after the migration is printed by a poll that has read the migration; the next one shows
    // that the follower went on past that poll.
    for i in 1..=2 {
        let n = append_watch(&mut store, i);
        assert_eq!(follower.next().unwrap()["id"].as_i64().unwrap(), n);
    }
}

#[test]
fn follow_ends_when_a_migration_leaves_the_verdict_refusing_even_if_user_version_never_moves() {
    // `migration_order`-like and `store_too_new` refusals come from the verdict, not from a number:
    // the schema changes (so `schema_version` moves) and `user_version` stays what it was.
    let fx = seeded();
    let mut follower = Follower::start(&fx, &["--plan", "pay", "--since", "11"]);
    // It is idle, in its poll loop, once it has printed an Event committed after it started.
    let n = append_edit(&mut open(&fx.env), "goal");
    assert_eq!(follower.next().unwrap()["id"].as_i64().unwrap(), n);
    newer_cpd_migrates(&fx.env, "breaking");
    let user_version: i64 = Connection::open(home(&fx.env).db()).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(user_version, cpd_core::store::VERSION);
    let last = follower.next().unwrap();
    assert_eq!(last["error"]["code"], "store_too_new");
    assert_eq!(last["error"]["details"]["floor"], "29991231T2359-newer");
    assert_eq!(follower.exit(), 1);
}

#[test]
fn follow_re_execs_when_its_binary_is_replaced_and_loses_and_repeats_no_event() {
    let fx = seeded();
    let mut store = open(&fx.env);
    // The follower's own executable is a file the test then replaces by a rename, so its path holds another inode: the
    // `FakeSpawn` says it is cpd's, records the `exec` and returns, as an `exec` that failed does.
    let bin = tempfile::tempdir().unwrap();
    let exe = bin.path().join("cpd");
    fs::write(&exe, "v1").unwrap();
    fx.fakes.spawn.set_self_exe(&exe);
    let mut follower = Follower::start(&fx, &["--plan", "pay", "--since", "11"]);
    let a = append_edit(&mut store, "goal");
    assert_eq!(follower.next().unwrap()["id"].as_i64().unwrap(), a);

    fs::write(bin.path().join("cpd.new"), "v2").unwrap();
    fs::rename(bin.path().join("cpd.new"), &exe).unwrap();
    assert_eq!(follower.exit(), 1, "the fake exec returns: the follower ends with its error, spawn_failed");
    assert_eq!(follower.next().unwrap()["error"]["code"], "spawn_failed");
    let launch = fx.take_launch(0);
    assert_eq!(fx.fakes.spawn.launches().len(), 1);
    assert_eq!(std::path::Path::new(&launch.program), exe);
    // The same follower again, after the last Event it printed.
    assert_eq!(launch.args.join(" "), format!("events follow --plan pay --since {a}"));

    let b = append_edit(&mut store, "tier");
    let c = append_edit(&mut store, "touches");
    let mut restarted = Follower::start_launch(&fx, &launch);
    assert_eq!(restarted.next().unwrap()["id"].as_i64().unwrap(), b);
    assert_eq!(restarted.next().unwrap()["id"].as_i64().unwrap(), c);
}
