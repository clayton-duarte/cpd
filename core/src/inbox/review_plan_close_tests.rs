//! pro-d416 (A3): `review_plan::finish_closed` finishes an open `codereview` Plan whose pull request is closed, merged or
//! approved by the viewer, through the same code as `review submit`. The lookup is a parsed value: no `gh` runs.

use super::review_plan::finish_closed;
use super::team::{GhPr, GhState, Lookup, Review};
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::store::Store;
use std::collections::BTreeMap;

const ME: &str = "clayton-duarte";
const PLAN: &str = "r-pay-812";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

fn sql(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
}

fn text(store: &mut Store, sql: &str) -> String {
    store
        .read(|c: &rusqlite::Connection| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// An open `codereview` Plan `r-pay-812` in `grounding`, its `pr_subject` as given (SQL: a quoted text or `NULL`).
fn plan(store: &mut Store, pr_subject: &str) {
    sql(
        store,
        &format!(
            "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Code review', 't', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l1', 'lead', 'codereview', '{PLAN}', 'L1', '/', 'S3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, pr_subject)
               VALUES ('{PLAN}', 'codereview', 'Review', 'grounding', 'l1', 't', {pr_subject});"
        ),
    );
}

fn pr(state: GhState, decision: Option<&str>, reviews: &[(&str, &str)]) -> GhPr {
    GhPr {
        title: "Add retries".to_owned(),
        draft: false,
        state,
        author: "ryan".to_owned(),
        decision: decision.map(str::to_owned),
        reviews: reviews
            .iter()
            .map(|(author, state)| Review {
                author: (*author).to_owned(),
                state: (*state).to_owned(),
                commit_oid: None,
            })
            .collect(),
        head_oid: None,
        checks_state: None,
    }
}

fn lookup(pr: GhPr) -> Lookup {
    Lookup {
        me: ME.to_owned(),
        workflows: BTreeMap::from([(("acme/pay".to_owned(), 812), Some(pr))]),
    }
}

fn phase(store: &mut Store) -> String {
    text(
        store,
        &format!("SELECT phase FROM plans WHERE id = '{PLAN}'"),
    )
}

fn done_events(store: &mut Store) -> i64 {
    store
        .read(|c: &rusqlite::Connection| {
            Ok(c.query_row(
                "SELECT count(*) FROM events WHERE kind = 'plan-done'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn assert_finished(state: GhState, decision: Option<&str>, reviews: &[(&str, &str)]) {
    let (_dir, mut store) = store();
    plan(&mut store, "'pr:acme/pay#812'");
    let done = finish_closed(&mut store, &lookup(pr(state, decision, reviews))).unwrap();
    assert_eq!(
        done,
        [PlanId::parse(PLAN).unwrap()],
        "the finished Plan ids"
    );
    assert_eq!(phase(&mut store), "done");
    let (locked, done_at) = (
        text(
            &mut store,
            &format!("SELECT locked_at FROM plans WHERE id = '{PLAN}'"),
        ),
        text(
            &mut store,
            &format!("SELECT done_at FROM plans WHERE id = '{PLAN}'"),
        ),
    );
    assert_eq!(
        locked, done_at,
        "locked_at = done_at = the transaction time"
    );
    assert_eq!(done_events(&mut store), 1, "one plan-done Event");
}

#[test]
fn closed_pr_finishes_open_codereview_plan() {
    assert_finished(GhState::Closed, None, &[]);
}

#[test]
fn merged_pr_finishes_open_codereview_plan() {
    assert_finished(GhState::Merged, None, &[]);
}

#[test]
fn viewer_approved_pr_finishes_open_codereview_plan() {
    // The viewer's own latest review is APPROVED; another reviewer's changes request does not matter.
    assert_finished(
        GhState::Open,
        Some("CHANGES_REQUESTED"),
        &[(ME, "APPROVED"), ("ryan", "CHANGES_REQUESTED")],
    );
}

#[test]
fn open_unapproved_pr_leaves_codereview_plan_open() {
    let (_dir, mut store) = store();
    plan(&mut store, "'pr:acme/pay#812'");
    // Open; the viewer asked for changes, another reviewer approved: neither finishes the Plan.
    let l = lookup(pr(
        GhState::Open,
        Some("APPROVED"),
        &[(ME, "CHANGES_REQUESTED"), ("ryan", "APPROVED")],
    ));
    assert!(finish_closed(&mut store, &l).unwrap().is_empty());
    assert_eq!(phase(&mut store), "grounding");
    assert_eq!(done_events(&mut store), 0);
}

#[test]
fn plan_without_pr_subject_is_never_auto_finished() {
    let (_dir, mut store) = store();
    plan(&mut store, "NULL");
    assert!(
        finish_closed(&mut store, &lookup(pr(GhState::Merged, None, &[])))
            .unwrap()
            .is_empty()
    );
    assert_eq!(phase(&mut store), "grounding");
    assert_eq!(done_events(&mut store), 0);
}
