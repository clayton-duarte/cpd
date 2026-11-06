//! The SWR cache (swr.md §6): the change-only commit, failures that keep data, invalidate and freshness.
//! A temp store; time is passed in from a `ManualClock`; a write transaction is a `txns` row.

use std::time::Duration;

use time::OffsetDateTime;

use super::cache::{self, Committed, Query};
use crate::clock::{Clock, ManualClock, timestamp};
use crate::error::CpdError;
use crate::store::Store;

const REPO: &str = "github:repo:acme/api";
const VIEWER: &str = "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, 'ada', 't')";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = crate::home::CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

fn txns(store: &mut Store) -> i64 {
    store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?))
        .unwrap()
}

fn text(store: &mut Store, sql: &str) -> Option<String> {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, Option<String>>(0))?))
        .unwrap()
}

fn int(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

fn after(clock: &ManualClock, secs: u64) -> OffsetDateTime {
    clock.advance(Duration::from_secs(secs));
    clock.utc()
}

/// A first Changed commit of `key` whose `apply` writes the viewer row.
fn first(store: &mut Store, key: &str, hash: &str, now: OffsetDateTime) -> Committed {
    cache::commit(store, key, hash, now, now, |tx| {
        tx.execute(VIEWER, [])?;
        Ok::<(), CpdError>(())
    })
    .unwrap()
}

#[test]
fn swr_commit_unchanged_hash_opens_no_write_txn() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    assert_eq!(first(&mut store, REPO, "h1", t0), Committed::Changed);
    let before = txns(&mut store);
    let t1 = after(&clock, 60);
    let committed = cache::commit(&mut store, REPO, "h1", t1, t1, |_| {
        panic!("apply never runs for unchanged data")
    })
    .unwrap();
    assert_eq!(committed, Committed::Unchanged);
    assert_eq!(txns(&mut store), before, "no write transaction was opened");
    assert_eq!(
        text(&mut store, "SELECT fetched_at FROM swr_queries"),
        Some(timestamp(t0)),
        "fetched_at is the time of the last Changed commit"
    );
}

#[test]
fn swr_commit_changed_hash_writes_rows_and_stamps_query() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    let before = txns(&mut store);
    assert_eq!(first(&mut store, REPO, "h1", t0), Committed::Changed);
    assert_eq!(txns(&mut store), before + 1, "one write transaction");
    assert_eq!(int(&mut store, "SELECT count(*) FROM swr_viewer"), 1);
    let q = cache::get(&mut store, REPO)
        .unwrap()
        .expect("the query row");
    assert_eq!(q.system, "github");
    assert_eq!(q.status, "success");
    assert_eq!(q.data_hash.as_deref(), Some("h1"));
    assert_eq!(q.fetched_at, Some(t0));
    assert_eq!(
        text(&mut store, "SELECT stale_after FROM swr_queries"),
        Some(timestamp(t0 + Duration::from_secs(24 * 3600))),
        "a repository is stale after 24 h"
    );
    assert_eq!(q.last_error, None);

    let t1 = after(&clock, 60);
    let again = cache::commit(&mut store, REPO, "h2", t1, t1, |tx| {
        tx.execute("UPDATE swr_viewer SET login = 'bob'", [])?;
        Ok::<(), CpdError>(())
    })
    .unwrap();
    assert_eq!(again, Committed::Changed);
    assert_eq!(txns(&mut store), before + 2);
    assert_eq!(
        text(&mut store, "SELECT data_hash FROM swr_queries"),
        Some("h2".into())
    );
    assert_eq!(
        text(&mut store, "SELECT login FROM swr_viewer"),
        Some("bob".into())
    );
}

#[test]
fn swr_failed_fetch_keeps_cached_data_and_records_error() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    first(&mut store, REPO, "h1", clock.utc());
    let t1 = after(&clock, 60);
    cache::fail(&mut store, REPO, t1, "gh: 502").unwrap();
    let q = cache::get(&mut store, REPO).unwrap().unwrap();
    assert_eq!(q.status, "error");
    assert_eq!(q.last_error.as_deref(), Some("gh: 502"));
    assert_eq!(q.error_count, 1);
    assert_eq!(q.data_hash.as_deref(), Some("h1"), "the data hash is kept");
    assert_eq!(
        text(&mut store, "SELECT login FROM swr_viewer"),
        Some("ada".into()),
        "the cached rows are kept"
    );
}

#[test]
fn swr_repeated_identical_failure_opens_no_write() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    first(&mut store, REPO, "h1", clock.utc());
    cache::fail(&mut store, REPO, after(&clock, 60), "gh: 502").unwrap();
    let before = txns(&mut store);
    cache::fail(&mut store, REPO, after(&clock, 60), "gh: 502").unwrap();
    assert_eq!(txns(&mut store), before, "the same error writes nothing");
    assert_eq!(
        int(&mut store, "SELECT error_count FROM swr_queries"),
        1,
        "a repeat is not counted"
    );
    cache::fail(&mut store, REPO, after(&clock, 60), "gh: 503").unwrap();
    assert_eq!(txns(&mut store), before + 1, "a different error is written");
    assert_eq!(int(&mut store, "SELECT error_count FROM swr_queries"), 2);
}

#[test]
fn swr_success_after_error_clears_error_state() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    first(&mut store, REPO, "h1", clock.utc());
    cache::fail(&mut store, REPO, after(&clock, 60), "gh: 502").unwrap();
    let before = txns(&mut store);
    let t2 = after(&clock, 60);
    let recovered = cache::commit(&mut store, REPO, "h1", t2, t2, |_| {
        panic!("unchanged data applies nothing")
    })
    .unwrap();
    assert_eq!(recovered, Committed::Unchanged);
    assert_eq!(txns(&mut store), before + 1, "leaving error is a write");
    let q = cache::get(&mut store, REPO).unwrap().unwrap();
    assert_eq!(q.status, "success");
    assert_eq!(q.last_error, None);

    cache::fail(&mut store, REPO, after(&clock, 60), "gh: 500").unwrap();
    let t4 = after(&clock, 60);
    let changed =
        cache::commit(&mut store, REPO, "h2", t4, t4, |_| Ok::<(), CpdError>(())).unwrap();
    assert_eq!(changed, Committed::Changed);
    let q = cache::get(&mut store, REPO).unwrap().unwrap();
    assert_eq!(q.status, "success");
    assert_eq!(q.last_error, None);
}

#[test]
fn swr_invalidate_marks_query_and_forces_stale() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    first(&mut store, REPO, "h1", clock.utc());
    assert!(!cache::get(&mut store, REPO).unwrap().unwrap().invalidated);
    let before = txns(&mut store);
    cache::invalidate(&mut store, REPO).unwrap();
    assert_eq!(txns(&mut store), before + 1, "invalidate is a write");
    assert_eq!(int(&mut store, "SELECT invalidated FROM swr_queries"), 1);
    let q = cache::get(&mut store, REPO).unwrap().unwrap();
    assert!(q.invalidated);
    assert_eq!(q.data_hash.as_deref(), Some("h1"), "the data is kept");
}

#[test]
fn swr_freshness_respects_stale_time_and_infinity() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    let pr = "github:pr:acme/api#12";
    first(&mut store, pr, "h1", t0);
    let row = cache::get(&mut store, pr).unwrap().unwrap();
    assert_eq!(row.stale_after, Some(t0 + Duration::from_secs(5)));
    assert!(cache::is_fresh(&row, t0 + Duration::from_secs(4)));
    assert!(!cache::is_fresh(&row, t0 + Duration::from_secs(5)));

    let channel = "slack:channel:C123";
    cache::commit(
        &mut store,
        channel,
        "h1",
        t0,
        t0,
        |_| Ok::<(), CpdError>(()),
    )
    .unwrap();
    let row = cache::get(&mut store, channel).unwrap().unwrap();
    assert_eq!(row.stale_after, Some(t0 + Duration::from_secs(30 * 60)));

    let never = Query {
        key: "github:pr:acme/api#13".into(),
        system: "github".into(),
        status: "success".into(),
        data_hash: Some("h".into()),
        fetched_at: Some(t0),
        invalidated: false,
        stale_after: None,
        last_error: None,
        error_count: 0,
        inactive_at: None,
    };
    assert!(cache::is_fresh(
        &never,
        t0 + Duration::from_secs(10 * 365 * 24 * 3600)
    ));
}
