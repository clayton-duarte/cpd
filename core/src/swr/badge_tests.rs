//! The Lead completion badge (docs/architecture/swr-classifier.md §2): put, get, freshness, eviction.
//! A temp store; time is passed in from a `ManualClock`; a write transaction is a `txns` row.

use std::time::Duration;

use serde_json::json;

use super::badge::{self, Classification};
use super::gc;
use super::outbox_tests::store;
use crate::clock::{Clock, ManualClock};
use crate::store::Store;

const SESSION: &str = "s1";
const TTL: Duration = Duration::from_secs(7 * 86_400);
const DAY: Duration = Duration::from_secs(86_400);

fn txns(store: &mut Store) -> i64 {
    store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?))
        .unwrap()
}

fn rows(store: &mut Store) -> i64 {
    store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM swr_badges", [], |r| r.get(0))?))
        .unwrap()
}

fn done(entry: &str) -> Classification {
    Classification {
        entry_id: entry.into(),
        label: "safe_to_archive".into(),
        score: 93,
        text: "Safe to archive · 93%".into(),
        tone: "done".into(),
        others: None,
    }
}

#[test]
fn badge_put_then_get_returns_the_stored_classification() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    assert_eq!(
        badge::get(&mut store, SESSION).unwrap(),
        None,
        "a miss is data"
    );

    let put = badge::put(&mut store, t0, SESSION, &done("e1")).unwrap();
    assert!(put.changed);
    assert_eq!(put.synced_at, t0);

    let got = badge::get(&mut store, SESSION).unwrap().unwrap();
    assert_eq!(got.session_id, SESSION);
    assert_eq!(got.classification, done("e1"));
    assert_eq!(got.synced_at, t0);
}

#[test]
fn badge_unchanged_put_opens_no_write_txn() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    badge::put(&mut store, t0, SESSION, &done("e1")).unwrap();
    let before = txns(&mut store);

    clock.advance(Duration::from_secs(60));
    let again = badge::put(&mut store, clock.utc(), SESSION, &done("e1")).unwrap();
    assert!(!again.changed);
    assert_eq!(again.synced_at, t0, "the stored synced_at is kept");
    assert_eq!(txns(&mut store), before, "no write transaction was opened");
}

#[test]
fn badge_entry_change_replaces_the_row() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    badge::put(&mut store, t0, SESSION, &done("e1")).unwrap();

    clock.advance(Duration::from_secs(60));
    let t1 = clock.utc();
    let next = Classification {
        label: "open_work".into(),
        score: 40,
        text: "Open work · 40%".into(),
        tone: "blocked".into(),
        ..done("e2")
    };
    let put = badge::put(&mut store, t1, SESSION, &next).unwrap();
    assert!(put.changed);
    assert_eq!(put.synced_at, t1);
    assert_eq!(rows(&mut store), 1, "one row per Session");
    let got = badge::get(&mut store, SESSION).unwrap().unwrap();
    assert_eq!(got.classification, next);
    assert_eq!(got.synced_at, t1);
}

#[test]
fn badge_get_is_fresh_while_entry_id_matches() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    assert!(
        !badge::is_fresh(&mut store, SESSION, "e1").unwrap(),
        "no row is stale"
    );
    badge::put(&mut store, t0, SESSION, &done("e1")).unwrap();
    assert!(badge::is_fresh(&mut store, SESSION, "e1").unwrap());
    assert!(
        !badge::is_fresh(&mut store, SESSION, "e2").unwrap(),
        "the transcript moved"
    );
    assert!(
        badge::get(&mut store, SESSION).unwrap().is_some(),
        "a stale row is still returned"
    );
}

#[test]
fn badge_rows_unused_for_seven_days_are_garbage_collected() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    badge::put(&mut store, t0, "stopped", &done("e1")).unwrap();
    badge::put(&mut store, t0, "live", &done("e1")).unwrap();
    badge::put(&mut store, t0, "revived", &done("e1")).unwrap();
    badge::mark_inactive(&mut store, "stopped", t0).unwrap();
    badge::mark_inactive(&mut store, "revived", t0).unwrap();
    badge::put(&mut store, t0 + 6 * DAY, "revived", &done("e2")).unwrap();

    let deleted = gc::sweep(&mut store, t0 + 8 * DAY, TTL).unwrap();
    assert_eq!(deleted, 1, "only the stopped Session's row goes");
    assert!(badge::get(&mut store, "stopped").unwrap().is_none());
    assert!(
        badge::get(&mut store, "live").unwrap().is_some(),
        "never inactive"
    );
    assert!(
        badge::get(&mut store, "revived").unwrap().is_some(),
        "a put clears inactive_at"
    );
}

#[test]
fn badge_others_json_is_optional_and_round_trips() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    badge::put(&mut store, t0, "plain", &done("e1")).unwrap();
    let plain = badge::get(&mut store, "plain").unwrap().unwrap();
    assert_eq!(plain.classification.others, None);
    let stored: Option<String> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT others_json FROM swr_badges WHERE session_id = 'plain'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(stored, None, "NULL when others is absent");

    let others =
        json!([{"label": "open_work", "score": 30}, {"label": "unanswered_ask", "score": 12}]);
    let with = Classification {
        others: Some(others.clone()),
        ..done("e1")
    };
    badge::put(&mut store, t0, "rich", &with).unwrap();
    let got = badge::get(&mut store, "rich").unwrap().unwrap();
    assert_eq!(got.classification.others, Some(others));

    let before = txns(&mut store);
    assert!(!badge::put(&mut store, t0, "rich", &with).unwrap().changed);
    assert_eq!(txns(&mut store), before, "equal others opens no write");
}
