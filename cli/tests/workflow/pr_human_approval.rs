//! The human-approval rule (pro-d158 rule 1, contract §7.6): `review_decision` is `approved` only when GitHub's reviewDecision is
//! APPROVED and a human approved. The poll, its direct read and `pr view`, all over the mocked `gh` of `pr.rs`: no real GitHub.

use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

use super::pr_view;
use super::workflow::{Poll, Rows, node, pay, poll_fx, reviewed, viewer};

const APPROVED: &str = "APPROVED";

/// The stored `review_decision` of PR 812 after one poll that finds it as `pull`.
fn polled(fx: &Poll, pull: String) -> String {
    fx.search(&[pull]);
    fx.poll();
    fx.pr_data().1
}

/// Plan `pay`'s PR, reviewDecision APPROVED, its latest opinionated reviews `reviewers`.
fn approved_by(reviewers: &[(&str, &str, &str)]) -> String {
    reviewed(pay("OPEN", Some(APPROVED), None), reviewers)
}

fn bot_list(fx: &Poll, list: &str) {
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"t\"\n[inbox]\nbot_reviewers = {list}\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
}

#[test]
fn human_approval_a_bot_only_approval_is_stored_as_none() {
    let fx = poll_fx("deploy");

    assert_eq!(
        polled(&fx, approved_by(&[("Bot", "acme-review-app", "APPROVED")])),
        "none"
    );
}

#[test]
fn human_approval_a_bot_is_known_by_its_type_its_suffix_or_the_default_list() {
    for reviewer in [
        ("Bot", "acme-review-app", "APPROVED"),
        ("User", "ci-app[bot]", "APPROVED"),
        ("User", "github-actions", "APPROVED"),
    ] {
        let fx = poll_fx("deploy");
        assert_eq!(
            polled(&fx, approved_by(&[reviewer])),
            "none",
            "{reviewer:?}"
        );
        drop(fx);
    }
}

#[test]
fn human_approval_the_configured_bot_list_replaces_the_default() {
    let fx = poll_fx("deploy");
    bot_list(&fx, r#"["ci-bot"]"#);
    assert_eq!(
        polled(&fx, approved_by(&[("User", "ci-bot", "APPROVED")])),
        "none",
        "a listed login is a bot"
    );
    drop(fx);

    let fx = poll_fx("deploy");
    bot_list(&fx, r#"["ci-bot"]"#);
    assert_eq!(
        polled(&fx, approved_by(&[("User", "github-actions", "APPROVED")])),
        "approved",
        "the default list no longer applies"
    );
}

#[test]
fn human_approval_approved_with_no_listed_reviewer_is_none() {
    let fx = poll_fx("deploy");
    assert_eq!(polled(&fx, approved_by(&[])), "none", "empty nodes");
    drop(fx);

    let fx = poll_fx("deploy");
    let no_key = pay("OPEN", None, None).replace(
        r#""reviewDecision":null,"#,
        r#""reviewDecision":"APPROVED","#,
    );
    assert!(!no_key.contains("latestOpinionatedReviews"));
    assert_eq!(
        polled(&fx, no_key),
        "none",
        "no latestOpinionatedReviews key"
    );
    drop(fx);

    let fx = poll_fx("deploy");
    let null_author = approved_by(&[("User", "ghost", "APPROVED")])
        .replace(r#"{"__typename":"User","login":"ghost"}"#, "null");
    assert!(null_author.contains(r#""author":null"#));
    assert_eq!(polled(&fx, null_author), "none", "an author that is null");
}

#[test]
fn human_approval_the_direct_read_applies_the_same_rule() {
    let fx = poll_fx("deploy");
    let plain = node("acme/pay", 812, "OPEN", None, None);
    assert_eq!(polled(&fx, plain.clone()), "none");

    // The search no longer lists it: the poll reads it directly.
    fx.search(&[]);
    fx.lookup(&[Some(viewer(
        approved_by(&[("Bot", "acme-review-app", "APPROVED")]),
        true,
        false,
    ))]);
    fx.poll();

    assert_eq!(fx.pr_data().1, "none");
}

/// `pr view` of an open PR whose `gh pr view` says `decision` and `reviews`: its `pr.review_decision`.
fn view_decision(decision: &str, reviews: Value) -> Value {
    let fx = pr_view::fx("open");
    let reply = json!({"state": "OPEN", "mergedAt": null, "title": "Pay the bills", "url": "https://github.com/acme/pay/pull/812",
                       "reviewDecision": decision, "statusCheckRollup": [], "reviews": reviews, "comments": []});
    let _ = (*fx.fx.fakes.runner).clone().on(
        "gh",
        |c| c.args.starts_with(&["pr".into(), "view".into()]),
        Reply::ok(&format!("{reply}\n")),
    );
    fx.fx
        .cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"])
        .ok()["pr"]["review_decision"]
        .clone()
}

fn review(login: &str, state: &str) -> Value {
    json!([{"id": "R1", "author": {"login": login}, "state": state, "body": ""}])
}

#[test]
fn human_approval_pr_view_a_bot_only_approval_is_none() {
    assert_eq!(
        view_decision("APPROVED", review("ci-app[bot]", "APPROVED")),
        "none"
    );
    assert_eq!(
        view_decision("APPROVED", review("github-actions", "APPROVED")),
        "none"
    );
}

// Guards: these pass today and after.

#[test]
fn human_approval_a_human_beside_a_bot_is_approved() {
    let fx = poll_fx("deploy");
    let both = approved_by(&[
        ("Bot", "acme-review-app", "APPROVED"),
        ("User", "amy", "APPROVED"),
    ]);
    assert_eq!(polled(&fx, both), "approved");
}

#[test]
fn human_approval_changes_requested_and_review_required_keep_their_meaning() {
    let fx = poll_fx("deploy");
    let pull = reviewed(
        pay("OPEN", Some("CHANGES_REQUESTED"), None),
        &[("Bot", "acme-review-app", "CHANGES_REQUESTED")],
    );
    assert_eq!(polled(&fx, pull), "changes_requested");
    drop(fx);

    let fx = poll_fx("deploy");
    let pull = reviewed(
        pay("OPEN", Some("REVIEW_REQUIRED"), None),
        &[("Bot", "acme-review-app", "APPROVED")],
    );
    assert_eq!(polled(&fx, pull), "none");
}
