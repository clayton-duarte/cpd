//! A clean pull request with a live Job of any Workflow kind (docs, test, impl, verify, work, open, update) is an `updating` row, and a stored blocked merge state is
//! not a `ready to merge` row. The fixture is a small copy of `pr_tests`'s (its own is private to that module).

use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Kind, Urgency};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Owner `acme`, Plan `pay` with an idle Lead, and approved, mergeable, passed pull request `acme/shop#1` owned by
/// Workflow `p1`, with `merge_state` as stored, and one Job of `kind` and `status`.
fn row(merge_state: &str, kind: &str, status: &str) -> (Urgency, String) {
    let sha = if status == "merged" {
        "'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'"
    } else {
        "NULL"
    };
    let cause = if status == "failed" {
        "'crashed'"
    } else {
        "NULL"
    };
    let (reason, requested) = if status == "cancelled" {
        ("'t'", format!("'{T}'"))
    } else {
        ("NULL", "NULL".to_owned())
    };
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', '{T}', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'deploy', 'l-pay', '{T}', '{T}');
                 INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                            mergeable, authored, review_requested, merge_state, feedback, base_ref, default_branch)
                 VALUES ('acme/shop', 1, 'https://github.com/acme/shop/pull/1', 'T', 0, 'open', 'approved', 'passed', '{T}', '{T}',
                         'mergeable', 1, 0, '{merge_state}', '[]', 'main', 'main');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pull_request, pr_number, pr_url,
                                        pr_opened_at, created_txn, created_at, started_at)
                 VALUES ('pay', 'p1', '/r', 'main', 'open', 'cpd/pay/p1', '/lw',
                         (SELECT id FROM pull_requests WHERE number = 1), 1, 'https://github.com/acme/shop/pull/1', '{T}', 1, '{T}', '{T}');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                   fingerprint, created_txn, created_at, kind, merged_sha, failed_cause, cancel_reason, cancel_requested_at, tests, test_files)
                 VALUES ('pay', 'p1', 'j1', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{FP}', 1, '{T}', '{kind}', {sha}, {cause}, {reason}, {requested}, CASE WHEN '{kind}' = 'test' THEN '[\"t\"]' END, CASE WHEN '{kind}' = 'test' THEN '[\"t\"]' END);"
            ))?;
            Ok(())
        })
        .unwrap();
    let all = conditions_with(&mut store, &home, NOW, 20, 1, None).unwrap();
    let c = all
        .into_iter()
        .find(|c| c.kind == Kind::PrReady && c.subject == "pr:acme/shop#1")
        .expect("the pr-ready row");
    let status = c
        .label
        .split('\n')
        .next()
        .unwrap()
        .strip_prefix("shop#1 ")
        .unwrap()
        .to_owned();
    (c.urgency, status)
}

fn cyan_updating(kind: &str, status: &str) {
    assert_eq!(
        row("clean", kind, status),
        (Urgency::Cyan, "updating".to_owned()),
        "{kind} {status}"
    );
}

#[test]
fn a_clean_pr_with_a_live_update_job_is_a_cyan_updating_row_not_ready_to_merge() {
    cyan_updating("update", "queued");
    cyan_updating("update", "running");
}

#[test]
fn a_clean_pr_with_a_live_fix_job_is_a_cyan_updating_row_not_ready_to_merge() {
    cyan_updating("work", "queued");
    cyan_updating("work", "running");
}

#[test]
fn a_stored_blocked_merge_state_is_not_a_ready_to_merge_row() {
    // GitHub refused a merge as blocked while an update Job runs: the stored state relabels the row, never yellow ready.
    let (urgency, status) = row("blocked", "update", "running");
    assert_ne!(status, "ready to merge");
    assert_eq!((urgency, status.as_str()), (Urgency::Cyan, "updating"));
}

#[test]
fn a_clean_pr_with_a_live_docs_test_impl_verify_or_open_job_is_a_cyan_updating_row_not_ready_to_merge()
 {
    for kind in ["docs", "test", "impl", "verify", "open"] {
        cyan_updating(kind, "queued");
        cyan_updating(kind, "running");
    }
}

#[test]
fn a_clean_pr_with_only_a_finished_job_is_not_an_updating_row() {
    for (kind, status) in [
        ("impl", "merged"),
        ("docs", "failed"),
        ("test", "cancelled"),
    ] {
        let (_, label) = row("clean", kind, status);
        assert_ne!(label, "updating", "{kind} {status}");
    }
}
