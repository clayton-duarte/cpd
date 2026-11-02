//! `review submit` (commands.md §31.1, §21.9; pro-d107, pro-d108, pro-d110; A4, A5), run in-process on `run_with` over a
//! `gh` handler that the runner records. The command needs no Plan, Context or PR key; the queue is read through SQL.

use crate::common;

use std::sync::Arc;

use common::fx::Fx;
use common::refused;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use serde_json::{Value, json};

const REF: &str = "acme/api#7";
const URL: &str = "https://github.com/acme/api/pull/7";
const CHANNEL: &str = "C02JU6C2Z1D";
const TS: &str = "1793040197.490209";
const SLACK: &str = "C02JU6C2Z1D/1793040197.490209";
const REVIEW_URL: &str = "https://github.com/acme/api/pull/7#pullrequestreview-4242";

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const MOVED: &str = "fedcba9876543210fedcba9876543210fedcba98";
const POST: [&str; 6] = [
    "api",
    "--method",
    "POST",
    "repos/acme/api/pulls/7/reviews",
    "--input",
    "-",
];
const GET: [&str; 2] = ["api", "repos/acme/api/pulls/7"];

/// The in-process `gh`: an `api` call with `POST` answers `post`, any other `api` call (the head read) answers `head`;
/// any other call exits 9.
struct Gh {
    head: Reply,
    post: Reply,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        match call.args.first().map(String::as_str) {
            Some("api") if call.args.iter().any(|a| a == "POST") => self.post.clone(),
            Some("api") => self.head.clone(),
            _ => Reply::exit(9, "", ""),
        }
    }
}

fn answer() -> Reply {
    Reply::ok(&json!({"id": 4242, "html_url": REVIEW_URL, "state": "APPROVED"}).to_string())
}

fn head_at(sha: &str) -> Reply {
    Reply::ok(
        &json!({"number": 7, "state": "open", "title": "t", "head": {"ref": "topic", "sha": sha}})
            .to_string(),
    )
}

fn head() -> Reply {
    head_at(HEAD)
}

/// A fixture whose `gh api` answers `head` (the read) and `post` (the review), with a store that exists.
fn fixture(head: Reply, post: Reply) -> Fx {
    let fx = Fx::with_ports();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql("SELECT 1");
    let _ = (*fx.fakes.runner)
        .clone()
        .route("gh", Arc::new(Gh { head, post }));
    fx
}

/// Writes `text` as the review file and returns its path.
fn input(fx: &Fx, text: &str) -> String {
    let path = fx.env.cpd_home().join("review.json");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

fn review() -> Value {
    json!({"body": "Looks right.",
           "comments": [{"path": "src/a.rs", "line": 3, "side": "RIGHT", "body": "nit"}]})
}

fn submit(
    fx: &Fx,
    pr: &str,
    head: &str,
    event: &str,
    file: &str,
    slack: Option<&str>,
) -> common::Run {
    let mut args = vec![
        "review", "submit", "--pr", pr, "--head", head, "--event", event, "--input", file,
    ];
    if let Some(slack) = slack {
        args.extend(["--slack", slack]);
    }
    fx.cpd(&args)
}

fn rows(fx: &Fx) -> Vec<Value> {
    let text: String = fx.env.query(
        "SELECT coalesce(json_group_array(json_object('channel', channel, 'ts', ts, 'emoji', emoji, 'reacted_at', reacted_at)), '[]')
           FROM (SELECT * FROM slack_reactions ORDER BY id)",
    );
    serde_json::from_str(&text).unwrap()
}

fn gh_calls(fx: &Fx) -> Vec<Call> {
    fx.calls("gh")
}

fn message(error: &Value) -> String {
    error["message"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn approve_posts_the_review_and_queues_the_check_mark() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(
        out,
        json!({"ok": true, "review": {"id": 4242, "url": REVIEW_URL, "state": "APPROVED", "commit_id": HEAD},
               "reaction": {"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "already": false}})
    );
    assert_eq!(
        rows(&fx),
        [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "reacted_at": null})]
    );
}

#[test]
fn request_changes_posts_the_review_and_queues_changes_requested() {
    let fx = fixture(
        head(),
        Reply::ok(
            &json!({"id": 4242, "html_url": REVIEW_URL, "state": "CHANGES_REQUESTED"}).to_string(),
        ),
    );
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "REQUEST_CHANGES", &file, Some(SLACK)).ok();
    assert_eq!(
        out["review"],
        json!({"id": 4242, "url": REVIEW_URL, "state": "CHANGES_REQUESTED", "commit_id": HEAD})
    );
    assert_eq!(
        out["reaction"],
        json!({"channel": CHANNEL, "ts": TS, "emoji": "github-changes-requested", "already": false})
    );
    assert_eq!(
        rows(&fx),
        [
            json!({"channel": CHANNEL, "ts": TS, "emoji": "github-changes-requested", "reacted_at": null})
        ]
    );
}

#[test]
fn the_input_is_sent_with_the_event_and_commit_id_added_after_the_head_read() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    submit(&fx, REF, HEAD, "APPROVE", &file, None).ok();
    let calls = gh_calls(&fx);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].args, GET);
    assert_eq!(calls[1].args, POST);
    let mut want = review();
    want["event"] = json!("APPROVE");
    want["commit_id"] = json!(HEAD);
    let sent: Value =
        serde_json::from_slice(calls[1].stdin.as_deref().expect("the review on stdin")).unwrap();
    assert_eq!(sent, want);
}

#[test]
fn a_pr_url_and_owner_repo_hash_n_name_the_same_pull_request() {
    for pr in [REF, URL, &format!("{URL}/")] {
        let fx = fixture(head(), answer());
        let file = input(&fx, &review().to_string());
        submit(&fx, pr, HEAD, "APPROVE", &file, None).ok();
        let calls = gh_calls(&fx);
        assert_eq!(calls.len(), 2, "{pr}: {calls:?}");
        assert_eq!(calls[0].args, GET, "{pr}");
        assert_eq!(calls[1].args, POST, "{pr}");
    }
}

#[test]
fn a_second_submit_posts_a_second_review_and_queues_one_row() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    let first = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    let second = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(first["reaction"]["already"], false);
    assert_eq!(second["reaction"]["already"], true);
    assert_eq!(
        gh_calls(&fx).len(),
        4,
        "a rerun reads the head and posts a second review"
    );
    assert_eq!(rows(&fx).len(), 1);
}

#[test]
fn without_slack_nothing_is_queued_and_reaction_is_null() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "REQUEST_CHANGES", &file, None).ok();
    assert_eq!(out["reaction"], Value::Null);
    assert_eq!(out["review"]["id"], 4242);
    assert!(rows(&fx).is_empty());
}

#[test]
fn an_input_carrying_an_event_is_refused_before_any_call() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &json!({"body": "x", "event": "COMMENT"}).to_string());
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
        "usage",
        2,
    );
    assert!(message(&error).contains("event"), "{error}");
    assert!(message(&error).contains("--input"), "{error}");
    assert!(gh_calls(&fx).is_empty());
}

#[test]
fn an_input_that_is_not_a_json_object_is_refused_before_any_call() {
    for text in ["[1]", "\"x\"", "not json", ""] {
        let fx = fixture(head(), answer());
        let file = input(&fx, text);
        let error = refused(&submit(&fx, REF, HEAD, "APPROVE", &file, None), "usage", 2);
        assert!(message(&error).contains("--input"), "{text}: {error}");
        assert!(gh_calls(&fx).is_empty(), "{text}");
    }
    let fx = fixture(head(), answer());
    let missing = fx.env.cpd_home().join("missing.json").display().to_string();
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &missing, None),
        "usage",
        2,
    );
    assert!(message(&error).contains("--input"), "{error}");
    assert!(gh_calls(&fx).is_empty());
}

#[test]
fn a_malformed_pr_event_or_slack_target_is_refused_before_any_call() {
    let cases: [(&str, &str, Option<&str>, &str); 12] = [
        ("foo", "APPROVE", None, "--pr"),
        ("acme/api#0", "APPROVE", None, "--pr"),
        (REF, "COMMENT", None, "--event"),
        (REF, "approve", None, "--event"),
        (REF, "", None, "--event"),
        (REF, "APPROVE", Some("C02JU6C2Z1D"), "--slack"),
        (
            REF,
            "APPROVE",
            Some("c02JU6C2Z1D/1793040197.490209"),
            "--slack",
        ),
        (REF, "APPROVE", Some("C02JU6C/1793040197.490209"), "--slack"),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/179304019.490209"),
            "--slack",
        ),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/1793040197490209"),
            "--slack",
        ),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/1793040197.49020"),
            "--slack",
        ),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/1793040197.490209/x"),
            "--slack",
        ),
    ];
    for (pr, event, slack, flag) in cases {
        let fx = fixture(head(), answer());
        let file = input(&fx, &review().to_string());
        let error = refused(&submit(&fx, pr, HEAD, event, &file, slack), "usage", 2);
        assert!(
            message(&error).contains(flag),
            "{pr} {event} {slack:?}: {error}"
        );
        assert!(gh_calls(&fx).is_empty(), "{pr} {event} {slack:?}");
    }
}

#[test]
fn a_failed_post_queues_nothing_and_is_gh_failed() {
    let fx = fixture(
        head(),
        Reply::exit(1, "", "HTTP 422: Unprocessable Entity\n"),
    );
    let file = input(&fx, &review().to_string());
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
        "gh_failed",
        1,
    );
    assert_eq!(error["details"]["stderr"], "HTTP 422: Unprocessable Entity");
    assert_eq!(gh_calls(&fx).len(), 2);
    assert!(rows(&fx).is_empty());
}

#[test]
fn an_answer_without_a_review_id_is_gh_failed_and_queues_nothing() {
    for body in [
        json!({"html_url": REVIEW_URL, "state": "APPROVED"}),
        json!({"id": 4242, "state": "APPROVED"}),
    ] {
        let fx = fixture(head(), Reply::ok(&body.to_string()));
        let file = input(&fx, &review().to_string());
        let error = refused(
            &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
            "gh_failed",
            1,
        );
        assert!(
            message(&error).contains("review may have been posted"),
            "{error}"
        );
        assert!(rows(&fx).is_empty());
    }
}

#[test]
fn a_missing_head_is_refused_usage_before_any_call() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    let run = fx.cpd(&[
        "review", "submit", "--pr", REF, "--event", "APPROVE", "--input", &file, "--slack", SLACK,
    ]);
    refused(&run, "usage", 2);
    assert!(gh_calls(&fx).is_empty());
    assert!(rows(&fx).is_empty());
}

#[test]
fn a_head_that_is_not_forty_hex_digits_is_refused_usage_before_any_call() {
    let bad = [
        String::new(),
        HEAD[..39].to_owned(),
        format!("{HEAD}0"),
        format!("{}g", &HEAD[..39]),
        format!("0x{}", &HEAD[..38]),
        format!(" {}", &HEAD[..39]),
        "main".to_owned(),
        "abc1234".to_owned(),
    ];
    for value in bad {
        let fx = fixture(head(), answer());
        let file = input(&fx, &review().to_string());
        let error = refused(
            &submit(&fx, REF, &value, "APPROVE", &file, Some(SLACK)),
            "usage",
            2,
        );
        assert!(message(&error).contains("--head"), "{value:?}: {error}");
        assert!(message(&error).contains("40"), "{value:?}: {error}");
        assert!(gh_calls(&fx).is_empty(), "{value:?}");
        assert!(rows(&fx).is_empty(), "{value:?}");
    }
}

#[test]
fn an_input_carrying_a_commit_id_is_refused_usage_before_any_call() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &json!({"body": "x", "commit_id": HEAD}).to_string());
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
        "usage",
        2,
    );
    for word in ["commit_id", "--input", "--head"] {
        assert!(message(&error).contains(word), "{word}: {error}");
    }
    assert!(gh_calls(&fx).is_empty());
    assert!(rows(&fx).is_empty());
}

#[test]
fn a_matching_head_reads_the_head_then_posts_the_review_pinned_to_it() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(
        out["review"],
        json!({"id": 4242, "url": REVIEW_URL, "state": "APPROVED", "commit_id": HEAD})
    );
    let calls = gh_calls(&fx);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].args, GET);
    assert_eq!(calls[1].args, POST);
    let mut want = review();
    want["event"] = json!("APPROVE");
    want["commit_id"] = json!(HEAD);
    let sent: Value =
        serde_json::from_slice(calls[1].stdin.as_deref().expect("the review on stdin")).unwrap();
    assert_eq!(sent, want);
    assert_eq!(
        rows(&fx),
        [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "reacted_at": null})]
    );
}

#[test]
fn an_upper_case_head_is_accepted_and_used_as_lowercase() {
    let fx = fixture(head(), answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, &HEAD.to_uppercase(), "APPROVE", &file, None).ok();
    assert_eq!(out["review"]["commit_id"], HEAD);
    let calls = gh_calls(&fx);
    assert_eq!(calls.len(), 2, "{calls:?}");
    let sent: Value =
        serde_json::from_slice(calls[1].stdin.as_deref().expect("the review on stdin")).unwrap();
    assert_eq!(sent["commit_id"], HEAD);
    drop(fx);

    let fx = fixture(head_at(&HEAD.to_uppercase()), answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "APPROVE", &file, None).ok();
    assert_eq!(out["review"]["commit_id"], HEAD);
    assert_eq!(gh_calls(&fx).len(), 2);
}

#[test]
fn a_moved_head_is_refused_head_moved_and_nothing_is_posted_or_queued() {
    for (pr, given) in [
        (REF, HEAD.to_owned()),
        (URL, HEAD.to_owned()),
        (REF, HEAD.to_uppercase()),
    ] {
        let fx = fixture(head_at(MOVED), answer());
        let file = input(&fx, &review().to_string());
        let error = refused(
            &submit(&fx, pr, &given, "APPROVE", &file, Some(SLACK)),
            "head_moved",
            3,
        );
        assert_eq!(
            error["details"],
            json!({"pr": REF, "expected": HEAD, "actual": MOVED}),
            "{pr} {given}"
        );
        assert!(message(&error).contains(HEAD), "{error}");
        assert!(message(&error).contains(MOVED), "{error}");
        let calls = gh_calls(&fx);
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0].args, GET);
        assert!(rows(&fx).is_empty());
    }
}

#[test]
fn a_head_read_that_fails_is_gh_failed_and_nothing_is_posted() {
    let fx = fixture(Reply::exit(1, "", "HTTP 404: Not Found\n"), answer());
    let file = input(&fx, &review().to_string());
    refused(
        &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
        "gh_failed",
        1,
    );
    assert_eq!(gh_calls(&fx).len(), 1);
    assert!(rows(&fx).is_empty());
}

#[test]
fn a_head_answer_without_a_head_sha_is_gh_failed_and_nothing_is_posted() {
    for stdout in [
        "not json",
        "{}",
        "{\"head\":{}}",
        "{\"head\":{\"sha\":7}}",
        "[]",
    ] {
        let fx = fixture(Reply::ok(stdout), answer());
        let file = input(&fx, &review().to_string());
        refused(
            &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
            "gh_failed",
            1,
        );
        assert_eq!(gh_calls(&fx).len(), 1, "{stdout}");
        assert!(rows(&fx).is_empty(), "{stdout}");
    }
}
