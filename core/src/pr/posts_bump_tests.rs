//! Tests of the repeat bump of `pr_posts` (contract §4.4): `last_posted_at`, `next_bump`, `mark_bumped`.

use super::*;
use crate::home::CpdHome;
use crate::store::Store;

const OLD: &str = "2026-01-01T00:00:00.000Z";
const OLDER: &str = "2025-12-01T00:00:00.000Z";
const CUTOFF: &str = "2026-02-01T00:00:00.000Z";
const NEW: &str = "2026-03-01T00:00:00.000Z";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');",
            )?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// What a fixture row looks like: the pull request, the Workflow, the Plan and the post times.
struct Row {
    draft: i64,
    state: &'static str,
    review: &'static str,
    status: &'static str,
    posted_at: &'static str,
    last: Option<&'static str>,
}

fn row(posted_at: &'static str, last: Option<&'static str>) -> Row {
    Row {
        draft: 0,
        state: "open",
        review: "none",
        status: "open",
        posted_at,
        last,
    }
}

/// Adds Workflow `key` with its pull request and a post; `last_posted_at` is written by SQL, as the migration will have it.
fn add(store: &mut Store, key: &str, r: &Row) {
    store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                 VALUES ('a/b', (SELECT count(*) + 1 FROM pull_requests), 'u', 't', ?1, ?2, ?3, 'none', 't', 't')",
                rusqlite::params![r.draft, r.state, r.review],
            )?;
            tx.execute(
                "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, pr_number, pr_url, pr_opened_at, pull_request, merged_at)
                 VALUES ('pay', ?1, '/r', 'main', ?2, 'cpd/pay/' || ?1, '/r/lead', cpd_txn(), 't', 't', 7, 'u', 't', (SELECT max(id) FROM pull_requests), IIF(?2 = 'merged', 't', NULL))",
                rusqlite::params![key, r.status],
            )?;
            let post = PostRecord {
                channel: "C0POLICY1".into(),
                ts: format!("1793040197.{key:0>6}"),
                link: "l".into(),
                posted_at: r.posted_at.into(),
                reaction: None,
                reacted_at: None,
                auto_merge: String::new(),
                auto_merge_at: None,
            };
            insert(tx, "pay", key, &post)?;
            tx.execute("UPDATE pr_posts SET last_posted_at = ?2 WHERE workflow = ?1", rusqlite::params![key, r.last])?;
            Ok(())
        })
        .unwrap();
}

fn last_posted_at(store: &mut Store, key: &str) -> Option<String> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT last_posted_at FROM pr_posts WHERE workflow = ?1",
                [key],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn next(store: &mut Store, skip: &[(String, String)]) -> Option<String> {
    store
        .read(|c| next_bump(c, CUTOFF, skip))
        .unwrap()
        .map(|b| b.workflow)
}

#[test]
fn insert_sets_last_posted_at_equal_to_posted_at() {
    let (_dir, mut store) = store();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                   VALUES ('a/b', 1, 'u', 't', 0, 'open', 'none', 'none', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, pr_number, pr_url, pr_opened_at, pull_request)
                   VALUES ('pay', 'a', '/r', 'main', 'open', 'cpd/pay/a', '/r/lead', cpd_txn(), 't', 't', 7, 'u', 't', 1);",
            )?;
            let post = PostRecord {
                channel: "C0POLICY1".into(),
                ts: "1793040197.000001".into(),
                link: "l".into(),
                posted_at: OLD.into(),
                reaction: None,
                reacted_at: None,
                auto_merge: String::new(),
                auto_merge_at: None,
            };
            assert!(insert(tx, "pay", "a", &post)?);
            Ok(())
        })
        .unwrap();
    assert_eq!(last_posted_at(&mut store, "a").as_deref(), Some(OLD));
}

#[test]
fn next_bump_returns_the_oldest_due_open_unreviewed_post() {
    let (_dir, mut store) = store();
    add(&mut store, "a", &row(OLDER, Some(OLD)));
    add(
        &mut store,
        "b",
        &row(OLDER, Some("2025-12-15T00:00:00.000Z")),
    );
    add(&mut store, "c", &row("2025-12-20T00:00:00.000Z", None));
    let bump = store.read(|c| next_bump(c, CUTOFF, &[])).unwrap().unwrap();
    assert_eq!(
        bump,
        Bump {
            plan: "pay".into(),
            workflow: "b".into(),
            channel: "C0POLICY1".into(),
            ts: "1793040197.00000b".into()
        }
    );
    // NULL last_posted_at falls back to posted_at: c (12-20) is older than a (01-01).
    let skip = [("pay".to_owned(), "b".to_owned())];
    assert_eq!(next(&mut store, &skip).as_deref(), Some("c"));
}

#[test]
fn next_bump_skips_a_post_younger_than_the_cutoff() {
    let (_dir, mut store) = store();
    add(&mut store, "a", &row(OLDER, Some(NEW)));
    add(&mut store, "b", &row(NEW, None));
    assert_eq!(next(&mut store, &[]), None);
    add(&mut store, "c", &row(OLDER, Some(CUTOFF)));
    assert_eq!(next(&mut store, &[]).as_deref(), Some("c"));
}

#[test]
fn next_bump_skips_draft_closed_reviewed_and_non_open_workflow_and_archived_plan() {
    let (_dir, mut store) = store();
    add(
        &mut store,
        "draft",
        &Row {
            draft: 1,
            ..row(OLDER, Some(OLD))
        },
    );
    add(
        &mut store,
        "closed",
        &Row {
            state: "closed",
            ..row(OLDER, Some(OLD))
        },
    );
    add(
        &mut store,
        "reviewed",
        &Row {
            review: "approved",
            ..row(OLDER, Some(OLD))
        },
    );
    add(
        &mut store,
        "merged",
        &Row {
            status: "merged",
            ..row(OLDER, Some(OLD))
        },
    );
    assert_eq!(next(&mut store, &[]), None);
    add(&mut store, "fine", &row(OLDER, Some(OLD)));
    assert_eq!(next(&mut store, &[]).as_deref(), Some("fine"));
    store
        .write("archive", |tx| {
            Ok(tx.execute("UPDATE plans SET archived_at = 't' WHERE id = 'pay'", [])?)
        })
        .unwrap();
    assert_eq!(next(&mut store, &[]), None);
}

#[test]
fn next_bump_ignores_the_skip_pairs() {
    let (_dir, mut store) = store();
    add(&mut store, "a", &row(OLDER, Some(OLDER)));
    add(&mut store, "b", &row(OLDER, Some(OLD)));
    let skip = [
        ("pay".to_owned(), "a".to_owned()),
        ("other".to_owned(), "b".to_owned()),
    ];
    assert_eq!(next(&mut store, &skip).as_deref(), Some("b"));
    let both = [
        ("pay".to_owned(), "a".to_owned()),
        ("pay".to_owned(), "b".to_owned()),
    ];
    assert_eq!(next(&mut store, &both), None);
}

#[test]
fn mark_bumped_sets_last_posted_at_to_the_transaction_time_and_keeps_posted_at() {
    let (_dir, mut store) = store();
    add(&mut store, "a", &row(OLDER, Some(OLD)));
    let at = |store: &mut Store| -> String {
        store
            .write("bump", |tx| {
                assert!(mark_bumped(tx, "pay", "a")?);
                Ok(tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?)
            })
            .unwrap()
    };
    let first = at(&mut store);
    assert_eq!(
        last_posted_at(&mut store, "a").as_deref(),
        Some(first.as_str())
    );
    let post = store.read(|c| find(c, "pay", "a")).unwrap().unwrap();
    assert_eq!(post.posted_at, OLDER);
    let past = "2020-01-01T00:00:00.000Z";
    store
        .write("past", |tx| {
            tx.execute(
                "UPDATE pr_posts SET last_posted_at = ?1 WHERE workflow = 'a'",
                [past],
            )?;
            Ok(())
        })
        .unwrap();
    let second = at(&mut store);
    assert_ne!(second, past, "the second bump moves it again");
    assert_eq!(
        last_posted_at(&mut store, "a").as_deref(),
        Some(second.as_str())
    );
    assert!(
        !store
            .write("bump", |tx| mark_bumped(tx, "pay", "nope"))
            .unwrap()
    );
}
