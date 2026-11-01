//! 'Ready to merge' in the cockpit's read model (pro-d118; commands.md §13.2): the Plan view's Workflow box has `Pull.clean` (its
//! yellow `open`) only for a Workflow whose base is the repo's default branch; a NULL base or default branch is not ready. Rows are
//! inserted with the `base_ref` and `default_branch` columns of the `pr-base` migration. [`store_with`] is shared with
//! `widgets::stacked_ready_tests`.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;

use crate::read;

/// A store of Context `ctx` and Plan `pay` whose one Workflow owns the open, authored, non-draft pull request `acme/repo#7`, with
/// `merge_state = 'clean'` and the given `base_ref` and `default_branch` (SQL literals, e.g. `'main'` or `NULL`).
pub(crate) fn store_with(base_ref: &str, default_branch: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let at = "'2026-01-02T00:00:00.000Z'";
    let sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', NULL, 'lead1', '/repo', 'O3', 'idle', {at}, '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', '2026-01-01T00:00:00.000Z', {at}, NULL);
         UPDATE sessions SET plan = 'pay' WHERE id = 'lead1';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, authored,
                                    base_ref, default_branch, created_at, changed_at)
           VALUES ('acme/repo', 7, 'https://github.com/acme/repo/pull/7', 'T', 0, 'open', 'none', 'passed', 'clean', 1,
                   {base_ref}, {default_branch}, {at}, {at});
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at)
           VALUES ('pay', 'api', '/repo', 'main', 'open', 'cpd/pay/api', '/wt', 7, 'https://github.com/acme/repo/pull/7', {at},
                   (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), '2026-01-01T00:00:00.000Z', {at});"
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    (dir, store)
}

/// `Pull.clean` of the Plan view's one Workflow box.
fn box_clean(store: &mut Store) -> bool {
    let boxes = read::plan_boxes(store, &PlanId::parse("pay").unwrap()).unwrap();
    boxes.workflows[0].pull.as_ref().expect("the Workflow has a pull request").clean
}

/// A stacked Workflow (clean against its base, which is not the default branch): its `open` is not yellow.
#[test]
fn stacked_ready_plan_view_open_is_not_yellow_for_a_stacked_pr() {
    let (_dir, mut store) = store_with("'cpd/other'", "'main'");
    assert!(!box_clean(&mut store));
}
