//! The mutation outbox (swr.md §8): enqueue and its coalesce rules, claim order, complete, fail.
//! A temp store; time is passed in from a `ManualClock`; no sleep.

use std::time::Duration;

use serde_json::{Value, json};
use time::OffsetDateTime;

use super::cache;
use super::outbox::{self, Coalesce, Mutation, Op, Retry};
use crate::clock::{Clock, ManualClock};
use crate::store::Store;

pub(super) fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = crate::home::CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

pub(super) const KEY: &str = "jira:issue:EEC-1";

/// A generic test op of system `test`; real datasource ops are not used here.
pub(super) fn op(kind: &str, vars: Value) -> Op {
    Op {
        system: "test".into(),
        target_key: KEY.into(),
        op_kind: kind.into(),
        vars,
        patch: Some(json!({"status": "x"})),
        context: None,
    }
}

pub(super) fn at(clock: &ManualClock, secs: u64) -> OffsetDateTime {
    clock.advance(Duration::from_secs(secs));
    clock.utc()
}

pub(super) fn row(store: &mut Store, id: i64) -> Mutation {
    outbox::get(store, id).unwrap().expect("the mutation row")
}

fn status(store: &mut Store, id: i64) -> String {
    row(store, id).status
}

#[test]
fn swr_enqueue_creates_pending_mutation() {
    let (_dir, mut store) = store();
    let now = ManualClock::new().utc();
    let id = outbox::enqueue(
        &mut store,
        now,
        op("merge", json!({"n": 1})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let m = row(&mut store, id);
    assert_eq!(m.status, "pending");
    assert_eq!(m.system, "test");
    assert_eq!(m.target_key, KEY);
    assert_eq!(m.op_kind, "merge");
    assert_eq!(m.submitted_at, now);
    assert_eq!(m.attempt_count, 0);
    assert_eq!(m.started_at, None);
    assert_eq!(m.next_attempt_at, None);
    assert_eq!(
        serde_json::from_str::<Value>(&m.vars_json).unwrap(),
        json!({"n": 1})
    );
    assert_eq!(
        serde_json::from_str::<Value>(m.patch_json.as_deref().unwrap()).unwrap(),
        json!({"status": "x"})
    );
}

#[test]
fn swr_newer_pending_supersedes_unstarted_same_key() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let a = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("merge", json!({"v": 1})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let other_kind = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("auto_merge", json!({"v": 1})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let b = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("merge", json!({"v": 2})),
        Coalesce::LatestWins,
    )
    .unwrap();
    assert_ne!(a, b);
    assert_eq!(status(&mut store, a), "superseded");
    assert_eq!(status(&mut store, b), "pending");
    assert_eq!(
        status(&mut store, other_kind),
        "pending",
        "another op_kind is another active key"
    );
}

#[test]
fn swr_in_flight_mutation_is_never_superseded() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let a = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("merge", json!({"v": 1})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let claimed = outbox::claim_next(&mut store, at(&clock, 1), "test")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, a);
    let b = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("merge", json!({"v": 2})),
        Coalesce::LatestWins,
    )
    .unwrap();
    assert_eq!(status(&mut store, a), "in_flight");
    assert_eq!(status(&mut store, b), "pending", "the new one waits behind");
}

#[test]
fn swr_forward_progression_keeps_the_furthest_target() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let (in_progress, in_review, done) = (1, 2, 3);
    let go = |store: &mut Store, now, to: &str, rank| {
        outbox::enqueue(
            store,
            now,
            op("transition", json!({"to": to})),
            Coalesce::Forward(rank),
        )
        .unwrap()
    };
    let a = go(&mut store, at(&clock, 0), "in_review", in_review);
    // A lower rank than an unstarted one: the new one is dropped itself.
    let b = go(&mut store, at(&clock, 1), "in_progress", in_progress);
    assert_eq!(status(&mut store, b), "superseded");
    assert_eq!(status(&mut store, a), "pending");
    // A higher rank supersedes every unstarted lower one.
    let c = go(&mut store, at(&clock, 1), "done", done);
    assert_eq!(status(&mut store, a), "superseded");
    assert_eq!(status(&mut store, c), "pending");
    assert_ne!(a, c);
}

#[test]
fn swr_dedupe_policy_returns_existing_active_mutation() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let vars = json!({"channel": "C1", "ts": "1.2", "emoji": "eyes"});
    let key = vars.to_string();
    let first = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("reaction", vars.clone()),
        Coalesce::Dedupe(key.clone()),
    )
    .unwrap();
    let again = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("reaction", vars.clone()),
        Coalesce::Dedupe(key.clone()),
    )
    .unwrap();
    assert_eq!(again, first, "the existing id comes back");
    let count: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM swr_mutations", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 1, "nothing is written");

    let other = json!({"channel": "C1", "ts": "1.2", "emoji": "tada"});
    let different = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("reaction", other.clone()),
        Coalesce::Dedupe(other.to_string()),
    )
    .unwrap();
    assert_ne!(different, first, "another emoji is another mutation");
}

#[test]
fn swr_claim_next_takes_oldest_due_and_marks_in_flight() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = at(&clock, 0);
    let mut older = op("reaction", json!({"n": 1}));
    older.target_key = "slack:channel:C1".into();
    let mut newer = op("reaction", json!({"n": 2}));
    newer.target_key = "slack:channel:C2".into();
    let mut elsewhere = op("reaction", json!({"n": 3}));
    elsewhere.system = "other".into();
    let a = outbox::enqueue(&mut store, t0, older, Coalesce::LatestWins).unwrap();
    let b = outbox::enqueue(&mut store, at(&clock, 1), newer, Coalesce::LatestWins).unwrap();
    outbox::enqueue(&mut store, at(&clock, 1), elsewhere, Coalesce::LatestWins).unwrap();

    let now = at(&clock, 1);
    let m = outbox::claim_next(&mut store, now, "test")
        .unwrap()
        .unwrap();
    assert_eq!(m.id, a, "the oldest first");
    assert_eq!(m.status, "in_flight");
    assert_eq!(m.started_at, Some(now));
    assert_eq!(m.attempt_count, 1);
    assert_eq!(row(&mut store, a).status, "in_flight");

    // A mutation backed off to the future is not due.
    outbox::fail(
        &mut store,
        a,
        now,
        "boom",
        Retry::After(Duration::from_secs(60)),
    )
    .unwrap();
    let next = outbox::claim_next(&mut store, now, "test")
        .unwrap()
        .unwrap();
    assert_eq!(next.id, b, "the backed-off one is skipped");
    assert!(
        outbox::claim_next(&mut store, now, "test")
            .unwrap()
            .is_none(),
        "nothing else is due"
    );
    let due = now + Duration::from_secs(60);
    let again = outbox::claim_next(&mut store, due, "test")
        .unwrap()
        .unwrap();
    assert_eq!(again.id, a);
    assert_eq!(again.attempt_count, 2);
}

#[test]
fn swr_complete_moves_to_revalidating_and_invalidates_query() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let id = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("merge", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    outbox::claim_next(&mut store, at(&clock, 1), "test")
        .unwrap()
        .unwrap();
    let done_at = at(&clock, 1);
    outbox::complete(&mut store, id, done_at).unwrap();
    let m = row(&mut store, id);
    assert_eq!(m.status, "revalidating");
    assert_eq!(m.completed_at, Some(done_at));
    let q = cache::get(&mut store, KEY).unwrap().expect("the query row");
    assert!(q.invalidated, "complete invalidates the target key");
}

#[test]
fn swr_permanent_failure_marks_failed_and_drops_overlay() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let id = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("merge", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    outbox::claim_next(&mut store, at(&clock, 1), "test")
        .unwrap()
        .unwrap();
    assert_eq!(
        super::overlay::active(&mut store, "test", KEY)
            .unwrap()
            .len(),
        1
    );
    outbox::fail(&mut store, id, at(&clock, 1), "denied", Retry::Never).unwrap();
    let m = row(&mut store, id);
    assert_eq!(m.status, "failed");
    assert_eq!(m.error.as_deref(), Some("denied"));
    assert!(
        super::overlay::active(&mut store, "test", KEY)
            .unwrap()
            .is_empty(),
        "a failed write's overlay disappears at once"
    );
}

#[test]
fn swr_transient_failure_backs_off_and_retries() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let id = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("merge", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let now = at(&clock, 1);
    outbox::claim_next(&mut store, now, "test")
        .unwrap()
        .unwrap();
    outbox::fail(
        &mut store,
        id,
        now,
        "timeout",
        Retry::After(Duration::from_secs(30)),
    )
    .unwrap();
    let m = row(&mut store, id);
    assert_eq!(m.status, "pending");
    assert_eq!(m.error.as_deref(), Some("timeout"));
    assert_eq!(m.next_attempt_at, Some(now + Duration::from_secs(30)));
    assert!(
        outbox::claim_next(&mut store, now + Duration::from_secs(29), "test")
            .unwrap()
            .is_none()
    );
    let again = outbox::claim_next(&mut store, now + Duration::from_secs(30), "test")
        .unwrap()
        .unwrap();
    assert_eq!(again.id, id);
    assert_eq!(again.attempt_count, 2);
}
