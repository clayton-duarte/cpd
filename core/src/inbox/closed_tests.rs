//! The `pr-closed` Inbox kind: an `open` Workflow of cpd whose stored pull request is `closed` (never merged).

use super::model::{Item, Kind, Urgency, reconcile};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;
use time::macros::datetime;

const T: &str = "2026-01-01T00:00:00.000Z";

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

/// Context `ctx` with Plan `pay` and its Lead `l-pay`, and the pull request `acme/api#7` in `state`.
fn fixture(state: &str) -> Fixture {
    fixture_of(state, "open")
}

/// [`fixture`] with the Workflow `api` in `status` (`open`, `merged` or `cancelled`).
fn fixture_of(state: &str, status: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture {
        _dir: dir,
        home,
        store,
    };
    let merged = if state == "merged" {
        format!("'{T}'")
    } else {
        "NULL".to_owned()
    };
    let pr_merged = if status == "merged" {
        format!("'{T}'")
    } else {
        "NULL".to_owned()
    };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T}', '{T}');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Title 7', 0, '{state}', 'none', 'none', '{T}', {merged}, '{T}');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at, merged_at)
           VALUES ('pay', 'api', '/r/acme-api', 'main', '{}', 'cpd/pay/api', '/lw/pay/api', 7,
                   'https://github.com/acme/api/pull/7', '{T}', (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), '{T}', '{T}', {pr_merged});",
        if status == "cancelled" { "open" } else { status }
    ));
    if status == "cancelled" {
        f.cancel();
    }
    f
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// `workflow cancel` of the open Workflow: the status is all the rule may read.
    fn cancel(&mut self) {
        self.sql("PRAGMA ignore_check_constraints = ON; UPDATE workflows SET status = 'cancelled' WHERE key = 'api'; PRAGMA ignore_check_constraints = OFF;");
    }

    /// One tick: the rules, then the reconcile. The open items of kind `pr-closed` afterwards.
    fn tick(&mut self) -> Vec<Item> {
        let found = conditions_with(
            &mut self.store,
            &self.home,
            datetime!(2026-01-02 00:00 UTC),
            20,
            1,
            None,
        )
        .unwrap();
        reconcile(&mut self.store, &found, T).unwrap();
        self.store
            .read(super::model::open_items)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == Kind::PrClosed)
            .collect()
    }
}

#[test]
fn an_open_pr_whose_pull_request_is_closed_opens_a_red_pr_closed_item() {
    let mut f = fixture("closed");
    let items = f.tick();
    let [item] = &items[..] else {
        panic!("one pr-closed item: {items:?}")
    };
    assert_eq!(item.urgency, Urgency::Red);
    assert_eq!(item.subject, "pr:acme/api#7");
    assert_eq!(item.label, "api#7 closed without a merge\nTitle 7");
    assert_eq!(
        item.url.as_deref(),
        Some("https://github.com/acme/api/pull/7")
    );
    assert_eq!(item.plan.as_ref().map(|p| p.as_str()), Some("pay"));
    assert_eq!(item.context.as_ref().map(|c| c.as_str()), Some("ctx"));
    assert_eq!(item.session.as_ref().map(|s| s.as_str()), Some("l-pay"));
}

#[test]
fn the_pr_closed_item_closes_when_the_pr_is_cancelled() {
    let mut f = fixture("closed");
    assert_eq!(f.tick().len(), 1, "the item opens first");
    f.cancel();
    assert!(f.tick().is_empty());
}

#[test]
fn the_pr_closed_item_closes_when_the_pull_request_is_reopened() {
    let mut f = fixture("closed");
    assert_eq!(f.tick().len(), 1, "the item opens first");
    f.sql("UPDATE pull_requests SET state = 'open' WHERE number = 7");
    assert!(f.tick().is_empty());
}

#[test]
fn a_closed_pull_request_of_a_pr_that_is_not_open_makes_no_item() {
    for status in ["merged", "cancelled"] {
        let mut f = fixture_of("closed", status);
        assert!(f.tick().is_empty(), "{status}");
    }
}

#[test]
fn the_pr_closed_item_is_the_same_item_after_two_reconcile_runs() {
    let mut f = fixture("closed");
    let first = f.tick();
    let second = f.tick();
    assert_eq!(first.len(), 1, "the item opens");
    assert_eq!(
        first.iter().map(|i| i.id).collect::<Vec<_>>(),
        second.iter().map(|i| i.id).collect::<Vec<_>>()
    );
}
