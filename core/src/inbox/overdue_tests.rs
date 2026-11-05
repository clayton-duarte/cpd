//! The overdue label of an authored pull request's row (contract §2.2 "The authored pull request rule", OVERDUE; pro-d386,
//! pro-d388): a posted, unreviewed pull request whose `COALESCE(last_posted_at, posted_at)` is 86 400 s old reads
//! `review overdue` in place of `waiting for peer review`. Same kind, subject and urgency; nothing is sent.

use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Kind, Urgency};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2025-12-01T00:00:00.000Z";
/// More than 24 h before `NOW`.
const OLD: &str = "2025-12-30T00:00:00.000Z";
/// Less than 24 h before `NOW`.
const YOUNG: &str = "2026-01-01T12:00:00.000Z";

const IN_REVIEW: &str = "shop#1 waiting for peer review\nT";
const OVERDUE: &str = "shop#1 review overdue\nT";

struct Fx {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

/// Plan `pay` of Context `ctx`; the open authored pull request `acme/shop#1` (checks passed, not clean, so never
/// `ready to merge`) owned by Workflow `p1` of `pay`, posted with `posted_at` and `last_posted_at`.
fn fixture(posted_at: &str, last_posted_at: Option<&str>, pull_assignments: &str) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fx {
        _dir: dir,
        home,
        store,
    };
    let last = last_posted_at.map_or("NULL".to_owned(), |l| format!("'{l}'"));
    f.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'l-pay', '{T}', '{T}');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    mergeable, authored, review_requested, merge_state, feedback, base_ref, default_branch)
           VALUES ('acme/shop', 1, 'https://github.com/acme/shop/pull/1', 'T', 0, 'open', 'none', 'passed', '{T}', '{T}',
                   'mergeable', 1, 0, 'blocked', '[]', 'main', 'main');
         UPDATE pull_requests SET {pull_assignments} WHERE number = 1;
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pull_request, pr_number, pr_url,
                                pr_opened_at, created_txn, created_at, started_at)
           VALUES ('pay', 'p1', '/r', 'main', 'open', 'cpd/pay/p1', '/lw',
                   (SELECT id FROM pull_requests WHERE number = 1), 1, 'https://github.com/acme/shop/pull/1', '{T}', 1, '{T}', '{T}');
         INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at)
           VALUES ('pay', 'p1', 'C1', '1.1', 'https://x/1', '{posted_at}', {last});"
    ));
    f
}

impl Fx {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// The one `pr-ready` row of pull request 1.
    fn row(&mut self) -> Condition {
        let all = conditions_with(&mut self.store, &self.home, NOW, 20, 1, None).unwrap();
        let mut rows: Vec<Condition> = all
            .into_iter()
            .filter(|c| c.kind == Kind::PrReady)
            .collect();
        assert_eq!(rows.len(), 1, "{rows:?}");
        rows.remove(0)
    }
}

#[test]
fn a_ready_pr_with_an_unreviewed_post_older_than_24h_is_labeled_review_overdue() {
    let mut f = fixture(OLD, None, "title = title");
    let c = f.row();
    assert_eq!(c.label, OVERDUE);
    // Not a new kind, subject or colour: only the tail of Row 1 changes.
    assert_eq!(c.kind, Kind::PrReady);
    assert_eq!(c.subject, "pr:acme/shop#1");
    assert_eq!(c.urgency, Urgency::White);
}

#[test]
fn a_post_younger_than_24h_keeps_the_ready_for_review_label() {
    let mut f = fixture(YOUNG, None, "title = title");
    let c = f.row();
    assert_eq!(c.label, IN_REVIEW);
    assert_eq!(c.urgency, Urgency::White);
    // The same row turns overdue once the post is old enough.
    f.sql("UPDATE pr_posts SET posted_at = '2025-12-30T00:00:00.000Z'");
    assert_eq!(f.row().label, OVERDUE);
}

#[test]
fn a_reviewed_draft_or_closed_pr_is_never_overdue() {
    for (what, set, label) in [
        ("approved", "review_decision = 'approved'", None),
        (
            "changes requested",
            "review_decision = 'changes_requested'",
            None,
        ),
        ("draft", "draft = 1, merge_state = 'draft'", None),
        ("closed", "state = 'closed'", Some(())),
    ] {
        let mut f = fixture(OLD, None, set);
        if label.is_some() {
            // A closed pull request has no `pr-ready` row at all.
            let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, None).unwrap();
            assert!(all.iter().all(|c| c.kind != Kind::PrReady), "{what}");
        } else {
            let c = f.row();
            assert!(!c.label.contains("overdue"), "{what}: {}", c.label);
        }
    }
    // Reviewed: the row was `review overdue` while unreviewed and is not once approved.
    let mut f = fixture(OLD, None, "title = title");
    assert_eq!(f.row().label, OVERDUE);
    f.sql("UPDATE pull_requests SET review_decision = 'approved' WHERE number = 1");
    assert!(!f.row().label.contains("overdue"));
}

#[test]
fn the_overdue_condition_uses_last_posted_at_and_falls_back_to_posted_at() {
    // last_posted_at wins over an old posted_at: a recent repost clears the label.
    let mut f = fixture(OLD, Some(YOUNG), "title = title");
    assert_eq!(f.row().label, IN_REVIEW);
    // ... and an old last_posted_at keeps it, whatever posted_at is.
    let mut f = fixture(YOUNG, Some(OLD), "title = title");
    assert_eq!(f.row().label, OVERDUE);
    // NULL last_posted_at falls back to posted_at.
    let mut f = fixture(OLD, None, "title = title");
    assert_eq!(f.row().label, OVERDUE);
    let mut f = fixture(YOUNG, None, "title = title");
    assert_eq!(f.row().label, IN_REVIEW);
}
