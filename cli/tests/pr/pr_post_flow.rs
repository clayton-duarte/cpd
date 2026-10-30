//! `pr poll`'s reasons `ready_to_post` and `approved` (contract §5.2 `pr-feedback`, §7.6; pro-d64): a posting owner's own PR
//! that turns ready to post, and a posted PR that a peer approves. All in-process over the mock `gh` of `pr.rs`: no real GitHub.

use serde_json::{Value, json};

use super::pr::{Poll, Rows, node, poll_fx, reviewed, viewer};

/// How the owner's policy file treats posting.
#[derive(Clone, Copy, PartialEq)]
enum Posting {
    Enabled,
    Disabled,
    Absent,
}

fn page(nodes: Vec<Value>) -> Value {
    json!({"totalCount": nodes.len(), "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": nodes})
}

fn comment(id: &str, login: &str) -> Value {
    json!({"id": id, "author": {"login": login}, "body": "Please change this", "url": format!("https://github.com/acme/pay/pull/812#{id}"),
           "createdAt": "2026-10-20T09:00:00Z"})
}

/// An open thread whose first and last comment is by `login`.
fn thread(id: &str, login: &str) -> Value {
    let last = format!("{id}-c");
    json!({"id": id, "isResolved": false, "isOutdated": false, "path": "src/a.rs", "comments": page(vec![comment(&last, login)])})
}

/// The comments read of pull request 812: its `updatedAt` and its threads.
fn answer(updated_at: &str, threads: Vec<Value>) -> String {
    let empty = page(vec![]);
    json!({"data": {"viewer": {"login": "me"}, "rateLimit": {"cost": 1, "remaining": 4900, "resetAt": "2026-10-20T11:00:00Z"},
        "p0": {"pullRequest": {"id": "PR_1", "updatedAt": updated_at, "reviewThreads": page(threads), "comments": empty, "reviews": empty}}}})
    .to_string()
}

/// Plan `pay`'s PR on a fixture whose owner policy says `posting`, `checks` for `pr.feedback.checks`, and which has a
/// `pr_posts` row when `posted`.
fn fixture(posting: Posting, posted: bool, checks: &str) -> Poll {
    let fx = poll_fx("deploy");
    let post = match posting {
        Posting::Enabled => "[pr.post]\nenabled = true\n",
        Posting::Disabled => "[pr.post]\nenabled = false\n",
        Posting::Absent => "",
    };
    fx.env.owner_policy("acme", &format!("[pr]\ngate = \"local\"\n[pr.feedback]\nchecks = \"{checks}\"\n{post}"));
    if posted {
        fx.store()
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at)
                     VALUES ('pay', 'repo', 'C02JU6C2Z1D', '1793040197.490209', 'https://example.slack.com/p1', 'opened', NULL, NULL)",
                )
                .map_err(Into::into)
            })
            .unwrap();
    }
    fx
}

/// One poll with pull request 812 as GitHub answers: updated at `updated_at`, its checks `rollup`, its `review` decision, open
/// `threads`; `listed_as` is the list the search puts it in: `authored` or `requested`.
fn poll(fx: &Poll, updated_at: &str, rollup: &str, review: Option<&str>, threads: Vec<Value>, listed_as: &str) {
    let pull = node("acme/pay", 812, "OPEN", review, Some(rollup))
        .replace(r#""updatedAt":"U1""#, &format!(r#""updatedAt":"{updated_at}""#))
        .replace(r#""mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN""#, r#""mergeable":"MERGEABLE","mergeStateStatus":"CLEAN""#);
    if listed_as != "authored" {
        // The row was stored as the User's own: the poll reads it directly, and GitHub says it is review-requested of them.
        fx.lookup(&[Some(viewer(pull.clone(), false, true))]);
    }
    let (authored, requested) = if listed_as == "authored" { (vec![pull], vec![]) } else { (vec![], vec![pull]) };
    fx.search_raw(&format!(
        r#"{{"data":{{"search":{{"issueCount":{},"nodes":[{}]}},"reviewRequests":{{"issueCount":{},"nodes":[{}]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#,
        authored.len(),
        authored.join(","),
        requested.len(),
        requested.join(",")
    ));
    *fx.gh.comments.lock().unwrap() = Some(answer(updated_at, threads));
    fx.poll();
}

/// `poll` of an authored pull request.
fn step(fx: &Poll, updated_at: &str, rollup: &str, review: Option<&str>, threads: Vec<Value>) {
    poll(fx, updated_at, rollup, review, threads, "authored");
}

/// The `reasons` of every `pr-feedback` Event so far.
fn reasons(fx: &Poll) -> Vec<Vec<String>> {
    fx.events("pr-feedback").iter().map(|(_, data)| data["reasons"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_owned()).collect()).collect()
}

fn told(reasons: &[&[&str]]) -> Vec<Vec<String>> {
    reasons.iter().map(|r| r.iter().map(|s| (*s).to_owned()).collect()).collect()
}

/// The PR turns ready: checks pending (no thread), then passed. Returns the reasons told.
fn becomes_ready(fx: &Poll) -> Vec<Vec<String>> {
    step(fx, "U1", "PENDING", None, vec![]);
    step(fx, "U2", "SUCCESS", None, vec![]);
    reasons(fx)
}

fn flat(reasons: &[Vec<String>]) -> Vec<&str> {
    reasons.iter().flatten().map(String::as_str).collect()
}

#[test]
fn a_pr_that_becomes_ready_tells_the_lead_ready_to_post_once() {
    let fx = fixture(Posting::Enabled, false, "any");
    assert_eq!(becomes_ready(&fx), told(&[&["ready_to_post"]]));
    step(&fx, "U2", "SUCCESS", None, vec![]);
    step(&fx, "U2", "SUCCESS", None, vec![]);
    assert_eq!(reasons(&fx), told(&[&["ready_to_post"]]));
}

#[test]
fn an_answered_bot_comment_that_makes_the_pr_ready_tells_comments_then_ready_to_post() {
    let fx = fixture(Posting::Enabled, false, "any");
    step(&fx, "U1", "SUCCESS", None, vec![thread("T1", "review-bot[bot]")]);
    assert_eq!(reasons(&fx), told(&[&["comments"]]), "the open bot thread is a comment");
    step(&fx, "U2", "SUCCESS", None, vec![]);
    assert_eq!(reasons(&fx), told(&[&["comments"], &["ready_to_post"]]), "the bot thread is answered: the PR is ready");
}

#[test]
fn a_resolved_bot_thread_with_an_unchanged_updated_at_tells_the_lead_ready_to_post() {
    let fx = fixture(Posting::Enabled, false, "any");
    step(&fx, "U1", "SUCCESS", None, vec![thread("T1", "review-bot[bot]")]);
    assert_eq!(reasons(&fx), told(&[&["comments"]]));
    let mut resolved = thread("T1", "review-bot[bot]");
    resolved["isResolved"] = json!(true);
    step(&fx, "U1", "SUCCESS", None, vec![resolved]);
    assert_eq!(reasons(&fx), told(&[&["comments"], &["ready_to_post"]]));
}

#[test]
fn a_ready_pr_with_an_open_bot_comment_is_not_told_ready_to_post() {
    let fx = fixture(Posting::Enabled, false, "any");
    step(&fx, "U1", "SUCCESS", None, vec![thread("T1", "review-bot[bot]")]);
    step(&fx, "U2", "SUCCESS", None, vec![thread("T1", "review-bot[bot]")]);
    assert_eq!(reasons(&fx), told(&[&["comments"]]));
}

#[test]
fn a_pr_that_becomes_ready_with_a_new_human_comment_is_told_both_in_one_event() {
    let fx = fixture(Posting::Enabled, false, "any");
    step(&fx, "U1", "PENDING", None, vec![]);
    step(&fx, "U2", "SUCCESS", None, vec![thread("T1", "rev")]);
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].1["reasons"], json!(["comments", "ready_to_post"]));
    assert!(events[0].0.contains("ready to post for review"), "{}", events[0].0);
}

#[test]
fn ready_to_post_needs_an_enabled_unposted_authored_pr() {
    let fx = fixture(Posting::Enabled, false, "any");
    assert_eq!(becomes_ready(&fx), told(&[&["ready_to_post"]]));
    drop(fx);
    for (posting, posted) in [(Posting::Enabled, true), (Posting::Disabled, false), (Posting::Absent, false)] {
        let fx = fixture(posting, posted, "any");
        assert_eq!(becomes_ready(&fx), told(&[]));
        drop(fx);
    }
    let fx = fixture(Posting::Enabled, false, "any");
    poll(&fx, "U1", "PENDING", None, vec![], "requested");
    poll(&fx, "U2", "SUCCESS", None, vec![], "requested");
    assert_eq!(reasons(&fx), told(&[]));
}

#[test]
fn a_pr_that_stops_being_ready_and_is_ready_again_is_told_again() {
    let fx = fixture(Posting::Enabled, false, "any");
    step(&fx, "U1", "SUCCESS", None, vec![]);
    step(&fx, "U2", "FAILURE", None, vec![]);
    step(&fx, "U3", "SUCCESS", None, vec![]);
    assert_eq!(reasons(&fx), told(&[&["ready_to_post"], &["checks_failed"], &["ready_to_post"]]));
}

#[test]
fn a_posted_pr_that_is_approved_tells_the_lead_approved_once() {
    let fx = fixture(Posting::Enabled, true, "any");
    step(&fx, "U1", "SUCCESS", None, vec![]);
    assert!(fx.events("pr-feedback").is_empty(), "a posted PR is told no ready_to_post");

    step(&fx, "U2", "SUCCESS", Some("APPROVED"), vec![]);
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].1["reasons"], json!(["approved"]));
    assert!(events[0].0.contains("needs attention: approved. Read"), "{}", events[0].0);

    step(&fx, "U2", "SUCCESS", Some("APPROVED"), vec![]);
    assert_eq!(fx.events("pr-feedback").len(), 1, "told once");

    step(&fx, "U3", "SUCCESS", Some("CHANGES_REQUESTED"), vec![]);
    step(&fx, "U4", "SUCCESS", Some("APPROVED"), vec![]);
    assert_eq!(reasons(&fx), told(&[&["approved"], &["changes_requested"], &["approved"]]));
}

#[test]
fn an_unposted_pr_or_a_disabled_owner_hears_no_approved() {
    let control = fixture(Posting::Enabled, true, "any");
    step(&control, "U1", "SUCCESS", None, vec![]);
    step(&control, "U2", "SUCCESS", Some("APPROVED"), vec![]);
    assert_eq!(reasons(&control), told(&[&["approved"]]), "the positive control");
    drop(control);

    let unposted = fixture(Posting::Enabled, false, "any");
    step(&unposted, "U1", "SUCCESS", None, vec![]);
    step(&unposted, "U2", "SUCCESS", Some("APPROVED"), vec![]);
    assert!(!flat(&reasons(&unposted)).contains(&"approved"), "{:?}", reasons(&unposted));
    drop(unposted);

    for posting in [Posting::Absent, Posting::Disabled] {
        let fx = fixture(posting, true, "any");
        step(&fx, "U1", "SUCCESS", None, vec![]);
        step(&fx, "U2", "SUCCESS", Some("APPROVED"), vec![]);
        assert_eq!(reasons(&fx), told(&[]));
        drop(fx);
    }
}

#[test]
fn the_approved_reason_follows_the_old_ones_in_the_documented_order() {
    let posted = fixture(Posting::Enabled, true, "any");
    step(&posted, "U1", "SUCCESS", None, vec![]);
    step(&posted, "U2", "FAILURE", Some("APPROVED"), vec![]);
    let events = posted.events("pr-feedback");
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].1["reasons"], json!(["checks_failed", "approved"]));
    assert!(events[0].0.contains("needs attention: checks failed, approved."), "{}", events[0].0);
}

#[test]
fn human_approval_a_bot_only_approval_tells_a_posted_pr_nothing() {
    let fx = fixture(Posting::Enabled, true, "any");
    step(&fx, "U1", "SUCCESS", None, vec![]);

    let bot_only = reviewed(
        node("acme/pay", 812, "OPEN", Some("APPROVED"), Some("SUCCESS"))
            .replace(r#""updatedAt":"U1""#, r#""updatedAt":"U2""#)
            .replace(r#""mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN""#, r#""mergeable":"MERGEABLE","mergeStateStatus":"CLEAN""#),
        &[("Bot", "acme-review-app", "APPROVED")],
    );
    fx.search(&[bot_only]);
    *fx.gh.comments.lock().unwrap() = Some(answer("U2", vec![]));
    fx.poll();

    assert!(!flat(&reasons(&fx)).contains(&"approved"), "{:?}", reasons(&fx));
    assert_eq!(fx.pr_data().1, "none");
}
