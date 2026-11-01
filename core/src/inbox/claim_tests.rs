//! A dismissal or a claim of a `review-requested` item lasts for the pull request (pro-d58, C1, C3).

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Filter, Kind, State, list, reconcile};
use super::rules::{Team, conditions_with};
use super::team::{GhPr, GhState, Lookup};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

/// The team review cache holding one primary post per `(number, ts_raw)`, posted `age` seconds before `now`.
fn write_cache(home: &CpdHome, now: OffsetDateTime, posts: &[(u64, &str, f64)]) {
    let posts: Vec<Value> = posts
        .iter()
        .map(|(number, ts_raw, age)| {
            json!({"repo": "acme/pay", "number": number, "ts": now.unix_timestamp() as f64 - age, "ts_raw": ts_raw,
                   "author": "Ryan Case", "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false})
        })
        .collect();
    let path = home.run_dir().join("team-review.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let cache = json!({"version": 1, "polled_at": "2026-01-01T00:00:00.000Z", "channel": "C02JU6C2Z1D", "read": 10, "posts": posts});
    std::fs::write(path, cache.to_string()).unwrap();
}

/// The `review-requested` conditions of the cache, every posted pull request open on GitHub with no review.
fn reviews(
    store: &mut Store,
    home: &CpdHome,
    now: OffsetDateTime,
    numbers: &[u64],
) -> Vec<Condition> {
    let prs = numbers
        .iter()
        .map(|n| {
            let pr = GhPr {
                title: "Add retries".to_owned(),
                draft: false,
                state: GhState::Open,
                author: "ryan".to_owned(),
                decision: None,
                reviews: Vec::new(),
            };
            (("acme/pay".to_owned(), *n), Some(pr))
        })
        .collect();
    let lookup = Lookup {
        me: "clayton-duarte".to_owned(),
        prs,
    };
    let bots = ["github-actions".to_owned()];
    let mut github = |_: &[(String, u64)]| -> Result<Lookup, CpdError> { Ok(lookup.clone()) };
    let team = Team {
        bot_reviewers: &bots,
        github: &mut github,
    };
    let all = conditions_with(store, home, now, 20, 1, Some(team)).unwrap();
    all.into_iter()
        .filter(|c| c.kind == Kind::ReviewRequested)
        .collect()
}

fn subjects(conditions: &[Condition]) -> Vec<&str> {
    conditions.iter().map(|c| c.subject.as_str()).collect()
}

fn run(store: &mut Store, conditions: &[Condition], at: &str) {
    reconcile(store, conditions, at).unwrap();
}

fn sql(store: &mut Store, statement: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(statement)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn count_open(store: &mut Store) -> usize {
    store
        .read(|c| {
            list(
                c,
                &Filter {
                    context: None,
                    state: State::Open,
                },
            )
        })
        .unwrap()
        .len()
}

/// Opens the item of `#812` and sets `column` (`dismissed_at` or `claimed_at`) on it.
fn open_and_mark(store: &mut Store, home: &CpdHome, column: &str) {
    write_cache(home, NOW, &[(812, "1.1", 3600.0)]);
    let open = reviews(store, home, NOW, &[812]);
    assert_eq!(subjects(&open), ["pr:acme/pay#812"]);
    run(store, &open, "2026-01-01T00:00:00.000Z");
    assert_eq!(count_open(store), 1);
    sql(
        store,
        &format!(
            "UPDATE inbox_items SET {column} = '2026-01-01T01:00:00.000Z' WHERE subject = 'pr:acme/pay#812'"
        ),
    );
}

fn dismissed_and_closed_is_never_reopened(column: &str) {
    let (_dir, home, mut store) = store();
    open_and_mark(&mut store, &home, column);
    let now = reviews(&mut store, &home, NOW, &[812]);
    assert!(
        now.is_empty(),
        "no condition for a marked pull request: {now:?}"
    );
    run(&mut store, &now, "2026-01-01T02:00:00.000Z");
    assert_eq!(count_open(&mut store), 0, "the item closed");
    let later = reviews(&mut store, &home, NOW, &[812]);
    assert!(
        later.is_empty(),
        "the post is still there and needs review, yet no condition: {later:?}"
    );
}

#[test]
fn a_dismissed_review_request_opens_no_new_item_once_its_item_closed() {
    dismissed_and_closed_is_never_reopened("dismissed_at");
}

#[test]
fn a_claimed_review_request_opens_no_new_item_once_its_item_closed() {
    dismissed_and_closed_is_never_reopened("claimed_at");
}

#[test]
fn a_dismissed_review_request_opens_no_new_item_for_a_repost_a_new_head_or_a_later_window() {
    let (_dir, home, mut store) = store();
    open_and_mark(&mut store, &home, "dismissed_at");
    let none = reviews(&mut store, &home, NOW, &[812]);
    run(&mut store, &none, "2026-01-01T02:00:00.000Z");
    assert_eq!(count_open(&mut store), 0);

    let later = NOW + time::Duration::days(10);
    write_cache(&home, later, &[(812, "1.1", 10.0 * 86400.0 + 3600.0)]);
    assert!(
        reviews(&mut store, &home, later, &[812]).is_empty(),
        "the old post left the window"
    );

    write_cache(&home, later, &[(812, "9.9", 600.0)]);
    let repost = reviews(&mut store, &home, later, &[812]);
    assert!(
        repost.is_empty(),
        "a repost of a dismissed pull request: {repost:?}"
    );
}

#[test]
fn an_open_dismissed_review_item_closes_at_the_next_reconcile() {
    open_item_closes("dismissed_at");
}

#[test]
fn an_open_claimed_review_item_closes_at_the_next_reconcile() {
    open_item_closes("claimed_at");
}

fn open_item_closes(column: &str) {
    let (_dir, home, mut store) = store();
    open_and_mark(&mut store, &home, column);
    let next = reviews(&mut store, &home, NOW, &[812]);
    run(&mut store, &next, "2026-01-01T02:00:00.000Z");
    assert_eq!(
        count_open(&mut store),
        0,
        "the item closed with the same cache"
    );
}

#[test]
fn a_dismissal_of_one_pull_request_does_not_stop_the_review_item_of_another() {
    let (_dir, home, mut store) = store();
    write_cache(&home, NOW, &[(812, "1.1", 3600.0), (813, "1.2", 3600.0)]);
    let open = reviews(&mut store, &home, NOW, &[812, 813]);
    assert_eq!(open.len(), 2);
    run(&mut store, &open, "2026-01-01T00:00:00.000Z");
    sql(
        &mut store,
        "UPDATE inbox_items SET dismissed_at = '2026-01-01T01:00:00.000Z' WHERE subject = 'pr:acme/pay#812'",
    );
    let next = reviews(&mut store, &home, NOW, &[812, 813]);
    run(&mut store, &next, "2026-01-01T02:00:00.000Z");
    let again = reviews(&mut store, &home, NOW, &[812, 813]);
    assert_eq!(subjects(&again), ["pr:acme/pay#813"]);
}

#[test]
fn a_claimed_item_is_left_out_of_the_visible_list_and_listed_with_the_open_items() {
    let (_dir, home, mut store) = store();
    write_cache(&home, NOW, &[(812, "1.1", 3600.0)]);
    let open = reviews(&mut store, &home, NOW, &[812]);
    run(&mut store, &open, "2026-01-01T00:00:00.000Z");
    sql(
        &mut store,
        "UPDATE inbox_items SET claimed_at = '2026-01-01T01:00:00.000Z' WHERE subject = 'pr:acme/pay#812'",
    );
    let visible = store
        .read(|c| {
            list(
                c,
                &Filter {
                    context: None,
                    state: State::Visible {
                        now: "2026-01-01T02:00:00.000Z".into(),
                    },
                },
            )
        })
        .unwrap();
    assert!(visible.is_empty(), "{visible:?}");
    assert_eq!(count_open(&mut store), 1);
}
