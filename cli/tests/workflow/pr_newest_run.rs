//! The newest run of a check decides whether it failed (contract §7.6 step 3b; decision pro-d230): a superseded run is
//! not a failure. All in-process over a mocked `gh`: no real GitHub.

use serde_json::{Value, json};

use super::pr_checks::{checks_of_row, failing_rollup, feedback_data, one_page, read_says, reads};

/// A GraphQL CheckRun node of `workflow` (none: a third-party app).
fn node_run(
    workflow: Option<&str>,
    name: &str,
    conclusion: Option<&str>,
    started: Option<&str>,
) -> Value {
    let run = workflow.map(|w| json!({"workflow": {"name": w}}));
    let status = if conclusion.is_some() {
        "COMPLETED"
    } else {
        "IN_PROGRESS"
    };
    json!({"__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion, "startedAt": started,
           "checkSuite": {"workflowRun": run}})
}

fn node_ctx(context: &str, state: &str, created: &str) -> Value {
    json!({"__typename": "StatusContext", "context": context, "state": state, "createdAt": created})
}

const W: Option<&str> = Some("Run Danger on PR");

/// Poll over a failing rollup whose checks read answers `nodes`: (stored checks, feedback Events).
fn poll_with(nodes: Vec<Value>) -> (String, Vec<Value>) {
    let fx = failing_rollup();
    read_says(&fx, one_page(nodes));
    fx.poll();
    (checks_of_row(&fx), feedback_data(&fx))
}

#[test]
fn a_superseded_failure_beside_a_newer_success_is_passed_and_wakes_nobody() {
    let (checks, events) = poll_with(vec![
        node_run(W, "danger", Some("FAILURE"), Some("2026-10-30T04:19:08Z")),
        node_run(W, "danger", Some("FAILURE"), Some("2026-10-30T04:22:08Z")),
        node_run(W, "danger", Some("SUCCESS"), Some("2026-10-30T14:27:08Z")),
    ]);
    assert_eq!(events, Vec::<Value>::new());
    assert_eq!(checks, "passed");
}

#[test]
fn a_pending_rerun_supersedes_an_older_failure() {
    let (checks, events) = poll_with(vec![
        node_run(W, "ci", Some("FAILURE"), Some("2026-10-30T04:19:08Z")),
        node_run(W, "ci", None, Some("2026-10-30T05:00:00Z")),
    ]);
    assert_eq!(events, Vec::<Value>::new());
    assert_eq!(checks, "pending");
}

#[test]
fn a_status_context_is_decided_by_its_newest_state() {
    let (checks, _) = poll_with(vec![
        node_ctx("Danger", "ERROR", "2026-10-30T10:00:00Z"),
        node_ctx("Danger", "SUCCESS", "2026-10-30T11:00:00Z"),
    ]);
    assert_eq!(checks, "passed");
}

#[test]
fn the_checks_read_asks_for_the_workflow_and_the_start_time() {
    let fx = failing_rollup();
    read_says(
        &fx,
        one_page(vec![node_run(
            W,
            "ci",
            Some("SUCCESS"),
            Some("2026-10-30T04:19:08Z"),
        )]),
    );
    fx.poll();
    let query = &reads(&fx)[0][3];
    for word in ["startedAt", "createdAt", "workflowRun"] {
        assert!(query.contains(word), "the query lacks {word}: {query}");
    }
}

// Controls: they pass today and must keep passing.

#[test]
fn a_failure_newer_than_a_success_still_fails_and_wakes() {
    let (checks, events) = poll_with(vec![
        node_run(W, "ci", Some("SUCCESS"), Some("2026-10-30T04:19:08Z")),
        node_run(W, "ci", Some("FAILURE"), Some("2026-10-30T05:00:00Z")),
    ]);
    assert_eq!(checks, "failed");
    assert_eq!(events.len(), 1);
}

#[test]
fn runs_are_told_apart_by_workflow_and_name() {
    let (checks, events) = poll_with(vec![
        node_run(
            Some("A"),
            "build",
            Some("FAILURE"),
            Some("2026-10-30T04:19:08Z"),
        ),
        node_run(
            Some("B"),
            "build",
            Some("SUCCESS"),
            Some("2026-10-30T05:00:00Z"),
        ),
    ]);
    assert_eq!(checks, "failed");
    assert_eq!(events.len(), 1);
}

#[test]
fn a_cancelled_run_supersedes_nothing() {
    let (checks, events) = poll_with(vec![
        node_run(W, "ci", Some("FAILURE"), Some("2026-10-30T04:19:08Z")),
        node_run(W, "ci", Some("CANCELLED"), Some("2026-10-30T05:00:00Z")),
    ]);
    assert_eq!(checks, "failed");
    assert_eq!(events.len(), 1);
}

#[test]
fn equal_times_let_the_failing_run_win() {
    let (checks, events) = poll_with(vec![
        node_run(W, "ci", Some("SUCCESS"), Some("2026-10-30T04:19:08Z")),
        node_run(W, "ci", Some("FAILURE"), Some("2026-10-30T04:19:08Z")),
    ]);
    assert_eq!(checks, "failed");
    assert_eq!(events.len(), 1);
}

#[test]
fn a_run_without_a_time_is_older_than_a_run_with_one() {
    // A timeless success is older than a timed failure, so the failure stands; a timeless failure alone is counted.
    let (checks, _) = poll_with(vec![
        node_run(W, "ci", Some("SUCCESS"), None),
        node_run(W, "ci", Some("FAILURE"), Some("2026-10-30T04:19:08Z")),
    ]);
    assert_eq!(checks, "failed");
    let (checks, _) = poll_with(vec![node_run(W, "ci", Some("FAILURE"), None)]);
    assert_eq!(checks, "failed");
}
