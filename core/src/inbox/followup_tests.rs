//! A follow-up `review-requested` item: the User's CHANGES_REQUESTED review, a later head with green checks.

use serde_json::json;
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Kind, reconcile};
use super::rules::{Team, conditions_with};
use super::team::{GhPr, GhState, Lookup, Review};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const ME: &str = "clayton-duarte";
const SUBJECT: &str = "pr:acme/pay#812";
const S_REV: &str = "1111111111111111111111111111111111111111";
const S_HEAD: &str = "2222222222222222222222222222222222222222";
const S_HEAD2: &str = "3333333333333333333333333333333333333333";

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn write_cache(store: &mut Store) {
    let post = json!({"repo": "acme/pay", "number": 812, "ts": NOW.unix_timestamp() as f64 - 3600.0, "ts_raw": "1.1",
                      "author": "Ryan Case", "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    crate::inbox::team::seed_cache(store, "C02JU6C2Z1D", &[post]);
}

fn pr(
    decision: Option<&str>,
    reviews: Vec<Review>,
    head: &str,
    checks: Option<&str>,
    draft: bool,
    state: GhState,
) -> GhPr {
    GhPr {
        title: "Add retries".to_owned(),
        draft,
        state,
        author: "ryan".to_owned(),
        decision: decision.map(str::to_owned),
        reviews,
        head_oid: Some(head.to_owned()),
        checks_state: checks.map(str::to_owned),
    }
}

fn review(author: &str, state: &str, oid: &str) -> Review {
    Review {
        author: author.to_owned(),
        state: state.to_owned(),
        commit_oid: Some(oid.to_owned()),
    }
}

/// The pull request before any review, at `head`.
fn initial(head: &str) -> GhPr {
    pr(
        None,
        Vec::new(),
        head,
        Some("SUCCESS"),
        false,
        GhState::Open,
    )
}

/// The User requested changes at `S_REV`; the head is `head`, the checks `checks`.
fn changed(head: &str, checks: Option<&str>) -> GhPr {
    pr(
        Some("CHANGES_REQUESTED"),
        vec![review(ME, "CHANGES_REQUESTED", S_REV)],
        head,
        checks,
        false,
        GhState::Open,
    )
}

fn followup(head: &str) -> GhPr {
    changed(head, Some("SUCCESS"))
}

fn conditions(store: &mut Store, home: &CpdHome, pr: &GhPr) -> Vec<Condition> {
    let lookup = Lookup {
        me: ME.to_owned(),
        workflows: [(("acme/pay".to_owned(), 812), Some(pr.clone()))].into(),
    };
    let bots = ["github-actions".to_owned()];
    let mut github = |_: &[(String, u64)]| -> Result<Lookup, CpdError> { Ok(lookup.clone()) };
    let team = Team {
        bot_reviewers: &bots,
        github: &mut github,
    };
    conditions_with(store, home, NOW, 20, 1, Some(team))
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == Kind::ReviewRequested)
        .collect()
}

fn subjects(conditions: &[Condition]) -> Vec<&str> {
    conditions.iter().map(|c| c.subject.as_str()).collect()
}

fn sql(store: &mut Store, statement: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(statement)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// `column` of the newest item of the subject.
fn column(store: &mut Store, column: &str) -> Option<String> {
    store
        .read(|c| {
            Ok(c.query_row(
                &format!("SELECT {column} FROM inbox_items WHERE subject = '{SUBJECT}' ORDER BY id DESC LIMIT 1"),
                [],
                |r| r.get::<_, Option<String>>(0),
            )?)
        })
        .unwrap()
}

fn open(store: &mut Store, conditions: &[Condition]) {
    reconcile(store, conditions, "2026-01-01T00:00:00.000Z").unwrap();
}

/// An initial item at `head`, claimed (`mark` = `claimed_at` or `dismissed_at`) with `oid` as its `head_oid`, then closed.
fn settle(store: &mut Store, home: &CpdHome, head: &str, mark: &str, oid: Option<&str>) {
    write_cache(store);
    let items = conditions(store, home, &initial(head));
    assert_eq!(subjects(&items), [SUBJECT]);
    open(store, &items);
    let oid = oid.map_or("NULL".to_owned(), |o| format!("'{o}'"));
    sql(
        store,
        &format!(
            "UPDATE inbox_items SET {mark} = '2026-01-01T01:00:00.000Z', head_oid = {oid} WHERE subject = '{SUBJECT}' AND closed_at IS NULL"
        ),
    );
    open(store, &[]);
}

/// The follow-up item at `S_HEAD` opens, then is marked and closed like [`settle`].
fn settle_followup(store: &mut Store, home: &CpdHome, mark: &str) {
    settle(store, home, S_REV, "claimed_at", Some(S_REV));
    let items = conditions(store, home, &followup(S_HEAD));
    assert_eq!(subjects(&items), [SUBJECT], "the follow-up item opens");
    open(store, &items);
    sql(
        store,
        &format!(
            "UPDATE inbox_items SET {mark} = '2026-01-01T02:00:00.000Z', head_oid = '{S_HEAD}' WHERE subject = '{SUBJECT}' AND closed_at IS NULL"
        ),
    );
    open(store, &[]);
}

/// After a claim of the earlier item at `oid`, the conditions of `pr`.
fn after_claim(oid: Option<&str>, pr: &GhPr) -> Vec<String> {
    let (_dir, home, mut store) = store();
    settle(&mut store, &home, S_REV, "claimed_at", oid);
    conditions(&mut store, &home, pr)
        .iter()
        .map(|c| c.subject.clone())
        .collect()
}

#[test]
fn followup_item_opens_when_changes_requested_head_moved_and_ci_green() {
    for oid in [None, Some(S_REV)] {
        assert_eq!(after_claim(oid, &followup(S_HEAD)), [SUBJECT], "{oid:?}");
    }
}

#[test]
fn followup_item_not_opened_when_head_equals_reviewed_oid() {
    assert!(after_claim(None, &followup(S_REV)).is_empty());
}

#[test]
fn followup_item_not_opened_while_ci_pending() {
    assert!(after_claim(None, &changed(S_HEAD, Some("PENDING"))).is_empty());
}

#[test]
fn followup_item_not_opened_when_ci_failing_or_missing() {
    assert!(after_claim(None, &changed(S_HEAD, Some("FAILURE"))).is_empty());
    assert!(after_claim(None, &changed(S_HEAD, None)).is_empty());
}

#[test]
fn followup_item_not_opened_for_draft_closed_or_merged_pr() {
    let with = |draft, state| {
        let base = followup(S_HEAD);
        GhPr {
            draft,
            state,
            ..base
        }
    };
    assert!(after_claim(None, &with(true, GhState::Open)).is_empty());
    assert!(after_claim(None, &with(false, GhState::Closed)).is_empty());
    assert!(after_claim(None, &with(false, GhState::Merged)).is_empty());
}

#[test]
fn followup_item_not_opened_for_another_reviewers_changes_requested() {
    let other = pr(
        Some("CHANGES_REQUESTED"),
        vec![review("maria", "CHANGES_REQUESTED", S_REV)],
        S_HEAD,
        Some("SUCCESS"),
        false,
        GhState::Open,
    );
    assert!(after_claim(None, &other).is_empty());
}

#[test]
fn followup_item_stays_closed_after_claim_of_same_head() {
    let (_dir, home, mut store) = store();
    settle_followup(&mut store, &home, "claimed_at");
    assert!(conditions(&mut store, &home, &followup(S_HEAD)).is_empty());
}

#[test]
fn followup_item_stays_closed_after_dismissal_of_same_head() {
    let (_dir, home, mut store) = store();
    settle_followup(&mut store, &home, "dismissed_at");
    assert!(conditions(&mut store, &home, &followup(S_HEAD)).is_empty());
}

#[test]
fn followup_item_reopens_for_a_later_head() {
    let (_dir, home, mut store) = store();
    settle_followup(&mut store, &home, "claimed_at");
    let items = conditions(&mut store, &home, &followup(S_HEAD2));
    assert_eq!(subjects(&items), [SUBJECT]);
}

#[test]
fn initial_review_item_records_head_oid() {
    let (_dir, home, mut store) = store();
    write_cache(&mut store);
    let items = conditions(&mut store, &home, &initial(S_HEAD));
    assert_eq!(subjects(&items), [SUBJECT]);
    open(&mut store, &items);
    assert_eq!(column(&mut store, "head_oid").as_deref(), Some(S_HEAD));
    assert_eq!(column(&mut store, "reviewed_oid"), None);
}

#[test]
fn legacy_claim_without_head_oid_still_suppresses_initial_item() {
    let (_dir, home, mut store) = store();
    settle(&mut store, &home, S_HEAD, "claimed_at", None);
    assert!(conditions(&mut store, &home, &initial(S_HEAD2)).is_empty());
}

#[test]
fn followup_item_carries_reviewed_oid() {
    let (_dir, home, mut store) = store();
    settle(&mut store, &home, S_REV, "claimed_at", None);
    let items = conditions(&mut store, &home, &followup(S_HEAD));
    assert_eq!(subjects(&items), [SUBJECT]);
    open(&mut store, &items);
    assert_eq!(column(&mut store, "reviewed_oid").as_deref(), Some(S_REV));
    assert_eq!(column(&mut store, "head_oid").as_deref(), Some(S_HEAD));
}
