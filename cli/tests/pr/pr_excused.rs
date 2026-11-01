//! `pr.excused_labels` (contract §7.6 step 3b, §8.3): while the pull request carries a listed label (ASCII
//! case-insensitive), a transition to `failed` raises no `checks_failed`; the stored `checks` stays `failed`. In-process
//! over a mocked `gh`: no real GitHub.

use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

use super::pr::{Poll, Rows, pay, poll_fx};
use super::pr_checks::{
    answer, checks_of_row, contexts, feedback_data, green, is_checks_read, read_says, reads, run,
    with_labels,
};

const POLICY: &str =
    "[pr]\nfeedback.checks = \"any\"\nexcused_labels = [\"No Merges on Friday\", \"Dont Merge\"]\n";

/// Today the loader drops a `[pr]` table with an unknown key, so the controls (which must pass before the feature) cannot
/// carry `excused_labels`; they use this table, with no list.
const PLAIN: &str = "[pr]\nfeedback.checks = \"any\"\n";

fn failing() -> Vec<Value> {
    let mut nodes = green(2);
    nodes.push(run("danger", "COMPLETED", Some("FAILURE")));
    nodes
}

/// The checks read's answer for one pull request carrying `labels` and one failing check.
fn labelled(labels: &[&str]) -> String {
    answer(vec![with_labels(contexts(failing(), None), labels)])
}

/// An owned PR (812) with `POLICY`, search `search`, whose checks read answers `labelled(labels)`.
fn fx_with(search: String, labels: &[&str]) -> Poll {
    fx_under(POLICY, search, labels)
}

fn fx_under(policy: &str, search: String, labels: &[&str]) -> Poll {
    let fx = poll_fx("deploy");
    fx.env.owner_policy("acme", policy);
    fx.search(&[search]);
    read_says(&fx, labelled(labels));
    fx
}

#[test]
fn an_excused_label_stops_the_checks_failed_wake_and_the_checks_stay_failed() {
    let fx = fx_with(pay("OPEN", None, Some("FAILURE")), &["No Merges on Friday"]);

    fx.poll();

    assert_eq!(
        reads(&fx).len(),
        1,
        "the policy is valid and the failed rollup is confirmed by one checks read"
    );
    assert_eq!(feedback_data(&fx), Vec::<Value>::new());
    assert_eq!(checks_of_row(&fx), "failed");
}

#[test]
fn an_excused_label_matches_without_regard_to_case() {
    let fx = fx_with(pay("OPEN", None, Some("FAILURE")), &["dont merge"]);

    fx.poll();

    assert_eq!(
        reads(&fx).len(),
        1,
        "the policy is valid and the failed rollup is confirmed by one checks read"
    );
    assert_eq!(feedback_data(&fx), Vec::<Value>::new());
    assert_eq!(checks_of_row(&fx), "failed");
}

#[test]
fn removing_the_excused_label_while_checks_stay_failed_wakes_nobody() {
    let fx = fx_with(pay("OPEN", None, Some("FAILURE")), &["No Merges on Friday"]);
    fx.poll();
    assert_eq!(
        reads(&fx).len(),
        1,
        "the policy is valid and the failed rollup is confirmed by one checks read"
    );
    assert_eq!(feedback_data(&fx), Vec::<Value>::new());

    read_says(&fx, labelled(&[]));
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    fx.poll();

    assert_eq!(feedback_data(&fx), Vec::<Value>::new());
    assert_eq!(checks_of_row(&fx), "failed");
}

#[test]
fn an_excused_label_still_lets_changes_requested_wake_the_lead() {
    let fx = fx_with(
        pay("OPEN", Some("CHANGES_REQUESTED"), Some("FAILURE")),
        &["No Merges on Friday"],
    );

    fx.poll();

    let events = feedback_data(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["reasons"], json!(["changes_requested"]));
}

#[test]
fn the_checks_read_asks_for_the_labels() {
    let fx = fx_with(pay("OPEN", None, Some("FAILURE")), &[]);

    fx.poll();

    assert!(
        reads(&fx)[0][3].contains("labels(first: 100)"),
        "{}",
        reads(&fx)[0][3]
    );
}

#[test]
fn a_checks_read_without_labels_fails_the_poll_and_writes_nothing() {
    let fx = poll_fx("deploy");
    fx.env.owner_policy("acme", POLICY);
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    let mut pull = contexts(failing(), None);
    pull.as_object_mut().unwrap().remove("labels");
    let _ = (*fx.fakes.runner)
        .clone()
        .once("gh", is_checks_read, Reply::ok(&answer(vec![pull])));
    let (txns, events, row) = (fx.txns(), fx.event_count(), fx.pr_data());

    let run = fx.cpd(&["pr", "poll", "--plan", "pay"]);

    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.error()["code"], "gh_failed");
    assert_eq!(
        (fx.txns(), fx.event_count(), fx.pr_data()),
        (txns, events, row)
    );
}

#[test]
fn a_label_outside_the_list_still_wakes_the_lead() {
    let fx = fx_under(PLAIN, pay("OPEN", None, Some("FAILURE")), &["bug"]);

    fx.poll();

    let events = feedback_data(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["reasons"], json!(["checks_failed"]));
    assert_eq!(checks_of_row(&fx), "failed");
}

#[test]
fn no_label_still_wakes_the_lead() {
    let fx = fx_under(PLAIN, pay("OPEN", None, Some("FAILURE")), &[]);

    fx.poll();

    let events = feedback_data(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["reasons"], json!(["checks_failed"]));
}
