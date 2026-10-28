//! Engine step E20, Parked drafts (contract §3.7 "Parked drafts", §2.2 "A parked draft"; hs-d731, hs-d733; Plan
//! `draft-never-parked`), in-process on a prepared database with the fake tmux server of the tmux route, on virtual time.
//! The Lead is `idle` in a live pane; a full draft added after the lock with no Plan review is parked.

use crate::common;

use common::fx::Fx as Ip;
use serde_json::Value;

/// Ticks of the engine for "a later tick writes nothing more".
const LATER_TICKS: usize = 5;

/// Plan `pay` (`dispatch`) and its Lead `lead1`, `idle` in a live pane, the engine in-process on virtual time.
fn fx() -> Ip {
    let env = Ip::with_ports();
    env.engine_config_on_tmux("");
    env.engine_routes_on_tmux();
    let out = env.tmux(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    env.env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');");
    env.engine_plan();
    env.env.sql(&format!("UPDATE sessions SET tmux_pane = '{pane}', state = 'idle', state_at = 't' WHERE id = 'lead1';"));
    env
}

/// A full draft `key` of PR `repo`, no Plan review: parked.
fn draft(fx: &Ip, key: &str) {
    fx.add_draft_in("repo", key);
}

/// An Event of `kind` of Plan `pay`, as a command appends it.
fn event(fx: &Ip, kind: &str, job: &str) {
    fx.env.sql(&format!(
        "INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES (cpd_txn(), 't', '{kind}', 'ctx', 'pay', '{job}', 'x', '{{}}');"
    ));
}

/// Every `session-message` Event: (session, `data`).
fn messages(fx: &Ip) -> Vec<(String, Value)> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT session, data FROM events WHERE kind = 'session-message' ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(|(s, d)| (s, serde_json::from_str(&d).unwrap())).collect())
    })
}

fn count(fx: &Ip, kind: &str) -> i64 {
    fx.env.query(&format!("SELECT count(*) FROM events WHERE kind = '{kind}'"))
}

fn text(message: &(String, Value)) -> &str {
    message.1["text"].as_str().unwrap()
}

#[test]
fn wakes_the_idle_lead_once() {
    let fx = fx();
    draft(&fx, "d1");
    fx.watch_ticks(1).unwrap();
    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "one wake: {sent:?}");
    assert_eq!(sent[0].0, "lead1");
    assert_eq!((sent[0].1["from"].clone(), sent[0].1["wake"].clone()), (Value::Null, Value::Bool(true)), "{:?}", sent[0].1);
    assert!(text(&sent[0]).starts_with("Drafts of Plan pay are parked: d1 (no pending Plan review, no unfinished need)."), "{}", text(&sent[0]));
    assert!(text(&sent[0]).ends_with("Nothing runs it otherwise."), "{}", text(&sent[0]));

    // The message is the record: the next ticks send nothing.
    fx.watch_ticks(LATER_TICKS).unwrap();
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn lists_the_parked_keys_sorted() {
    let fx = fx();
    for key in ["zeta", "alpha", "mid"] {
        draft(&fx, key);
    }
    fx.watch_ticks(1).unwrap();
    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(text(&sent[0]).starts_with("Drafts of Plan pay are parked: alpha, mid, zeta (no pending Plan review"), "{}", text(&sent[0]));
}

#[test]
fn wakes_again_after_a_state_change() {
    let fx = fx();
    draft(&fx, "d1");
    draft(&fx, "d2");
    fx.watch_ticks(1).unwrap();
    assert_eq!(messages(&fx).len(), 1);

    // The Lead changes something (cancels d1, adds d3) and leaves drafts parked: T moves past W.
    fx.env.sql("UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE plan = 'pay' AND key = 'd1';");
    event(&fx, "job-cancelled", "d1");
    draft(&fx, "d3");
    event(&fx, "job-added", "d3");
    fx.watch_ticks(1).unwrap();
    let sent = messages(&fx);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(text(&sent[1]).starts_with("Drafts of Plan pay are parked: d2, d3 (no pending Plan review"), "{}", text(&sent[1]));

    fx.watch_ticks(LATER_TICKS).unwrap();
    assert_eq!(messages(&fx).len(), 2);
}

/// Archives Plan `pay` with two drafts (of any kind) and one queued Job, which E20 leaves alone.
fn archived_plan(fx: &Ip) {
    fx.add_job("q1", &[], "");
    draft(fx, "d1");
    draft(fx, "d2");
    fx.env.sql("UPDATE plans SET archived_at = 't' WHERE id = 'pay';");
}

fn statuses(fx: &Ip) -> Vec<(String, String)> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT key, status FROM jobs WHERE plan = 'pay' ORDER BY key")?;
        Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    })
}

fn expected_statuses(drafts: &str) -> Vec<(String, String)> {
    ["d1", "d2", "q1"].iter().map(|k| (k.to_string(), if *k == "q1" { "queued" } else { drafts }.to_owned())).collect()
}

#[test]
fn cancels_drafts_of_an_archived_plan() {
    let fx = fx();
    archived_plan(&fx);
    fx.watch_ticks(1).unwrap();
    assert_eq!(statuses(&fx), expected_statuses("cancelled"));
    let reasons: Vec<(String, String)> = fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT key, cancel_reason FROM jobs WHERE status = 'cancelled' ORDER BY key")?;
        Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    });
    let reason = "Plan archived with a draft (hs-d731)";
    assert_eq!(reasons, [("d1".to_owned(), reason.to_owned()), ("d2".to_owned(), reason.to_owned())]);
    let cancelled: Vec<String> = fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT job FROM events WHERE kind = 'job-cancelled' ORDER BY job")?;
        Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    });
    assert_eq!(cancelled, ["d1", "d2"], "one job-cancelled Event each");
    assert_eq!(messages(&fx), [], "an archived Plan's Lead is not woken");
}

#[test]
fn archived_cancel_is_idempotent() {
    let fx = fx();
    archived_plan(&fx);
    fx.watch_ticks(1).unwrap();
    assert_eq!(count(&fx, "job-cancelled"), 2, "the first tick cancelled both drafts");
    let before: i64 = fx.env.query("SELECT max(id) FROM events");
    fx.watch_ticks(LATER_TICKS).unwrap();
    let after: Vec<(i64, String)> = fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT id, kind FROM events WHERE id > ?1 AND kind IN ('job-cancelled', 'session-message')")?;
        Ok(stmt.query_map([before], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    });
    assert_eq!(after, [], "a later tick writes nothing more");
    assert_eq!(statuses(&fx), expected_statuses("cancelled"));
}
