//! `job dispatch`, the open Job's merge and the Jira ticket of a PR (Plan pr-ticket): when the owner's policy sets `workflow.jira_required`,
//! an `open` Job whose PR has no `jira` and no `head_ref` is refused `jira_required` before any Attempt or driver, and its merge ends
//! the Attempt `failed` / `gate_error`. An adopted PR's open Job, an `update` Job and a policy that leaves the key false need no ticket.

use crate::common;

use super::driver_desc_gate::{CLEAR_JIRA, refused_open_merge};
use super::driver_open::{Live, open_live, open_pr_live, submit_only};
use super::driver::PR_URL;
use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

const NO_JIRA: &str = "UPDATE workflows SET jira = NULL WHERE key = 'repo';";
const WITH_JIRA: &str = "UPDATE workflows SET jira = 'ABC-1', title = 'feat: ABC-1/Charge the card at checkout' WHERE key = 'repo';";
const BAD_TITLE: &str =
    "UPDATE workflows SET title = 'Charge the card at checkout' WHERE key = 'repo'";

/// `open_live` whose owner `acme` requires a ticket (`required`) or says nothing of it.
fn live_requiring(required: bool) -> Live {
    let live = open_live();
    let key = if required {
        "[workflow]\njira_required = true\n"
    } else {
        "[workflow]\njira_required = false\n"
    };
    live.fx.env.owner_policy(
        "acme",
        &format!("[gate]\nmerge = \"local\"\npr = \"local\"\n{key}"),
    );
    live
}

fn attempts(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM attempts")
}

#[test]
fn job_dispatch_refuses_an_open_job_whose_pr_has_no_jira_when_the_policy_requires_it() {
    let live = live_requiring(true);

    let run = live.dispatch("open-repo");

    let error = common::refused(&run, "jira_required", 3);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("cpd workflow edit repo --jira"),
        "{error}"
    );
    assert_eq!(attempts(&live), 0, "no Attempt row");
    assert_eq!(
        live.fx.fakes.spawn.launches().len(),
        0,
        "no driver launched"
    );
    assert_eq!(live.job_status("open-repo"), "queued");
}

#[test]
fn job_dispatch_starts_the_open_job_of_a_pr_with_a_jira_key_when_the_policy_requires_it() {
    let live = live_requiring(true);
    live.fixture(WITH_JIRA);
    let out = live.dispatch("open-repo").ok();
    assert_eq!(out["attempt"], "open-repo-a1", "{out}");
    assert_eq!(attempts(&live), 1);
}

#[test]
fn an_open_jobs_merge_with_no_jira_ticket_fails_gate_error_and_is_not_merged() {
    let live = live_requiring(true);
    live.fixture(WITH_JIRA);
    refused_open_merge(&live, CLEAR_JIRA, "jira_required");
}

#[test]
fn open_merge_refuses_a_title_without_the_jira_key() {
    let live = live_requiring(true);
    live.fixture(WITH_JIRA);
    refused_open_merge(&live, BAD_TITLE, "jira_title_invalid");
}

/// Guard: a policy that leaves the key false never needs a ticket.
#[test]
fn job_dispatch_starts_the_open_job_of_a_pr_with_no_jira_when_the_policy_does_not_require_one() {
    let live = live_requiring(false);
    live.fixture(NO_JIRA);
    let out = live.dispatch("open-repo").ok();
    assert_eq!(out["attempt"], "open-repo-a1", "{out}");
}

/// Guard: an adopted PR (`head_ref` set) needs no ticket; `head_ref` is fixed by a trigger, lifted for the update and put back.
#[test]
fn job_dispatch_starts_the_open_job_of_an_adopted_pr_with_no_jira_when_required() {
    let live = live_requiring(true);
    let trigger: String =
        live.value("SELECT sql FROM sqlite_master WHERE name = 'workflows_head_ref_fixed'");
    live.fixture(&format!("DROP TRIGGER workflows_head_ref_fixed; UPDATE workflows SET head_ref = 'feature' WHERE key = 'repo'; {trigger};"));
    let out = live.dispatch("open-repo").ok();
    assert_eq!(out["attempt"], "open-repo-a1", "{out}");
}

/// Guard: an `update` Job needs no ticket.
#[test]
fn job_dispatch_starts_an_update_job_whose_pr_has_no_jira_when_required() {
    let live = open_pr_live();
    live.fx.env.owner_policy(
        "acme",
        "[gate]\nmerge = \"local\"\npr = \"local\"\n\n[workflow]\njira_required = true\n",
    );
    live.add_cpd_job("up-repo", "update");
    let out = live.dispatch("up-repo").ok();
    assert_eq!(out["attempt"], "up-repo-a1", "{out}");
}

// ---------- the in_review transition at the PR open (pro-d454) ----------

/// The `jira` rows of the outbox as `[target_key, target, status]`, oldest first.
fn jira_rows(live: &Live) -> Value {
    let json: String = live.value(
        "SELECT coalesce(json_group_array(json_array(target_key, target, status)), '[]')
           FROM (SELECT target_key, json_extract(vars_json, '$.target') AS target, status
                   FROM swr_mutations WHERE system = 'jira' ORDER BY id)",
    );
    serde_json::from_str(&json).unwrap()
}

/// The `[target_key, target]` of the rows still `pending`.
fn pending(live: &Live) -> Value {
    Value::Array(
        jira_rows(live)
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r[2] == "pending")
            .map(|r| json!([r[0], r[1]]))
            .collect(),
    )
}

fn merge_the_open_job(live: &Live) {
    let (attempt, _) = live.run_job("open-repo", submit_only);
    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("merged"), &json!("merged")),
        "{attempt}"
    );
}

#[test]
fn an_open_job_merge_queues_the_in_review_transition() {
    let live = open_live();
    live.gh_creates();
    live.fixture("UPDATE workflows SET jira = 'ABC-1' WHERE key = 'repo';");

    merge_the_open_job(&live);

    assert_eq!(live.pr_row().0, "open");
    assert_eq!(
        pending(&live),
        json!([["jira:issue:ABC-1", "in_review"]]),
        "{}",
        jira_rows(&live)
    );
}

#[test]
fn an_open_job_merge_of_a_workflow_with_no_ticket_queues_a_ticketless_in_review_row() {
    let live = open_live();
    live.gh_creates();

    merge_the_open_job(&live);

    assert!(
        pending(&live)
            .as_array()
            .unwrap()
            .contains(&json!([null, "in_review"])),
        "{}",
        jira_rows(&live)
    );
}

#[test]
fn an_open_job_merge_of_an_already_merged_pr_leaves_only_done_pending() {
    let live = open_live();
    live.fixture("UPDATE workflows SET jira = 'ABC-1' WHERE key = 'repo';");
    let merged = common::pay_lookup()
        .replace("\"state\":\"OPEN\"", "\"state\":\"MERGED\"")
        .replace("\"mergedAt\":null", "\"mergedAt\":\"2026-10-20T12:00:00Z\"");
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok(&format!("[{{\"number\":812,\"url\":\"{PR_URL}\",\"state\":\"MERGED\",\"createdAt\":\"2026-10-28T10:00:00Z\"}}]")))
        .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&merged));

    merge_the_open_job(&live);

    assert_eq!(live.pr_row().0, "merged");
    let rows = jira_rows(&live);
    assert_eq!(pending(&live), json!([["jira:issue:ABC-1", "done"]]), "{rows}");
    assert!(
        rows.as_array().unwrap().iter().any(|r| r[1] == "in_review"),
        "the open queued in_review before the merge's done: {rows}"
    );
}
