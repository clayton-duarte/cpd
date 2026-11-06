//! Red tests of `require_ready` in a re-planned Plan (pro-d429, pro-d433, A5): a Plan in phase `plan` with `locked_at` set
//! keeps starting the queued Jobs of Workflows that are `dispatch` or `open`; a `waiting` Workflow's Jobs stay refused.
//! `require_ready` is private to the starter, so the tests go through `start_with` and look at the refusal: `job_not_ready`
//! or not. No git, no tmux, no process (the fakes of `crate::ports`).

use super::{StartCtx, start_with};
use crate::attempt::explore_tests::{Rig, catalog, plan};
use crate::error::ErrorCode;
use crate::ids::JobKey;
use crate::plan::{jobs, locked_fingerprint};

/// A Rig whose Plan `pay` is in phase `plan` with `locked_at` set, and Workflow `api` in `status` with its queued Job `work`.
fn replanned(status: &str) -> Rig {
    let mut rig = Rig::new();
    rig.sql(&format!(
        "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at) SELECT 'acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'T', 0, 'open', 'none', 'none', 't', 't' WHERE '{status}' = 'open'; 
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at,
                                pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay', 'api', '/r/api', 'main', '{status}', 'cpd/pay/api', CASE WHEN '{status}' = 'waiting' THEN NULL ELSE '/w/api' END, 'Api', 'config', cpd_txn(), 't',
                   CASE WHEN '{status}' = 'waiting' THEN NULL ELSE 't' END,
                   CASE WHEN '{status}' = 'open' THEN 7 END, CASE WHEN '{status}' = 'open' THEN 'https://github.com/acme/pay/pull/7' END,
                   CASE WHEN '{status}' = 'open' THEN 't' END, (SELECT id FROM pull_requests WHERE number = 7));
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'api', 'work', 'T', 'S2', 'g', 'c', '[\"a.txt\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');"
    ));
    let all = rig
        .env
        .store
        .read(|conn| jobs::load(conn, &plan()))
        .unwrap();
    let fingerprint = locked_fingerprint(all.iter().find(|j| j.key == "work").unwrap());
    rig.sql(&format!(
        "UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE key = 'work';"
    ));
    rig
}

/// The code of the refusal the starter gives Job `work`, `None` when it started.
fn start_code(rig: &mut Rig) -> Option<ErrorCode> {
    rig.fakes
        .spawn
        .set_self_exe(std::env::current_exe().unwrap());
    let catalog = catalog();
    let ctx = StartCtx {
        home: &rig.env.home,
        config: &rig.env.config,
        catalog: &catalog,
        command: "test",
    };
    start_with(
        &rig.fakes.ports,
        &mut rig.env.store,
        &ctx,
        &plan(),
        &JobKey::parse("work").unwrap(),
    )
    .err()
    .map(|e| e.code())
}

#[test]
fn require_ready_accepts_phase_plan_for_a_dispatch_or_open_workflow() {
    for status in ["dispatch", "open"] {
        let mut rig = replanned(status);
        assert_ne!(
            start_code(&mut rig),
            Some(ErrorCode::JobNotReady),
            "a {status} Workflow's queued Job is Ready in a locked phase-plan Plan"
        );
    }
    let mut rig = replanned("waiting");
    assert_eq!(
        start_code(&mut rig),
        Some(ErrorCode::JobNotReady),
        "a waiting Workflow's Job is still refused"
    );
}
