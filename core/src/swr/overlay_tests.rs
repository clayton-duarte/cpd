//! The overlay and its poll fence (swr.md §9, §10). A temp store; explicit times; no sleep.

use std::time::Duration;

use serde_json::json;

use super::cache::{self, Committed};
use super::outbox::{self, Coalesce};
use super::outbox_tests::{KEY, at, op, row, store};
use super::overlay;
use crate::clock::ManualClock;
use crate::store::Store;

/// A mutation of `KEY` completed at `completed`, as `(id, completed_at)`; the query row exists with `hash`.
fn revalidating(store: &mut Store, clock: &ManualClock, kind: &str) -> (i64, time::OffsetDateTime) {
    let t0 = at(clock, 0);
    cache::commit(store, KEY, "h1", t0, t0, |_| Ok(())).unwrap();
    let id = outbox::enqueue(
        store,
        at(clock, 1),
        op(kind, json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    outbox::claim_next(store, at(clock, 1), "test")
        .unwrap()
        .unwrap();
    let completed = at(clock, 1);
    outbox::complete(store, id, completed).unwrap();
    (id, completed)
}

#[test]
fn swr_overlay_lists_pending_in_flight_and_revalidating() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let rev = outbox::enqueue(
        &mut store,
        at(&clock, 0),
        op("a", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let fly = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("b", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let pend = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("c", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let dead = outbox::enqueue(
        &mut store,
        at(&clock, 1),
        op("d", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    let mut elsewhere = op("a", json!({}));
    elsewhere.target_key = "jira:issue:EEC-2".into();
    outbox::enqueue(&mut store, at(&clock, 1), elsewhere, Coalesce::LatestWins).unwrap();

    // a -> revalidating, b -> in_flight, d -> failed, c stays pending.
    let now = at(&clock, 1);
    assert_eq!(
        outbox::claim_next(&mut store, now, "test")
            .unwrap()
            .unwrap()
            .id,
        rev
    );
    outbox::complete(&mut store, rev, now).unwrap();
    assert_eq!(
        outbox::claim_next(&mut store, now, "test")
            .unwrap()
            .unwrap()
            .id,
        fly
    );
    outbox::fail(&mut store, dead, now, "no", outbox::Retry::Never).unwrap();

    let ids: Vec<i64> = overlay::active(&mut store, "test", KEY)
        .unwrap()
        .iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(
        ids,
        vec![rev, fly, pend],
        "submitted_at order; failed excluded"
    );
}

#[test]
fn swr_poll_started_before_completion_does_not_settle_overlay() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let (id, completed) = revalidating(&mut store, &clock, "merge");
    let started = completed - Duration::from_secs(1);
    let c = cache::commit(&mut store, KEY, "h2", completed, started, |_| Ok(())).unwrap();
    assert_eq!(c, Committed::Changed, "the data is written");
    assert_eq!(row(&mut store, id).status, "revalidating");
    assert_eq!(overlay::active(&mut store, "test", KEY).unwrap().len(), 1);
    cache::settle(&mut store, KEY, completed, started).unwrap();
    assert_eq!(
        row(&mut store, id).status,
        "revalidating",
        "settle obeys the fence too"
    );
}

#[test]
fn swr_poll_started_at_completion_settles_overlay() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let (id, completed) = revalidating(&mut store, &clock, "merge");
    cache::commit(&mut store, KEY, "h2", completed, completed, |_| Ok(())).unwrap();
    assert_eq!(row(&mut store, id).status, "done");
    assert!(overlay::active(&mut store, "test", KEY).unwrap().is_empty());
}

#[test]
fn swr_settle_runs_even_when_fetched_data_is_unchanged() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let (id, completed) = revalidating(&mut store, &clock, "merge");
    let c = cache::commit(&mut store, KEY, "h1", completed, completed, |_| Ok(())).unwrap();
    assert_eq!(c, Committed::Unchanged);
    assert_eq!(row(&mut store, id).status, "done");
    assert!(overlay::active(&mut store, "test", KEY).unwrap().is_empty());
}
