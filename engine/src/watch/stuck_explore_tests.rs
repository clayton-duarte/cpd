//! E8 on failed explore Jobs (`scout`/`research`, `jobs.pr` NULL; contract §3.7 "A stuck Job"): E8 does not fail on them,
//! and the stuck explore Jobs of a Plan are one message to the Manager, once, whatever the Plan's Workflow Jobs do.

use serde_json::Value;

use super::inbox_tests::{data, failed_job, fx, manager, messages, open_items, stuck_text, tick};
use super::testkit::{Fx, ago};

/// A `failed` explore Job `key` of Plan `pay` (kind `scout`, no Workflow) whose `job-failed` Event is `minutes` old.
fn failed_explore(fx: &mut Fx, key: &str, minutes: i64) {
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, pr, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, failed_cause, created_txn, created_at)
         VALUES ('pay', NULL, '{key}', 'scout', 'Title', 'H2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'failed', 0,
                 hex(zeroblob(32)), 'timed_out', cpd_txn(), 't');
         INSERT INTO events (txn, at, kind, context, plan, job, text, data)
         VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', '{key}', 'x', '{{}}');",
        ago(minutes)
    ));
}

/// The text of the one message for a Plan's stuck explore Jobs, each `(key, minutes)` with cause `timed_out`.
fn explore_text(jobs: &[(&str, i64)]) -> String {
    let list: Vec<String> = jobs
        .iter()
        .map(|(k, m)| format!("pay/{k} ({m} min ago, timed_out)"))
        .collect();
    format!(
        "Explore Jobs of Plan pay failed and their Lead has not retried or cancelled them: {}. \
         Retry each (cpd job retry <key> --plan pay --tier <T>), nudge the Lead, or ask the User.",
        list.join("; ")
    )
}

fn idle_lead(fx: &mut Fx) {
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
}

#[test]
fn a_failed_explore_job_does_not_fail_e8() {
    let mut fx = fx();
    idle_lead(&mut fx);
    failed_explore(&mut fx, "repro", 2);

    tick(&mut fx);
    assert_eq!(data(&mut fx, "error"), Vec::<Value>::new());
}

#[test]
fn stuck_explore_jobs_of_a_plan_are_one_message_to_the_manager() {
    let mut fx = fx();
    idle_lead(&mut fx);
    manager(&mut fx);
    failed_explore(&mut fx, "a", 3);
    failed_explore(&mut fx, "b", 2);

    for _ in 0..4 {
        tick(&mut fx);
    }
    assert_eq!(
        messages(&mut fx),
        [(
            "mgr1".to_owned(),
            None,
            true,
            explore_text(&[("a", 3), ("b", 2)])
        )]
    );
    assert_eq!(open_items(&mut fx), []);
}

#[test]
fn a_stuck_explore_job_and_a_stuck_pr_job_are_two_messages() {
    let mut fx = fx();
    idle_lead(&mut fx);
    manager(&mut fx);
    failed_explore(&mut fx, "scout1", 5);
    failed_job(&mut fx, "old", 2);

    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(
        messages(&mut fx),
        [
            (
                "mgr1".to_owned(),
                None,
                true,
                explore_text(&[("scout1", 5)])
            ),
            (
                "mgr1".to_owned(),
                None,
                true,
                stuck_text("old", 2, "crashed")
            ),
        ]
    );
}

#[test]
fn only_the_leads_state_says_an_explore_job_is_being_worked_on() {
    let mut fx = fx(); // the Lead is `working`
    manager(&mut fx);
    failed_explore(&mut fx, "a", 2);

    tick(&mut fx);
    assert_eq!(messages(&mut fx), []);

    idle_lead(&mut fx);
    // A queued Workflow Job created after the failure does not hold an explore Job back: it has no Workflow.
    fx.fixture(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at)
         VALUES ('pay', 'repo', 'later', 'Title', 'S2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'queued', 1,
                 hex(zeroblob(32)), cpd_txn(), 't');",
    );
    tick(&mut fx);
    assert_eq!(
        messages(&mut fx),
        [("mgr1".to_owned(), None, true, explore_text(&[("a", 2)]))]
    );
}
