//! The authored pull request rule for a reopened pull request (pro-d362): its only Workflow is `cancelled` with the pull request
//! kept, so the `pr-ready` item still names its Context, Plan and Lead Session.

use time::macros::datetime;

use super::model::Kind;
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const T: &str = "2026-01-01T00:00:00.000Z";

#[test]
fn inbox_pr_ready_of_a_reopened_cancelled_workflow_carries_its_plan_and_session() {
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
                   VALUES ('acme/shop', 7, 'https://github.com/acme/shop/pull/7', 'T', 0, 'open', 'none', 'passed', '{T}', '{T}',
                           'mergeable', 1, 0, 'clean', '[]', 'main', 'main');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                        pr_number, pr_url, pr_opened_at, pull_request)
                   VALUES ('pay', 'api', '/r', 'main', 'cancelled', 'cpd/pay/api', NULL, cpd_txn(), '{T}', NULL,
                           7, 'https://github.com/acme/shop/pull/7', '{T}', (SELECT id FROM pull_requests WHERE number = 7));"
            ))?;
            Ok(())
        })
        .unwrap();

    let found = conditions_with(
        &mut store,
        &home,
        datetime!(2026-01-02 00:00 UTC),
        20,
        1,
        None,
    )
    .unwrap();

    let ready: Vec<_> = found.iter().filter(|c| c.kind == Kind::PrReady).collect();
    assert_eq!(ready.len(), 1);
    let item = ready[0];
    assert_eq!(item.subject, "pr:acme/shop#7");
    assert_eq!(
        (
            item.context.as_ref().map(|c| c.as_str()),
            item.plan.as_ref().map(|p| p.as_str()),
            item.session.as_ref().map(|s| s.as_str()),
        ),
        (Some("ctx"), Some("pay"), Some("l-pay"))
    );
}
