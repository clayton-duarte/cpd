//! Eviction (swr.md §4): inactive past gcTime and unobserved is deleted; an observer keeps the row.
//! A temp store; explicit times; no sleep.

use std::time::Duration;

use serde_json::json;

use super::cache;
use super::gc;
use super::outbox::{self, Coalesce};
use super::outbox_tests::{op, store};
use crate::clock::{Clock, ManualClock};
use crate::store::Store;

const TTL: Duration = Duration::from_secs(7 * 86_400);
const DAY: Duration = Duration::from_secs(86_400);

fn present(store: &mut Store, key: &str) -> bool {
    cache::get(store, key).unwrap().is_some()
}

/// A query row of `key`, inactive since `since`.
fn inactive(store: &mut Store, key: &str, since: time::OffsetDateTime) {
    cache::invalidate(store, key).unwrap();
    cache::mark_inactive(store, key, since).unwrap();
}

#[test]
fn swr_gc_deletes_inactive_rows_older_than_ttl() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    inactive(&mut store, "slack:channel:old", t0);
    inactive(&mut store, "slack:channel:recent", t0 + 3 * DAY);
    cache::invalidate(&mut store, "slack:channel:active").unwrap();

    let now = t0 + 8 * DAY;
    let deleted = gc::sweep(&mut store, now, TTL).unwrap();
    assert_eq!(deleted, 1);
    assert!(!present(&mut store, "slack:channel:old"));
    assert!(
        present(&mut store, "slack:channel:recent"),
        "younger than the ttl"
    );
    assert!(
        present(&mut store, "slack:channel:active"),
        "never inactive"
    );
    assert_eq!(
        gc::sweep(&mut store, now, TTL).unwrap(),
        0,
        "rerunning is safe"
    );
}

#[test]
fn swr_gc_keeps_rows_with_an_active_observer() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    let mutated = "jira:issue:EEC-1";
    let in_workflow = "github:pr:acme/shop#7";
    let in_inbox = "slack:channel:C9";
    let nobody = "slack:channel:C0";
    for key in [mutated, in_workflow, in_inbox, nobody] {
        inactive(&mut store, key, t0);
    }
    outbox::enqueue(
        &mut store,
        t0,
        op("transition", json!({})),
        Coalesce::LatestWins,
    )
    .unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state, authored, review_requested)
                   VALUES (7, 'acme/shop', 7, 'https://example/7', 'Ship', 0, 'open', 'none', 'none', 't', NULL, 't', 'unknown', 'unknown', 0, 0);
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at)
                   VALUES ('pay', 'ship', 'acme/shop', 'main', 'open', 'cpd/pay/ship', '/w', 7, 'https://example/7', 't', 7, cpd_txn(), 't', 't');
                 INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
                   VALUES ('engine-error', 'red', 'slack:channel:C9', 'l', 't');",
            )?;
            Ok(())
        })
        .unwrap();

    let deleted = gc::sweep(&mut store, t0 + 30 * DAY, TTL).unwrap();
    assert_eq!(deleted, 1, "only the unobserved row goes");
    assert!(!present(&mut store, nobody));
    assert!(
        present(&mut store, mutated),
        "an active mutation observes it"
    );
    assert!(
        present(&mut store, in_workflow),
        "an open Workflow observes it"
    );
    assert!(
        present(&mut store, in_inbox),
        "an open Inbox item observes it"
    );
}
