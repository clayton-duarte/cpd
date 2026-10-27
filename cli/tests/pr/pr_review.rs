//! `review submit` (commands.md §31.1, §21.9; pro-d107, pro-d108, pro-d110; A4, A5), run in-process on `run_with` over a
//! `gh` handler that the runner records. The command needs no Plan, Context or PR key; the queue is read through SQL.

use crate::common;

use std::sync::{Arc, Mutex};

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

/// The in-process `gh`: `api` answers `reply`; any other call exits 9.
struct Gh {
    reply: Mutex<Reply>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        match call.args.first().map(String::as_str) {
            Some("api") => self.reply.lock().unwrap().clone(),
            _ => Reply::exit(9, "", ""),
        }
    }
}

fn answer() -> Reply {
    Reply::ok(&json!({"id": 4242, "html_url": REVIEW_URL, "state": "APPROVED"}).to_string())
}

/// A fixture whose `gh api` answers `reply`, with a store that exists.
fn fixture(reply: Reply) -> Fx {
    let fx = Fx::with_ports();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql("SELECT 1");
    let _ = (*fx.fakes.runner).clone().route("gh", Arc::new(Gh { reply: Mutex::new(reply) }));
    fx
}

/// Writes `text` as the review file and returns its path.
fn input(fx: &Fx, text: &str) -> String {
    let path = fx.env.cpd_home().join("review.json");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

fn review() -> Value {
    json!({"body": "Looks right.", "commit_id": "0123456789abcdef0123456789abcdef01234567",
           "comments": [{"path": "src/a.rs", "line": 3, "side": "RIGHT", "body": "nit"}]})
}

fn submit(fx: &Fx, pr: &str, event: &str, file: &str, slack: Option<&str>) -> common::Run {
    let mut args = vec!["review", "submit", "--pr", pr, "--event", event, "--input", file];
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
    let fx = fixture(answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(
        out,
        json!({"ok": true, "review": {"id": 4242, "url": REVIEW_URL, "state": "APPROVED"},
               "reaction": {"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "already": false}})
    );
    assert_eq!(rows(&fx), [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "reacted_at": null})]);
}

#[test]
fn request_changes_posts_the_review_and_queues_changes_requested() {
    let fx = fixture(Reply::ok(&json!({"id": 4242, "html_url": REVIEW_URL, "state": "CHANGES_REQUESTED"}).to_string()));
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, "REQUEST_CHANGES", &file, Some(SLACK)).ok();
    assert_eq!(out["review"], json!({"id": 4242, "url": REVIEW_URL, "state": "CHANGES_REQUESTED"}));
    assert_eq!(out["reaction"], json!({"channel": CHANNEL, "ts": TS, "emoji": "github-changes-requested", "already": false}));
    assert_eq!(rows(&fx), [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-changes-requested", "reacted_at": null})]);
}

#[test]
fn the_input_is_sent_as_is_with_the_event_added_and_nothing_else_is_called() {
    let fx = fixture(answer());
    let file = input(&fx, &review().to_string());
    submit(&fx, REF, "APPROVE", &file, None).ok();
    let calls = gh_calls(&fx);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].args, ["api", "--method", "POST", "repos/acme/api/pulls/7/reviews", "--input", "-"]);
    let mut want = review();
    want["event"] = json!("APPROVE");
    let sent: Value = serde_json::from_slice(calls[0].stdin.as_deref().expect("the review on stdin")).unwrap();
    assert_eq!(sent, want);
}

#[test]
fn a_pr_url_and_owner_repo_hash_n_name_the_same_pull_request() {
    for pr in [REF, URL, &format!("{URL}/")] {
        let fx = fixture(answer());
        let file = input(&fx, &review().to_string());
        submit(&fx, pr, "APPROVE", &file, None).ok();
        let calls = gh_calls(&fx);
        assert_eq!(calls.len(), 1, "{pr}: {calls:?}");
        assert_eq!(calls[0].args, ["api", "--method", "POST", "repos/acme/api/pulls/7/reviews", "--input", "-"], "{pr}");
    }
}

#[test]
fn a_second_submit_posts_a_second_review_and_queues_one_row() {
    let fx = fixture(answer());
    let file = input(&fx, &review().to_string());
    let first = submit(&fx, REF, "APPROVE", &file, Some(SLACK)).ok();
    let second = submit(&fx, REF, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(first["reaction"]["already"], false);
    assert_eq!(second["reaction"]["already"], true);
    assert_eq!(gh_calls(&fx).len(), 2, "a rerun posts a second review");
    assert_eq!(rows(&fx).len(), 1);
}

#[test]
fn without_slack_nothing_is_queued_and_reaction_is_null() {
    let fx = fixture(answer());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, "REQUEST_CHANGES", &file, None).ok();
    assert_eq!(out["reaction"], Value::Null);
    assert_eq!(out["review"]["id"], 4242);
    assert!(rows(&fx).is_empty());
}

#[test]
fn an_input_carrying_an_event_is_refused_before_any_call() {
    let fx = fixture(answer());
    let file = input(&fx, &json!({"body": "x", "event": "COMMENT"}).to_string());
    let error = refused(&submit(&fx, REF, "APPROVE", &file, Some(SLACK)), "usage", 2);
    assert!(message(&error).contains("event"), "{error}");
    assert!(message(&error).contains("--input"), "{error}");
    assert!(gh_calls(&fx).is_empty());
}

#[test]
fn an_input_that_is_not_a_json_object_is_refused_before_any_call() {
    for text in ["[1]", "\"x\"", "not json", ""] {
        let fx = fixture(answer());
        let file = input(&fx, text);
        let error = refused(&submit(&fx, REF, "APPROVE", &file, None), "usage", 2);
        assert!(message(&error).contains("--input"), "{text}: {error}");
        assert!(gh_calls(&fx).is_empty(), "{text}");
    }
    let fx = fixture(answer());
    let missing = fx.env.cpd_home().join("missing.json").display().to_string();
    let error = refused(&submit(&fx, REF, "APPROVE", &missing, None), "usage", 2);
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
        (REF, "APPROVE", Some("c02JU6C2Z1D/1793040197.490209"), "--slack"),
        (REF, "APPROVE", Some("C02JU6C/1793040197.490209"), "--slack"),
        (REF, "APPROVE", Some("C02JU6C2Z1D/179304019.490209"), "--slack"),
        (REF, "APPROVE", Some("C02JU6C2Z1D/1793040197490209"), "--slack"),
        (REF, "APPROVE", Some("C02JU6C2Z1D/1793040197.49020"), "--slack"),
        (REF, "APPROVE", Some("C02JU6C2Z1D/1793040197.490209/x"), "--slack"),
    ];
    for (pr, event, slack, flag) in cases {
        let fx = fixture(answer());
        let file = input(&fx, &review().to_string());
        let error = refused(&submit(&fx, pr, event, &file, slack), "usage", 2);
        assert!(message(&error).contains(flag), "{pr} {event} {slack:?}: {error}");
        assert!(gh_calls(&fx).is_empty(), "{pr} {event} {slack:?}");
    }
}

#[test]
fn a_failed_post_queues_nothing_and_is_gh_failed() {
    let fx = fixture(Reply::exit(1, "", "HTTP 422: Unprocessable Entity\n"));
    let file = input(&fx, &review().to_string());
    let error = refused(&submit(&fx, REF, "APPROVE", &file, Some(SLACK)), "gh_failed", 1);
    assert_eq!(error["details"]["stderr"], "HTTP 422: Unprocessable Entity");
    assert_eq!(gh_calls(&fx).len(), 1);
    assert!(rows(&fx).is_empty());
}

#[test]
fn an_answer_without_a_review_id_is_gh_failed_and_queues_nothing() {
    for body in [json!({"html_url": REVIEW_URL, "state": "APPROVED"}), json!({"id": 4242, "state": "APPROVED"})] {
        let fx = fixture(Reply::ok(&body.to_string()));
        let file = input(&fx, &review().to_string());
        let error = refused(&submit(&fx, REF, "APPROVE", &file, Some(SLACK)), "gh_failed", 1);
        assert!(message(&error).contains("review may have been posted"), "{error}");
        assert!(rows(&fx).is_empty());
    }
}
