//! `job dispatch`, the open Job's merge and the Jira ticket of a PR (Plan pr-ticket): when the owner's policy sets `pr.jira_required`,
//! an `open` Job whose PR has no `jira` and no `head_ref` is refused `jira_required` before any Attempt or driver, and its merge ends
//! the Attempt `failed` / `gate_error`. An adopted PR's open Job, an `update` Job and a policy that leaves the key false need no ticket.

use crate::common;

use super::driver_desc_gate::{CLEAR_JIRA, refused_open_merge};
use super::driver_open::{Live, open_live, open_pr_live};

const NO_JIRA: &str = "UPDATE workflows SET jira = NULL WHERE key = 'repo';";
const WITH_JIRA: &str = "UPDATE workflows SET jira = 'ABC-1' WHERE key = 'repo';";

/// `open_live` whose owner `acme` requires a ticket (`required`) or says nothing of it.
fn live_requiring(required: bool) -> Live {
    let live = open_live();
    let key = if required {
        "[pr]\njira_required = true\n"
    } else {
        "[pr]\njira_required = false\n"
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
            .contains("cpd pr edit repo --jira"),
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
        "[gate]\nmerge = \"local\"\npr = \"local\"\n[pr]\njira_required = true\n",
    );
    live.add_cpd_job("up-repo", "update");
    let out = live.dispatch("up-repo").ok();
    assert_eq!(out["attempt"], "up-repo-a1", "{out}");
}
