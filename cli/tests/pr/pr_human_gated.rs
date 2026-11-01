//! Its `gh` answers the three merge-method flags through the shared `Gh` fake in pr_view.rs (default: squash only).
//! The human-gated checks rule (pro-d151, pro-d153): a PENDING StatusContext named in `pr.human_gated_contexts` counts as
//! passed when the PR has a CheckRun, none pending and none failing. The poll's checks read of a PENDING rollup, `pr post`
//! after it and `pr view`'s summary, all in-process over a mocked `gh` and a mocked Slack helper: no real GitHub.

use std::fs;

use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

use super::pr::{Poll, Rows, node, pay, poll_fx};
use super::pr_checks::{
    add_second_owned_pr, answer, checks_of_row, contexts, feedback_data, green, one_page, policy,
    read_says, reads, run, status_context,
};
use super::pr_view;

const OK: &str = r#"{"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","link":"https://acme.slack.com/archives/C0POLICY1/p1793040197490209"}"#;

/// Three green CheckRuns, one of them Chromatic's, and its two StatusContexts PENDING.
fn chromatic() -> Vec<Value> {
    let mut nodes = green(2);
    nodes.push(run(
        "e2e_tests / chromatic-ui-tests",
        "COMPLETED",
        Some("SUCCESS"),
    ));
    nodes.push(status_context("UI Review", "PENDING"));
    nodes.push(status_context("UI Tests", "PENDING"));
    nodes
}

/// An owned PR (812) with owner policy `body` whose search rollup is `rollup`, and whose checks read answers `nodes`.
fn pending_fx(body: &str, rollup: &str, nodes: Vec<Value>) -> Poll {
    let fx = poll_fx("deploy");
    fx.env.owner_policy("acme", body);
    fx.search(&[pay("OPEN", None, Some(rollup))]);
    read_says(&fx, one_page(nodes));
    fx
}

fn any() -> &'static str {
    "[pr]\nfeedback.checks = \"any\"\n"
}

#[test]
fn human_gated_chromatic_pending_after_green_check_runs_is_passed() {
    let fx = pending_fx(any(), "PENDING", chromatic());

    fx.poll();

    assert_eq!(reads(&fx).len(), 1, "{:?}", fx.gh_calls());
    assert_eq!(checks_of_row(&fx), "passed");
    assert!(fx.events("pr-feedback").is_empty());

    let txns = fx.txns();
    read_says(&fx, one_page(chromatic()));
    fx.poll();
    assert_eq!(fx.txns(), txns, "an identical poll writes nothing");
    assert_eq!(checks_of_row(&fx), "passed");
}

#[test]
fn human_gated_pr_that_passed_this_way_is_postable() {
    let fx = pending_fx(
        "[pr]\nfeedback.checks = \"any\"\n[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n",
        "PENDING",
        chromatic(),
    );
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        fx.env.tmux_socket()
    ));
    fx.install_pi();
    fx.env.repo_policy("acme", "pay", "");
    fx.fake_node(Reply::ok(&format!("{OK}\n")));
    fx.poll();
    assert_eq!(fx.pr_data().2, "passed");
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch("UPDATE pull_requests SET mergeable = 'mergeable' WHERE number = 812;")
                .map_err(Into::into)
        })
        .unwrap();

    let run = fx.cpd(&["pr", "post", "--pr", "repo", "--plan", "pay"]);

    let text = format!("{} {}", run.stdout, run.stderr);
    assert!(
        !text.contains("pr_not_ready") && !text.contains("checks_pending"),
        "{text}"
    );
}

#[test]
fn human_gated_pending_with_a_check_run_still_running_stays_pending() {
    let fx = pending_fx(
        any(),
        "PENDING",
        vec![
            run("build", "IN_PROGRESS", None),
            status_context("UI Review", "PENDING"),
        ],
    );

    fx.poll();

    assert_eq!(reads(&fx).len(), 1);
    assert_eq!(checks_of_row(&fx), "pending");
}

#[test]
fn human_gated_context_alone_without_a_check_run_stays_pending() {
    let fx = pending_fx(
        any(),
        "PENDING",
        vec![
            status_context("UI Review", "PENDING"),
            status_context("UI Tests", "PENDING"),
        ],
    );

    fx.poll();

    assert_eq!(reads(&fx).len(), 1);
    assert_eq!(checks_of_row(&fx), "pending");
}

#[test]
fn human_gated_another_pending_status_context_keeps_pending() {
    let mut nodes = green(3);
    nodes.push(status_context("UI Review", "PENDING"));
    nodes.push(status_context("ci/legacy", "PENDING"));
    let fx = pending_fx(any(), "PENDING", nodes);

    fx.poll();

    assert_eq!(reads(&fx).len(), 1);
    assert_eq!(checks_of_row(&fx), "pending");
}

#[test]
fn human_gated_a_failed_check_run_beside_pending_chromatic_is_failed() {
    let fx = pending_fx(
        any(),
        "PENDING",
        vec![
            run("ci/test", "COMPLETED", Some("FAILURE")),
            status_context("UI Review", "PENDING"),
        ],
    );

    fx.poll();

    assert_eq!(reads(&fx).len(), 1);
    assert_eq!(checks_of_row(&fx), "failed");
    let events = feedback_data(&fx);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["reasons"], json!(["checks_failed"]));
}

#[test]
fn human_gated_custom_list_replaces_the_default() {
    let list = "[pr]\nhuman_gated_contexts = [\"ci/review\"]\n";
    let with = |context: &str| {
        let mut nodes = green(3);
        nodes.push(status_context(context, "PENDING"));
        nodes
    };

    // One fixture at a time: each is dropped before the next.
    let fx = pending_fx(list, "PENDING", with("UI Review"));
    fx.poll();
    assert_eq!(
        (reads(&fx).len(), checks_of_row(&fx).as_str()),
        (1, "pending"),
        "the default list is replaced"
    );

    drop(fx);
    let fx = pending_fx(list, "PENDING", with("ci/review"));
    fx.poll();
    assert_eq!(
        (reads(&fx).len(), checks_of_row(&fx).as_str()),
        (1, "passed"),
        "the listed context"
    );
}

#[test]
fn human_gated_read_happens_with_feedback_checks_none() {
    let fx = pending_fx("[pr]\nfeedback.checks = \"none\"\n", "PENDING", chromatic());

    fx.poll();

    assert_eq!(reads(&fx).len(), 1);
    assert_eq!(checks_of_row(&fx), "passed");
}

#[test]
fn human_gated_the_failed_rollup_confirm_uses_the_same_rule() {
    let mut nodes = green(5);
    nodes.push(run("bot", "COMPLETED", Some("CANCELLED")));
    nodes.push(status_context("UI Tests", "PENDING"));
    let fx = pending_fx(any(), "FAILURE", nodes);

    fx.poll();

    assert_eq!(checks_of_row(&fx), "passed");
    assert!(fx.events("pr-feedback").is_empty());
}

#[test]
fn human_gated_one_read_serves_a_failed_and_a_pending_pull_request() {
    let fx = poll_fx("deploy");
    policy(&fx, "any");
    add_second_owned_pr(&fx);
    fx.search(&[
        pay("OPEN", None, Some("FAILURE")),
        node("acme/pay", 813, "OPEN", None, Some("PENDING")),
    ]);
    read_says(
        &fx,
        answer(vec![contexts(green(2), None), contexts(green(2), None)]),
    );

    fx.poll();

    let calls = reads(&fx);
    assert_eq!(calls.len(), 1, "one call for both: {calls:?}");
    assert!(
        calls[0][3].contains("p0:") && calls[0][3].contains("p1:"),
        "{}",
        calls[0][3]
    );
}

/// `pr view` of an open PR whose `gh pr view` has green CheckRuns and `UI Review` PENDING, under owner policy `body`.
fn view(body: &str) -> Value {
    let fx = pr_view::fx("open");
    fx.fx.env.owner_policy("acme", body);
    let mut checks = green(2);
    checks.push(status_context("UI Review", "PENDING"));
    let _ = (*fx.fx.fakes.runner).clone().on(
        "gh",
        |c| c.args.starts_with(&["pr".into(), "view".into()]),
        Reply::ok(&super::pr_checks::view_with(checks)),
    );
    fx.fx
        .cpd(&["pr", "view", "--pr", "repo", "--plan", "pay"])
        .ok()
}

#[test]
fn human_gated_pr_view_summary_follows_the_same_rule() {
    let out = view("[pr]\ngate = \"local\"\n");
    assert_eq!(out["pr"]["checks"], "passed");
    let listed = out["checks"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .any(|c| c["name"] == "UI Review" && c["conclusion"] == "PENDING"),
        "{listed:?}"
    );

    let out = view("[pr]\nhuman_gated_contexts = []\n");
    assert_eq!(out["pr"]["checks"], "pending");
}

#[test]
fn human_gated_empty_list_makes_no_read_and_keeps_pending() {
    let fx = pending_fx(
        "[pr]\nfeedback.checks = \"any\"\nhuman_gated_contexts = []\n",
        "PENDING",
        chromatic(),
    );

    fx.poll();

    assert_eq!(reads(&fx).len(), 0);
    assert_eq!(checks_of_row(&fx), "pending");
}

#[test]
fn human_gated_an_unowned_pr_makes_no_read() {
    let fx = poll_fx("deploy");
    policy(&fx, "any");
    fx.search(&[
        pay("OPEN", None, None),
        node("acme/web", 902, "OPEN", None, Some("PENDING")),
    ]);

    fx.poll();

    assert_eq!(reads(&fx).len(), 0);
    let row = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 902",
                [],
                |r| r.get::<_, String>(0),
            )?)
        })
        .unwrap();
    assert_eq!(row, "pending");
}
