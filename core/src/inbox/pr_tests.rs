//! The authored pull request rule (contract §2.2 "The authored pull request rule"): one `pr-ready` condition per
//! open authored pull request, its urgency and label by the 8-row STATUS table (pro-d130, pro-d131).

use serde_json::json;
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Kind, Urgency};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2026-01-01T00:00:00.000Z";
const T2: &str = "2026-01-01T06:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const POST: &str = "[pr.post]\nenabled = true\n";

use Urgency::{White, Red, Cyan, Yellow};

pub(super) struct Fx {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

/// A Context `ctx` with owner `acme`, a Plan `pay` and its Lead `l-pay` (working).
pub(super) fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fx {
        _dir: dir,
        home,
        store,
    };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'l-pay', '{T}', '{T}');"
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

    /// The owner's policy file `policy/acme/owner.toml`.
    fn policy(&mut self, toml: &str) {
        let dir = self.home.root().join("policy/acme");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("owner.toml"), toml).unwrap();
    }

    /// An open authored pull request `acme/shop#n`: passed checks, clean, no review, comments read and none, on main.
    pub(super) fn workflow(&mut self, n: i64, assignments: &str) {
        let url = format!("https://github.com/acme/shop/pull/{n}");
        self.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                        mergeable, authored, review_requested, merge_state, feedback, base_ref, default_branch)
             VALUES ('acme/shop', {n}, '{url}', 'T', 0, 'open', 'none', 'passed', '{T}', '{T}',
                     'mergeable', 1, 0, 'clean', '[]', 'main', 'main');
             UPDATE pull_requests SET {assignments} WHERE number = {n};"
        ));
    }

    /// Plan `plan` owns pull request `n` through a `prs` row.
    fn own(&mut self, plan: &str, n: i64) {
        self.sql(&format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pull_request, pr_number, pr_url,
                              pr_opened_at, created_txn, created_at, started_at)
             VALUES ('{plan}', 'p{n}', '/r', 'main', 'open', 'cpd/{plan}/p{n}', '/lw',
                     (SELECT id FROM pull_requests WHERE number = {n}), {n}, 'https://github.com/acme/shop/pull/{n}', '{T}', 1, '{T}', '{T}');"
        ));
    }

    fn idle(&mut self) {
        self.sql("UPDATE sessions SET state = 'idle' WHERE id = 'l-pay'");
    }

    /// A Job of `kind` and `status` of the Workflow `p<n>` of Plan `pay`.
    fn job(&mut self, n: i64, key: &str, kind: &str, status: &str) {
        let cause = if status == "failed" {
            "'crashed'"
        } else {
            "NULL"
        };
        let sha = if status == "merged" {
            "'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'"
        } else {
            "NULL"
        };
        let (reason, requested) = if status == "cancelled" {
            ("'t'", format!("'{T}'"))
        } else {
            ("NULL", "NULL".to_owned())
        };
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, failed_cause, kind, merged_sha, cancel_reason, cancel_requested_at)
             VALUES ('pay', 'p{n}', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{FP}', 1, '{T}', {cause}, '{kind}', {sha}, {reason}, {requested});"
        ));
    }

    fn posted(&mut self, n: i64) {
        self.sql(&format!(
            "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at) VALUES ('pay', 'p{n}', 'C1', '1.1', 'https://x/1', '{T}');"
        ));
    }

    fn rows(&mut self) -> Vec<Condition> {
        let all = conditions_with(&mut self.store, &self.home, NOW, 20, 1, None).unwrap();
        all.into_iter()
            .filter(|c| c.kind == Kind::PrReady)
            .collect()
    }

    /// The one row of pull request `n`.
    fn row(&mut self, n: i64) -> Condition {
        let subject = format!("pr:acme/shop#{n}");
        let mut found: Vec<Condition> = self
            .rows()
            .into_iter()
            .filter(|c| c.subject == subject)
            .collect();
        assert_eq!(found.len(), 1, "{subject}: {found:?}");
        found.remove(0)
    }

    /// Urgency and status of the row of pull request `n`; the label is `shop#<n> <status>`.
    fn status(&mut self, n: i64) -> (Urgency, String) {
        let c = self.row(n);
        let row1 = c.label.split('\n').next().unwrap();
        let status = row1
            .strip_prefix(&format!("shop#{n} "))
            .unwrap_or_else(|| panic!("label {:?}", c.label))
            .to_owned();
        (c.urgency, status)
    }
}

pub(super) fn is(f: &mut Fx, n: i64, urgency: Urgency, status: &str) {
    assert_eq!(
        f.status(n),
        (urgency, status.to_owned()),
        "pull request {n}"
    );
}

fn bot_comment() -> String {
    json!([{"id": "IC_1", "kind": "comment", "last": "IC_1", "author": "bot", "bot": true, "path": null,
            "outdated": false, "url": "https://github.com/acme/shop/pull/1#IC_1", "excerpt": "x"}])
    .to_string()
}

#[test]
fn an_authored_draft_pr_is_a_white_draft_row() {
    let mut f = fixture();
    f.workflow(3, "draft = 1, merge_state = 'draft'");
    let c = f.row(3);
    assert_eq!((c.urgency, c.label.as_str()), (Cyan, "shop#3 draft\nT"));
    assert_eq!(c.subject, "pr:acme/shop#3");
    assert_eq!(
        c.url.as_deref(),
        Some("https://github.com/acme/shop/pull/3")
    );
}

#[test]
fn an_authored_stacked_pr_is_a_white_stacked_row() {
    let mut f = fixture();
    f.workflow(
        1,
        "base_ref = 'feature', default_branch = 'main', review_decision = 'approved'",
    );
    f.workflow(2, "base_ref = NULL, default_branch = NULL");
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
    is(&mut f, 2, White, "waiting");
}

#[test]
fn an_unowned_broken_pr_is_a_red_row_named_by_its_cause() {
    let mut f = fixture();
    f.workflow(1, "mergeable = 'conflicting', merge_state = 'dirty'");
    f.workflow(2, "checks = 'failed', merge_state = 'blocked'");
    f.workflow(
        3,
        "review_decision = 'changes_requested', merge_state = 'blocked'",
    );
    is(&mut f, 1, Red, "conflict");
    is(&mut f, 2, Red, "failing");
    is(&mut f, 3, Red, "changes requested");
}

#[test]
fn a_broken_pr_shows_its_causes_in_the_order_conflict_failing_changes_requested() {
    let mut f = fixture();
    f.workflow(1, "mergeable = 'conflicting', merge_state = 'dirty', checks = 'failed', review_decision = 'changes_requested'");
    f.workflow(
        2,
        "merge_state = 'blocked', checks = 'failed', review_decision = 'changes_requested'",
    );
    is(&mut f, 1, Red, "conflict");
    is(&mut f, 2, Red, "failing");
}

/// A Job as (kind, status).
type JobKindStatus<'a> = (&'a str, &'a str);

#[test]
fn an_owned_broken_pr_is_cyan_while_its_lead_works_or_an_update_or_work_job_is_live() {
    let broken = [
        (
            "mergeable = 'conflicting', merge_state = 'dirty'",
            "conflict",
            "updating",
        ),
        (
            "checks = 'failed', merge_state = 'blocked'",
            "failing",
            "fixing",
        ),
        (
            "review_decision = 'changes_requested', merge_state = 'blocked'",
            "changes requested",
            "fixing",
        ),
    ];
    // (Lead working, a Job of (kind, status), handled)
    let cases: [(bool, Option<JobKindStatus>, bool); 10] = [
        (true, None, true),
        (false, Some(("update", "queued")), true),
        (false, Some(("update", "running")), true),
        (false, Some(("work", "running")), true),
        (false, Some(("work", "queued")), true),
        (false, Some(("update", "merged")), false),
        (false, Some(("update", "failed")), false),
        (false, Some(("update", "cancelled")), false),
        (false, Some(("docs", "queued")), false),
        (false, Some(("impl", "queued")), false),
    ];
    for (set, red, white) in broken {
        for (working, job, handled) in cases {
            let mut f = fixture();
            f.workflow(1, set);
            f.own("pay", 1);
            if !working {
                f.idle();
            }
            if let Some((kind, status)) = job {
                f.job(1, "j1", kind, status);
            }
            let expected = if handled { (White, white) } else { (Red, red) };
            is(&mut f, 1, expected.0, expected.1);
        }
        let mut f = fixture();
        f.workflow(1, set);
        f.own("pay", 1);
        f.idle();
        is(&mut f, 1, Red, red);
        let mut f = fixture();
        f.workflow(1, set);
        is(&mut f, 1, Red, red); // unowned
    }
}

#[test]
fn a_pr_with_pending_checks_is_a_cyan_checks_running_row() {
    let mut f = fixture();
    f.workflow(1, "checks = 'pending'");
    f.workflow(2, "checks = 'none'");
    is(&mut f, 1, White, "checks running");
    is(&mut f, 2, Yellow, "ready to merge");
}

#[test]
fn an_unposted_owned_postable_pr_of_a_post_owner_is_a_yellow_ready_for_review_row() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, "checks = 'passed'");
    f.own("pay", 1);
    let c = f.row(1);
    assert_eq!(
        (c.urgency, c.label.as_str()),
        (Yellow, "shop#1 ready for user review\nT")
    );
    assert_eq!(c.plan.as_ref().map(|p| p.as_str()), Some("pay"));
    assert_eq!(c.context.as_ref().map(|p| p.as_str()), Some("ctx"));
    assert_eq!(c.session.as_ref().map(|p| p.as_str()), Some("l-pay"));
}

#[test]
fn ready_for_review_needs_every_condition() {
    // Baseline: it is ready for review.
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, "checks = 'passed'");
    f.own("pay", 1);
    is(&mut f, 1, Yellow, "ready for user review");

    // A post.
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, "checks = 'passed'");
    f.own("pay", 1);
    f.posted(1);
    is(&mut f, 1, Yellow, "ready to merge");

    // No owner Plan.
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, "checks = 'passed'");
    is(&mut f, 1, Yellow, "ready to merge");

    // The policy disabled, and without [pr.post].
    for toml in ["[pr.post]\nenabled = false\n", ""] {
        let mut f = fixture();
        f.policy(toml);
        f.workflow(1, "checks = 'passed'");
        f.own("pay", 1);
        is(&mut f, 1, Yellow, "ready to merge");
    }

    // Not postable: comments unread, an open bot comment, no green checks.
    for set in [
        "feedback = NULL".to_owned(),
        format!("feedback = '{}'", bot_comment()),
        "checks = 'none'".to_owned(),
    ] {
        let mut f = fixture();
        f.policy(POST);
        f.workflow(1, "checks = 'passed'");
        f.sql(&format!("UPDATE pull_requests SET {set} WHERE number = 1"));
        f.own("pay", 1);
        is(&mut f, 1, White, "waiting");
    }
}

#[test]
fn ready_to_merge_for_a_post_owner_needs_a_post_or_an_approval() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, "feedback = NULL");
    f.own("pay", 1);
    is(&mut f, 1, White, "waiting");

    f.workflow(2, "feedback = NULL, review_decision = 'approved'");
    f.own("pay", 2);
    is(&mut f, 2, Yellow, "ready to merge");

    f.workflow(3, "feedback = NULL");
    f.own("pay", 3);
    f.posted(3);
    is(&mut f, 3, Yellow, "ready to merge");

    // A non-post owner, and no owner.
    for toml in ["", "[pr.post]\nenabled = false\n"] {
        let mut f = fixture();
        f.policy(toml);
        for (n, review) in [(1, "none"), (2, "approved")] {
            f.workflow(n, &format!("feedback = NULL, review_decision = '{review}'"));
            f.own("pay", n);
            is(&mut f, n, Yellow, "ready to merge");
        }
        for (n, review) in [(3, "none"), (4, "approved")] {
            f.workflow(n, &format!("feedback = NULL, review_decision = '{review}'"));
            is(&mut f, n, Yellow, "ready to merge");
        }
    }
}

#[test]
fn a_posted_pr_that_is_not_ready_to_merge_is_a_cyan_in_review_row() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, "merge_state = 'blocked', checks = 'passed'");
    f.own("pay", 1);
    f.posted(1);
    is(&mut f, 1, White, "waiting for peer review");
}

#[test]
fn an_authored_pr_that_matches_no_other_state_is_a_cyan_waiting_row() {
    let mut f = fixture();
    for (n, state) in [(1, "blocked"), (2, "behind"), (3, "unknown")] {
        f.workflow(n, &format!("checks = 'passed', merge_state = '{state}'"));
        is(&mut f, n, White, "waiting");
    }
}

#[test]
fn the_states_take_the_documented_precedence() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(
        1,
        "draft = 1, merge_state = 'draft', mergeable = 'conflicting'",
    );
    f.workflow(
        2,
        "mergeable = 'conflicting', merge_state = 'dirty', base_ref = 'feature'",
    );
    f.workflow(3, "base_ref = 'feature', checks = 'pending'");
    f.workflow(4, "checks = 'pending', review_decision = 'approved'");
    f.workflow(5, "review_decision = 'approved'");
    f.own("pay", 5);
    is(&mut f, 1, Cyan, "draft");
    is(&mut f, 2, Red, "conflict");
    is(&mut f, 3, White, "checks running");
    is(&mut f, 4, White, "checks running");
    is(&mut f, 5, Yellow, "ready for user review");
}

#[test]
fn an_unresolved_owner_policy_counts_as_post_disabled_and_fails_nothing() {
    // owner_unset
    let mut f = fixture();
    f.sql("UPDATE contexts SET owner = NULL");
    f.workflow(1, "checks = 'passed'");
    f.workflow(2, "merge_state = 'blocked', checks = 'passed'");
    f.own("pay", 1);
    f.own("pay", 2);
    is(&mut f, 1, Yellow, "ready to merge");
    is(&mut f, 2, White, "waiting");
    // policy_invalid
    let mut f = fixture();
    f.policy("[pr.post\nenabled = ");
    f.workflow(1, "checks = 'passed'");
    f.workflow(2, "merge_state = 'blocked', checks = 'passed'");
    f.own("pay", 1);
    f.own("pay", 2);
    is(&mut f, 1, Yellow, "ready to merge");
    is(&mut f, 2, White, "waiting");
}

#[test]
fn every_row_links_its_owner_and_carries_the_pull_request_url() {
    let mut f = fixture();
    f.workflow(1, "mergeable = 'conflicting', merge_state = 'dirty'");
    f.workflow(2, "mergeable = 'conflicting', merge_state = 'dirty'");
    f.workflow(3, "mergeable = 'conflicting', merge_state = 'dirty'");
    f.own("pay", 1);
    f.idle();
    f.own("pay", 2);
    f.sql("UPDATE sessions SET state = 'working' WHERE id = 'l-pay'");
    let red = f.row(1);
    f.idle();
    let red_after = f.row(1);
    assert_eq!((red.urgency, red_after.urgency), (White, Red)); // the Lead working, then idle
    for (c, n) in [(red_after, 1), (f.row(2), 2)] {
        assert_eq!(
            c.url.as_deref(),
            Some(format!("https://github.com/acme/shop/pull/{n}").as_str())
        );
        assert_eq!(c.plan.as_ref().map(|p| p.as_str()), Some("pay"));
        assert_eq!(c.context.as_ref().map(|p| p.as_str()), Some("ctx"));
        assert_eq!(c.session.as_ref().map(|p| p.as_str()), Some("l-pay"));
    }
    let c = f.row(3);
    assert_eq!(
        c.url.as_deref(),
        Some("https://github.com/acme/shop/pull/3")
    );
    assert_eq!((c.plan, c.context, c.session), (None, None, None));
}

#[test]
fn the_newest_owning_plan_decides_the_owner_and_a_pr_has_one_row() {
    for (pay_works, expected) in [(true, (Red, "conflict")), (false, (White, "updating"))] {
        let mut f = fixture();
        f.sql(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay2', 'lead', 'ctx', 'pay2', 'Lead2', '/', 'O3', '{}', '{T2}', '{T2}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay2', 'ctx', 'Pay2', 'deploy', 'l-pay2', '{T2}', '{T2}');
             UPDATE sessions SET state = '{}' WHERE id = 'l-pay';",
            if pay_works { "idle" } else { "working" },
            if pay_works { "working" } else { "idle" },
        ));
        f.workflow(1, "mergeable = 'conflicting', merge_state = 'dirty'");
        f.own("pay", 1);
        f.own("pay2", 1);
        is(&mut f, 1, expected.0, expected.1);
        let c = f.row(1);
        assert_eq!(c.plan.as_ref().map(|p| p.as_str()), Some("pay2"));
        assert_eq!(c.session.as_ref().map(|p| p.as_str()), Some("l-pay2"));
        assert_eq!(f.rows().len(), 1);
    }
}

#[test]
fn a_pr_that_is_not_open_or_not_authored_has_no_row() {
    let mut f = fixture();
    f.workflow(1, "authored = 0");
    f.workflow(
        2,
        "state = 'merged', merged_at = '2026-01-01T00:00:00.000Z'",
    );
    f.workflow(3, "state = 'closed'");
    assert!(f.rows().is_empty(), "{:?}", f.rows());
}

#[test]
fn a_clean_approved_unowned_pr_is_a_yellow_ready_to_merge_row() {
    let mut f = fixture();
    f.workflow(1, "review_decision = 'approved'");
    is(&mut f, 1, Yellow, "ready to merge");
}

const STACKED: &str = "base_ref = 'feature', default_branch = 'main'";

#[test]
fn stack_review_a_stacked_pr_with_pending_checks_is_checks_running() {
    let mut f = fixture();
    f.workflow(1, &format!("{STACKED}, checks = 'pending'"));
    is(&mut f, 1, White, "checks running");
}

#[test]
fn stack_review_a_stacked_postable_pr_is_ready_for_review() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, &format!("{STACKED}, checks = 'passed'"));
    f.own("pay", 1);
    is(&mut f, 1, Yellow, "ready for user review");
    // With an open bot comment it is not postable, so it stays stacked.
    f.sql(&format!(
        "UPDATE pull_requests SET feedback = '{}' WHERE number = 1",
        bot_comment()
    ));
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
}

#[test]
fn stack_review_a_posted_stacked_pr_is_in_review() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(
        1,
        &format!("{STACKED}, checks = 'passed', review_decision = 'none'"),
    );
    f.own("pay", 1);
    f.posted(1);
    is(&mut f, 1, White, "waiting for peer review");
}

#[test]
fn stack_review_a_stacked_pr_walks_checks_running_ready_for_review_in_review_stacked() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, &format!("{STACKED}, checks = 'pending'"));
    f.own("pay", 1);
    is(&mut f, 1, White, "checks running");
    f.sql("UPDATE pull_requests SET checks = 'passed' WHERE number = 1");
    is(&mut f, 1, Yellow, "ready for user review");
    f.posted(1);
    is(&mut f, 1, White, "waiting for peer review");
    f.sql("UPDATE pull_requests SET review_decision = 'approved' WHERE number = 1");
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
}

#[test]
fn stack_review_a_posted_approved_stacked_pr_is_stacked() {
    let mut f = fixture();
    f.policy(POST);
    f.workflow(
        1,
        &format!("{STACKED}, checks = 'passed', review_decision = 'approved'"),
    );
    f.own("pay", 1);
    f.posted(1);
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
}

#[test]
fn stack_review_an_unposted_stacked_pr_that_is_not_ready_for_review_is_stacked() {
    // Not owned.
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, &format!("{STACKED}, checks = 'passed'"));
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
    // Post disabled.
    let mut f = fixture();
    f.policy("[pr.post]\nenabled = false\n");
    f.workflow(1, &format!("{STACKED}, checks = 'passed'"));
    f.own("pay", 1);
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
    // Comments unread.
    let mut f = fixture();
    f.policy(POST);
    f.workflow(1, &format!("{STACKED}, checks = 'passed', feedback = NULL"));
    f.own("pay", 1);
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
}

#[test]
fn stack_review_stacking_blocks_only_ready_to_merge() {
    let mut f = fixture();
    f.workflow(1, &format!("{STACKED}, review_decision = 'approved'"));
    f.workflow(2, "review_decision = 'approved'");
    is(&mut f, 1, Cyan, "stacked pr pending base merge");
    is(&mut f, 2, Yellow, "ready to merge");
}
