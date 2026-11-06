//! `pr poll`'s checks read and the per-check mapping (contract §7.6 steps 2 and 3b, pro-d75; commands.md §9.2, §9.8): a
//! cancelled or pending check is not a failed check. All in-process over a mocked `gh`: no real GitHub.

use cpd_core::host::runner::{Call, Reply};
use serde_json::{Value, json};

use super::pr_view;
use super::workflow::{Poll, Rows, node, pay, poll_fx};

/// What the policy says of `feedback.checks`.
pub(super) fn policy(fx: &Poll, checks: &str) {
    fx.env
        .owner_policy("acme", &format!("[pr]\nfeedback.checks = \"{checks}\"\n"));
}

/// An owned PR (812) with policy `any` whose search rollup is `FAILURE`.
pub(super) fn failing_rollup() -> Poll {
    let fx = poll_fx("deploy");
    policy(&fx, "any");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    fx
}

pub(super) fn run(name: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({"__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion})
}

/// `n` successful check runs.
pub(super) fn green(n: usize) -> Vec<Value> {
    (0..n)
        .map(|i| run(&format!("ci/{i}"), "COMPLETED", Some("SUCCESS")))
        .collect()
}

pub(super) fn status_context(context: &str, state: &str) -> Value {
    json!({"__typename": "StatusContext", "context": context, "state": state})
}

/// One pull request's contexts of the checks read: its `nodes` and the page's `hasNextPage` and `endCursor`.
pub(super) fn contexts(nodes: Vec<Value>, next: Option<&str>) -> Value {
    json!({"labels": {"nodes": []}, "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {
        "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next}, "nodes": nodes}}}}]}})
}

/// `pull` (from [`contexts`]) carrying the pull request's `labels` (the policy hold of pro-d447 reads them from the checks read).
pub(super) fn with_labels(mut pull: Value, labels: &[&str]) -> Value {
    let nodes: Vec<Value> = labels.iter().map(|name| json!({"name": name})).collect();
    pull["labels"] = json!({"nodes": nodes});
    pull
}

/// The answer of the checks read: one alias `p<i>` per entry.
pub(super) fn answer(pulls: Vec<Value>) -> String {
    let data: serde_json::Map<String, Value> = pulls
        .into_iter()
        .enumerate()
        .map(|(i, p)| (format!("p{i}"), json!({"pullRequest": p})))
        .collect();
    json!({"data": data}).to_string() + "\n"
}

/// The answer for one pull request holding `nodes`, in one page.
pub(super) fn one_page(nodes: Vec<Value>) -> String {
    answer(vec![contexts(nodes, None)])
}

pub(super) fn is_checks_read(call: &Call) -> bool {
    call.args
        .get(3)
        .is_some_and(|q| q.contains("contexts(first: 100"))
}

/// The next checks read (the first page) is answered `text`.
pub(super) fn read_says(fx: &Poll, text: String) {
    let _ = (*fx.fakes.runner).clone().once(
        "gh",
        |c| is_checks_read(c) && !c.args[3].contains("after:"),
        Reply::ok(&text),
    );
}

/// The argv of every checks read so far.
pub(super) fn reads(fx: &Poll) -> Vec<Vec<String>> {
    fx.calls("gh")
        .into_iter()
        .filter(is_checks_read)
        .map(|c| c.args)
        .collect()
}

pub(super) fn feedback_data(fx: &Poll) -> Vec<Value> {
    fx.events("pr-feedback").into_iter().map(|e| e.1).collect()
}

pub(super) fn checks_of_row(fx: &Poll) -> String {
    fx.pr_data().2
}

#[test]
fn a_cancelled_check_among_pending_ones_raises_no_feedback() {
    let fx = failing_rollup();
    let mut nodes = green(19);
    nodes.push(run("bot", "COMPLETED", Some("CANCELLED")));
    nodes.push(status_context("ci/a", "PENDING"));
    nodes.push(status_context("ci/b", "PENDING"));
    read_says(&fx, one_page(nodes));

    fx.poll();

    assert_eq!(
        feedback_data(&fx),
        Vec::<Value>::new(),
        "a cancelled check is no failure"
    );
    assert_eq!(checks_of_row(&fx), "pending");
    assert_eq!(
        reads(&fx).len(),
        1,
        "the rollup's failed is confirmed by one checks read"
    );
}

#[test]
fn a_cancelled_check_among_green_ones_is_passed_and_raises_no_feedback() {
    let fx = failing_rollup();
    let mut nodes = green(10);
    nodes.push(run("skipped", "COMPLETED", Some("SKIPPED")));
    nodes.push(run("bot", "COMPLETED", Some("CANCELLED")));
    nodes.push(status_context("ci/ctx", "SUCCESS"));
    read_says(&fx, one_page(nodes));

    fx.poll();

    assert_eq!(feedback_data(&fx), Vec::<Value>::new());
    assert_eq!(checks_of_row(&fx), "passed");
}

#[test]
fn a_failing_check_beside_a_cancelled_one_raises_checks_failed() {
    let fx = failing_rollup();
    read_says(
        &fx,
        one_page(vec![
            run("bot", "COMPLETED", Some("CANCELLED")),
            run("ci/test", "COMPLETED", Some("FAILURE")),
        ]),
    );

    fx.poll();

    assert_eq!(
        reads(&fx).len(),
        1,
        "the rollup's failed is confirmed by one checks read"
    );
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["reasons"], json!(["checks_failed"]));
    assert_eq!(checks_of_row(&fx), "failed");
}

#[test]
fn each_failing_conclusion_raises_checks_failed() {
    let cases = [
        run("c", "COMPLETED", Some("FAILURE")),
        run("c", "COMPLETED", Some("TIMED_OUT")),
        run("c", "COMPLETED", Some("ACTION_REQUIRED")),
        run("c", "COMPLETED", Some("STARTUP_FAILURE")),
        status_context("c", "ERROR"),
        status_context("c", "FAILURE"),
    ];
    for failing in cases {
        let fx = failing_rollup();
        let mut nodes = green(2);
        nodes.push(failing.clone());
        read_says(&fx, one_page(nodes));

        fx.poll();

        assert_eq!(
            reads(&fx).len(),
            1,
            "{failing}: the rollup's failed is confirmed by one checks read"
        );
        let events = fx.events("pr-feedback");
        assert_eq!(events.len(), 1, "{failing}");
        assert_eq!(
            events[0].1["reasons"],
            json!(["checks_failed"]),
            "{failing}"
        );
        assert_eq!(checks_of_row(&fx), "failed", "{failing}");
    }
}

#[test]
fn a_real_failure_after_a_cancelled_poll_still_raises_feedback() {
    let fx = failing_rollup();
    let mut nodes = green(19);
    nodes.push(run("bot", "COMPLETED", Some("CANCELLED")));
    nodes.push(status_context("ci/a", "PENDING"));
    read_says(&fx, one_page(nodes));
    fx.poll();
    assert_eq!(feedback_data(&fx), Vec::<Value>::new());
    assert_eq!(checks_of_row(&fx), "pending");

    read_says(
        &fx,
        one_page(vec![
            run("bot", "COMPLETED", Some("CANCELLED")),
            run("ci/test", "COMPLETED", Some("FAILURE")),
        ]),
    );
    fx.poll();

    let events = fx.events("pr-feedback");
    assert_eq!(
        events.len(),
        1,
        "the stored value was the confirmed one: a transition"
    );
    assert_eq!(events[0].1["reasons"], json!(["checks_failed"]));
    assert_eq!(checks_of_row(&fx), "failed");
}

/// A second owned pull request, 813 of `acme/pay`, as PR `web` of Plan `pay`, open.
pub(super) fn add_second_owned_pr(fx: &Poll) {
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                   VALUES ('acme/pay', 813, 'https://github.com/acme/pay/pull/813', 'Pay the bills (web)', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
                 INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at,
                                  pr_number, pr_url, pr_opened_at, pull_request)
                   SELECT 'pay', 'web', repo, 'web', 'main', 'open', 'cpd/pay/web', lead_worktree, 'Show the receipt', shape, cpd_txn(), 't', 't',
                          813, 'https://github.com/acme/pay/pull/813', 'opened', (SELECT id FROM pull_requests WHERE number = 813)
                     FROM workflows WHERE key = 'repo';",
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn the_checks_are_read_only_for_an_owned_any_pr_whose_rollup_failed() {
    // Only an owned PR's FAILED rollup is confirmed here: the rollup of acme/web#902 is FAILURE but no Plan owns it. (The read of
    // an owned PR's PENDING rollup is pinned by pr_human_gated.) (One fixture at a time.)
    let fx = poll_fx("deploy");
    policy(&fx, "any");
    fx.search(&[
        pay("OPEN", None, Some("SUCCESS")),
        node("acme/web", 902, "OPEN", None, Some("FAILURE")),
    ]);
    fx.poll();
    assert_eq!(reads(&fx).len(), 0, "not owned");
    let rows = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 902",
                [],
                |r| r.get::<_, String>(0),
            )?)
        })
        .unwrap();
    assert_eq!(rows, "failed", "the search's value");

    // The policy says `none`: the row says failed, no Event, no read.
    drop(fx);
    let fx = poll_fx("deploy");
    policy(&fx, "none");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    fx.poll();
    assert_eq!(reads(&fx).len(), 0, "feedback.checks = none");
    assert_eq!(checks_of_row(&fx), "failed");
    assert!(feedback_data(&fx).is_empty());

    // Two owned `any` pull requests, both FAILURE: one call for both.
    drop(fx);
    let fx = poll_fx("deploy");
    policy(&fx, "any");
    add_second_owned_pr(&fx);
    fx.search(&[
        pay("OPEN", None, Some("FAILURE")),
        node("acme/pay", 813, "OPEN", None, Some("FAILURE")),
    ]);
    let both = answer(vec![contexts(green(2), None), contexts(green(2), None)]);
    read_says(&fx, both);
    fx.poll();
    let calls = reads(&fx);
    assert_eq!(calls.len(), 1, "one call for both: {calls:?}");
    assert!(
        calls[0][3].contains("p0:") && calls[0][3].contains("p1:"),
        "{}",
        calls[0][3]
    );
}

#[test]
fn more_than_one_hundred_checks_are_read_page_by_page() {
    let fx = failing_rollup();
    read_says(&fx, answer(vec![contexts(green(100), Some("c1"))]));
    let _ = (*fx.fakes.runner).clone().once(
        "gh",
        |c| is_checks_read(c) && c.args[3].contains(r#"after: "c1""#),
        Reply::ok(&one_page(vec![run("late", "COMPLETED", Some("FAILURE"))])),
    );

    fx.poll();

    assert_eq!(reads(&fx).len(), 2, "{:?}", fx.gh_calls());
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["reasons"], json!(["checks_failed"]));
}

#[test]
fn a_failed_checks_read_fails_the_poll_and_writes_nothing() {
    let fx = failing_rollup();
    let _ = (*fx.fakes.runner).clone().on(
        "gh",
        is_checks_read,
        Reply::exit(1, "", "gh: HTTP 502: Bad Gateway\n"),
    );
    let (txns, events, row) = (fx.txns(), fx.event_count(), fx.pr_data());

    let run = fx.cpd(&["pr", "poll", "--plan", "pay"]);

    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.error()["code"], "gh_failed");
    let details = &run.error()["details"];
    assert_eq!(
        (&details["call"], &details["transient"]),
        (&json!("lookup"), &json!(true)),
        "{details}"
    );
    assert_eq!(
        (fx.txns(), fx.event_count(), fx.pr_data()),
        (txns, events, row)
    );
}

/// The `gh pr view` reply with `checks` as its `statusCheckRollup`.
pub(super) fn view_with(checks: Vec<Value>) -> String {
    json!({"state": "OPEN", "mergedAt": null, "title": "Pay the bills", "url": "https://github.com/acme/pay/pull/812",
           "reviewDecision": null, "statusCheckRollup": checks, "reviews": [], "comments": []})
    .to_string()
        + "\n"
}

/// `pr view` of a PR whose `gh pr view` holds `checks`: its output.
pub(super) fn view(checks: Vec<Value>) -> Value {
    let fx = pr_view::fx("open");
    let _ = (*fx.fx.fakes.runner).clone().on(
        "gh",
        |c| c.args.starts_with(&["pr".into(), "view".into()]),
        Reply::ok(&view_with(checks)),
    );
    fx.fx
        .cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"])
        .ok()
}

#[test]
fn pr_view_does_not_count_a_cancelled_check_as_failed() {
    let cancelled = json!({"__typename": "CheckRun", "name": "bot", "conclusion": "CANCELLED", "status": "COMPLETED"});
    let pending =
        |n: &str| json!({"__typename": "StatusContext", "context": n, "state": "PENDING"});
    let success =
        |n: &str| json!({"__typename": "StatusContext", "context": n, "state": "SUCCESS"});
    let failure = json!({"__typename": "CheckRun", "name": "ci", "conclusion": "FAILURE", "status": "COMPLETED"});

    let out = view(vec![cancelled.clone(), pending("a"), pending("b")]);
    assert_eq!(out["pr"]["checks"], "pending");
    let listed = out["checks"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .any(|c| c["name"] == "bot" && c["conclusion"] == "CANCELLED"),
        "{listed:?}"
    );

    let out = view(vec![cancelled.clone(), success("a"), success("b")]);
    assert_eq!(out["pr"]["checks"], "passed");

    let out = view(vec![cancelled, failure]);
    assert_eq!(out["pr"]["checks"], "failed");
}
