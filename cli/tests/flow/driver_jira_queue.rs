//! The Jira transition queue's appends at the driver side (Plan jira-sync, J4a and J4c): the transaction that starts the first
//! Attempt of a PR queues `in_progress`; the open Job that finds the pull request already merged queues `done`.
//! Read with SQL, on the `live` harness of `driver_open`.

use crate::common;

use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

use super::driver::PR_URL;
use super::driver::add_job_in;
use super::driver_open::{Live, live, open_live, submit};

/// The queue as `[plan, pr, target, pending, unparked]` rows, oldest first; it fails at run time while the table is missing.
fn queue(live: &Live) -> Value {
    let json: String = live.value(
        "SELECT coalesce(json_group_array(json_array(plan, pr, target, settled_at IS NULL, refused_at IS NULL)), '[]')
           FROM (SELECT plan, pr, target, settled_at, refused_at FROM jira_transitions ORDER BY id)",
    );
    serde_json::from_str(&json).unwrap()
}

fn row(target: &str) -> Value {
    json!(["pay", "repo", target, 1, 1])
}

#[test]
fn the_first_attempt_of_a_pr_queues_in_progress_and_a_later_one_queues_nothing() {
    let live = live("");
    add_job_in(&live.fx.env, "repo", "web", &[]);

    live.dispatch("api").ok();
    assert_eq!(
        queue(&live),
        json!([row("in_progress")]),
        "the first Attempt of the PR"
    );

    // A PR runs one linear Attempt at a time: the first one ends before a second Job starts.
    live.fixture("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE status IN ('starting', 'running');
                  UPDATE jobs SET status = 'queued' WHERE plan = 'pay' AND key = 'api';");
    live.dispatch("web").ok();
    assert_eq!(
        queue(&live),
        json!([row("in_progress")]),
        "an Attempt of a second Job"
    );

    // A retry Attempt of the first Job: its first Attempt ended failed and the Job is queued again.
    live.fixture("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE status IN ('starting', 'running');
                  UPDATE jobs SET status = 'queued' WHERE plan = 'pay' AND key = 'api';");
    live.dispatch("api").ok();
    assert_eq!(
        live.value::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"),
        2,
        "a retry Attempt started"
    );
    assert_eq!(queue(&live), json!([row("in_progress")]), "a retry Attempt");

    // The PR's open Job starts.
    live.fixture("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE status IN ('starting', 'running');
                  UPDATE jobs SET status = 'merged', merged_sha = 's' WHERE key IN ('api', 'web');");
    live.add_cpd_job("open-repo", "open");
    live.dispatch("open-repo").ok();
    assert_eq!(
        queue(&live),
        json!([row("in_progress")]),
        "the open Job starting"
    );
}

#[test]
fn an_open_job_that_finds_the_pull_request_already_merged_queues_done() {
    let live = open_live();
    let merged = common::pay_lookup()
        .replace("\"state\":\"OPEN\"", "\"state\":\"MERGED\"")
        .replace("\"mergedAt\":null", "\"mergedAt\":\"2026-10-20T12:00:00Z\"");
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok(&format!("[{{\"number\":812,\"url\":\"{PR_URL}\",\"state\":\"MERGED\",\"createdAt\":\"2026-10-28T10:00:00Z\"}}]")))
        .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&merged));

    live.run_job("open-repo", submit);

    assert_eq!(
        live.value::<String>("SELECT status FROM prs WHERE key = 'repo'"),
        "merged"
    );
    let done: Value = serde_json::from_str(&live.value::<String>("SELECT coalesce(json_group_array(json_array(plan, pr, target, settled_at IS NULL)), '[]') FROM jira_transitions WHERE target = 'done'")).unwrap();
    assert_eq!(done, json!([["pay", "repo", "done", 1]]));
}
