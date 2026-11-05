//! E13's finished check (`sleep::LEAD_FINISHED`): a Lead's Plan's Workflows decide it, never another Plan's.

use super::sleep;
use super::testkit::{Fx, ago};

/// Plan `pay` is `done` long ago with its Workflow cancelled and Lead `lead1` idle since long ago; Plan `other`
/// (same Context) has a `cancelled` Workflow whose pull request is open.
fn rig(other_has_open_pr: bool) -> Fx {
    let mut fx = Fx::new("");
    let long_ago = ago(24 * 60);
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{long_ago}', 't', 'durable');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'lead1', 't', 't', '{long_ago}');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at)
           VALUES ('pay', 'repo', '/r', 'main', 'cancelled', 'cpd/pay', NULL, 'T', 'config', cpd_txn(), 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead2', 'lead', 'ctx', 'other', 'Lead2', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('other', 'ctx', 'Other', 'dispatch', 'lead2', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at)
           VALUES ('other', 'repo', '/o', 'main', 'cancelled', 'cpd/other', NULL, 'T', 'config', cpd_txn(), 't');"
    ));
    if other_has_open_pr {
        fx.fixture(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored)
               VALUES ('acme/other', 9, 'https://github.com/acme/other/pull/9', 'T', 0, 'open', 'none', 'none', 'opened', 'opened', 1);
             UPDATE workflows SET pr_number = 9, pr_url = 'https://github.com/acme/other/pull/9', pr_opened_at = 'opened',
                    pull_request = (SELECT id FROM pull_requests WHERE number = 9)
              WHERE plan = 'other';",
        );
    }
    fx
}

fn asleep(fx: &mut Fx) -> bool {
    fx.query::<i64>("SELECT count(*) FROM sessions WHERE id = 'lead1' AND asleep_at IS NOT NULL")
        == 1
}

#[test]
fn another_plans_cancelled_workflow_with_an_open_pr_does_not_keep_a_finished_lead_awake() {
    let mut fx = rig(true);
    sleep::run(&mut fx.engine);
    assert!(
        asleep(&mut fx),
        "a finished Lead sleeps whatever another Plan's Workflow holds"
    );
}

#[test]
fn a_plans_own_open_pr_workflow_keeps_its_lead_awake() {
    let mut fx = rig(false);
    fx.fixture(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored)
           VALUES ('acme/pay', 8, 'https://github.com/acme/pay/pull/8', 'T', 0, 'open', 'none', 'none', 'opened', 'opened', 1);
         UPDATE workflows SET pr_number = 8, pr_url = 'https://github.com/acme/pay/pull/8', pr_opened_at = 'opened',
                pull_request = (SELECT id FROM pull_requests WHERE number = 8)
          WHERE plan = 'pay';",
    );
    sleep::run(&mut fx.engine);
    assert!(
        !asleep(&mut fx),
        "the Plan's own open pull request keeps its Lead awake"
    );
}
