//! The merge side of Plan desc-gate: a refused open Job's merge ends the Attempt `failed` / `gate_error` (never `merged`), the retry is a
//! new Attempt of the same Job, and an update Job's merge needs no stored description.

use serde_json::json;

use super::driver_open::{Live, open_live, open_pr_live, submit_only};

pub(super) const CLEAR_DESCRIPTION: &str = "UPDATE prs SET description = NULL WHERE plan = 'pay' AND key = 'repo'";
pub(super) const CLEAR_TITLE: &str = "UPDATE prs SET title = NULL WHERE plan = 'pay' AND key = 'repo'";
pub(super) const CLEAR_JIRA: &str = "UPDATE prs SET jira = NULL WHERE plan = 'pay' AND key = 'repo'";

fn open_jobs(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM jobs WHERE plan = 'pay' AND kind = 'open'")
}

/// Runs `open-repo` with a Worker that clears the PR's field (`clear`) and submits; asserts the refusal's outcome. Returns the remote Lead Branch before.
pub(super) fn refused_open_merge(live: &Live, clear: &str, code: &str) -> String {
    live.gh_creates();
    let before = live.remote("cpd/pay/repo");

    let (attempt, submitted) = live.run_job("open-repo", |live, id| {
        live.fixture(clear);
        submit_only(live, id)
    });

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("gate_error")), "{attempt}");
    assert_eq!(attempt["status"], "ended", "{attempt}");
    assert!(attempt["merged_sha"].is_null(), "{attempt}");
    let failed = live.plan_events("job-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let detail = failed[0].1["detail"].as_str().unwrap();
    assert!(detail.starts_with(code), "{detail}");
    assert_eq!(live.job_status("open-repo"), "failed", "gate_error is no ladder cause");
    assert_eq!(open_jobs(live), 1, "no new open Job");
    assert!(live.gh_pr_calls().iter().all(|c| c[1] != "create"), "{:?}", live.gh_pr_calls());
    assert_eq!(live.remote("cpd/pay/repo"), before, "nothing was pushed");
    assert_eq!((live.pr_row().0.as_str(), live.phase().as_str()), ("dispatch", "dispatch"));
    assert!(live.plan_events("pr-opened").is_empty());
    assert!(live.plan_events("job-merged").iter().all(|e| e.1["kind"] != "open"), "no open Job merged");
    let merging: i64 = live.value("SELECT count(*) FROM attempts WHERE plan = 'pay' AND status = 'merging'");
    assert_eq!(merging, 0, "no Attempt is left merging");
    before
}

#[test]
fn an_open_jobs_merge_with_no_stored_description_fails_gate_error_and_is_not_merged() {
    let live = open_live();
    refused_open_merge(&live, CLEAR_DESCRIPTION, "description_required");
}

#[test]
fn an_open_jobs_merge_with_no_stored_title_fails_gate_error_and_is_not_merged() {
    let live = open_live();
    refused_open_merge(&live, CLEAR_TITLE, "pr_title_required");
}

#[test]
fn a_retry_after_a_refused_open_merge_is_a_new_attempt_of_the_same_job() {
    let live = open_live();
    refused_open_merge(&live, CLEAR_DESCRIPTION, "description_required");
    live.fixture("UPDATE prs SET description = 'A description' WHERE plan = 'pay' AND key = 'repo'");
    live.fx.cpd(&["job", "retry", "open-repo", "--tier", "S2", "--plan", "pay"]).ok();
    live.gh_creates();

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(attempt["id"], "open-repo-a2", "{attempt}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    let creates = live.gh_pr_calls().iter().filter(|c| c[1] == "create").count();
    assert_eq!(creates, 1, "{:?}", live.gh_pr_calls());
    assert_eq!(live.pr_row().0, "open");
    assert_eq!(open_jobs(&live), 1, "the retry is the same Job");
}

#[test]
fn an_update_jobs_merge_needs_no_stored_description() {
    let live = open_pr_live();
    live.fixture("UPDATE prs SET description = NULL WHERE plan = 'pay' AND key = 'repo'");
    live.add_cpd_job("up-repo", "update");
    live.commit(&live.lead, "README.md", "mine\n", "mine");
    let row = live.pr_row();
    let remote = live.remote("cpd/pay/repo");

    let (attempt, submitted) = live.run_job("up-repo", submit_only);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_ne!(live.remote("cpd/pay/repo"), remote, "its merge pushes");
    assert_eq!(live.remote("cpd/pay/repo"), live.lead_sha());
    assert_eq!(live.pr_row(), row, "an update Job writes no PR row");
}
