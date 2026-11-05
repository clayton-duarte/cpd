//! A Job that is not final holds the Plan in `deploy`: the last Workflow ending does not finish the Plan while a Job is
//! `failed`; cancelling that Job, or merging its retry, does. Flow tests of the cli, in-process on `Fx::with_ports()`.

use super::workflow_crud::{Crud, crud};

use serde_json::{Value, json};

/// A Plan `pay-1` in `dispatch`: Workflow `api` merged with a `failed` Job `broken`, Workflow `gateci` in dispatch
/// with only a cancelled Job. Cancelling `gateci` is the last Workflow ending.
fn plan_with_a_failed_job() -> Crud {
    let fx = crud("dispatch");
    let repo = fx.pay.display().to_string();
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 7, 'u', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, shape, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at,
                                pull_request, merged_at, created_txn, created_at)
           VALUES ('pay-1', 'api', '{repo}', 'main', 'merged', 'free', 'cpd/pay-1/api', '/w/api', 't', 7, 'u', 't',
                   (SELECT id FROM pull_requests WHERE number = 7), 't', cpd_txn(), 't');
         INSERT INTO workflows (plan, key, repo, base, status, shape, lead_branch, lead_worktree, started_at, created_txn, created_at)
           VALUES ('pay-1', 'gateci', '{repo}', 'main', 'dispatch', 'free', 'cpd/pay-1/gateci', '/w/gateci', 't', cpd_txn(), 't');"
    ));
    for (key, workflow, status) in [("broken", "api", "failed"), ("stub", "gateci", "cancelled")] {
        let ins = if status == "failed" { "queued" } else { status };
        fx.env.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('pay-1', '{workflow}', '{key}', 'work', 'Job {key}', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', '{ins}', 0,
                     iif('{ins}' = 'cancelled', NULL, printf('%064d', 1)),
                     iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');"
        ));
    }
    fx.env
        .sql("UPDATE jobs SET status = 'running' WHERE key = 'broken'");
    fx.env
        .sql("UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'broken'");
    fx
}

fn shown_phase(fx: &Crud) -> Value {
    fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok()["plan"]["phase"].clone()
}

fn plan_done_events(fx: &Crud) -> usize {
    fx.cpd(&["events", "list", "--kind", "plan-done"]).ok()["events"]
        .as_array()
        .unwrap()
        .len()
}

/// The last Workflow ends with the Job `broken` failed: the Plan is held in `deploy`.
fn end_the_last_workflow(fx: &Crud) {
    assert_eq!(shown_phase(fx), json!("dispatch"));
    fx.pr(&["cancel", "gateci"]).ok();
    assert_eq!(
        shown_phase(fx),
        json!("deploy"),
        "a failed Job holds the Plan in deploy"
    );
    assert_eq!(plan_done_events(fx), 0);
}

#[test]
fn a_failed_job_holds_the_plan_in_deploy_until_it_is_cancelled() {
    let fx = plan_with_a_failed_job();
    end_the_last_workflow(&fx);

    fx.cpd(&[
        "job", "cancel", "broken", "--reason", "given up", "--plan", "pay-1",
    ])
    .ok();
    assert_eq!(shown_phase(&fx), json!("done"));
    assert_eq!(plan_done_events(&fx), 1);
}

#[test]
fn a_failed_job_holds_the_plan_in_deploy_until_its_retry_merges() {
    let fx = plan_with_a_failed_job();
    end_the_last_workflow(&fx);

    // What `job retry` writes (core's `requeue`); the command itself needs pi's model catalog, an external tool.
    fx.env.sql(
        "UPDATE jobs SET status = 'queued', retry_tier = 'S2', failed_cause = NULL WHERE key = 'broken'",
    );
    assert_eq!(
        shown_phase(&fx),
        json!("deploy"),
        "a queued retry still holds the Plan"
    );
    assert_eq!(plan_done_events(&fx), 0);
    fx.cpd(&[
        "job", "cancel", "broken", "--reason", "given up", "--plan", "pay-1",
    ])
    .ok();
    assert_eq!(shown_phase(&fx), json!("done"));
    assert_eq!(plan_done_events(&fx), 1);
}
