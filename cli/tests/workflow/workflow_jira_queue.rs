//! The Jira transition queue's appends at the PR side (Plan jira-sync, J4b and J4c): the transaction that records a merge
//! queues `done`, the one that records a post queues `in_review`. The queue is read with SQL, over fake `gh` and `node`.

use crate::common::{self, TestEnv};

use serde_json::{Value, json};

use super::pr_post::{Setup, post};
use super::pr_reaction_queue::{REF, fixture, merge};

/// The `jira` transitions of the outbox as `[plan, workflow, target, pending, unparked]` rows, oldest first; a superseded row
/// (the Forward rule collapsed it into a pending one) is not shown.
fn queue(env: &TestEnv) -> Value {
    let json: String = env.query(
        "SELECT coalesce(json_group_array(json_array(plan, workflow, target, pending, unparked)), '[]')
           FROM (SELECT json_extract(context_json, '$.plan') AS plan, json_extract(context_json, '$.workflow') AS workflow,
                        json_extract(vars_json, '$.target') AS target, status = 'pending' AS pending, status <> 'failed' AS unparked
                   FROM swr_mutations WHERE system = 'jira' AND status <> 'superseded' ORDER BY id)",
    );
    serde_json::from_str(&json).unwrap()
}

fn row(plan: &str, pr: &str, target: &str) -> Value {
    json!([plan, pr, target, 1, 1])
}

#[test]
fn a_merge_queues_the_done_transition_of_the_pr() {
    let fx = fixture(None);
    fx.env
        .sql("UPDATE workflows SET jira = 'ABC-123' WHERE key = 'api';");
    merge(&fx);
    assert_eq!(queue(&fx.env), json!([row("pay-1", "api", "done")]));
}

#[test]
fn a_merge_of_a_pr_with_no_ticket_queues_the_same_row() {
    let fx = fixture(None);
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT jira FROM workflows WHERE key = 'api'"),
        None
    );
    merge(&fx);
    assert_eq!(
        queue(&fx.env),
        json!([row("pay-1", "api", "done")]),
        "rows wait for a ticket"
    );
}

#[test]
fn merging_twice_queues_one_done_row() {
    let fx = fixture(None);
    merge(&fx);
    // Merged on GitHub now: a second `pr merge` records it again (a no-op) and refuses before the merge-method read.
    let _ = fx.cpd(&["pr", "merge", REF]);
    assert_eq!(queue(&fx.env), json!([row("pay-1", "api", "done")]));
}

#[test]
fn a_post_queues_the_in_review_transition_and_a_refused_post_queues_nothing() {
    let pay = post(Setup::baseline());
    pay.post_cmd().ok();
    let env = &pay.fx.env;
    assert_eq!(queue(env), json!([row("pay", "repo", "in_review")]));

    common::refused(&pay.post_cmd(), "already_posted", 3);
    assert_eq!(
        queue(env),
        json!([row("pay", "repo", "in_review")]),
        "a repeated post queues nothing"
    );

    drop(pay); // the harness holds a process-wide lock for a fixture's life
    let unready = post(Setup::baseline());
    unready.set("draft = 1");
    common::refused(&unready.post_cmd(), "pr_not_ready", 3);
    assert_eq!(
        queue(&unready.fx.env),
        json!([]),
        "a refused post queues nothing"
    );
}

#[test]
fn a_post_succeeds_without_the_atlassian_server_and_never_calls_the_jira_helper() {
    let pay = post(Setup::baseline());
    let machine =
        std::fs::read_to_string(pay.fx.env.cpd_home().join("policy/host.toml")).unwrap_or_default();
    assert!(
        !machine.contains("mcp.servers.atlassian"),
        "the machine policy names no Atlassian server"
    );

    pay.post_cmd().ok();

    let scripts: Vec<String> = pay
        .fx
        .calls("node")
        .iter()
        .map(|c| c.args[0].rsplit('/').next().unwrap().to_owned())
        .collect();
    assert_eq!(
        scripts,
        ["slack-post.ts"],
        "node sees only the Slack helper"
    );
    assert_eq!(
        queue(&pay.fx.env),
        json!([row("pay", "repo", "in_review")]),
        "the row is queued all the same"
    );
}
