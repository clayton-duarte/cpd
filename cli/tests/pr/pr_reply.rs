//! `pr reply` (Plan fb-path, DESIGN item 5): answers an item of the stored feedback list with a marked reply, never resolves.

use std::fs;

use cpd_core::host::runner::{Call, Reply};
use serde_json::{Value, json};

use super::pr_view::{Pay, fx};

const MARKER: &str = "<!-- cpd:reply -->";
const THREAD_URL: &str = "https://github.com/acme/repo/pull/7#discussion_r1";
const COMMENT_URL: &str = "https://github.com/acme/pay/pull/812#issuecomment-5";
const THREAD_ANSWER: &str = r#"{"data": {"addPullRequestReviewThreadReply": {"comment": {"id": "c1", "url": "https://github.com/acme/repo/pull/7#discussion_r1"}}}}"#;
const COMMENT_ANSWER: &str = r#"{"data": {"addComment": {"commentEdge": {"node": {"id": "c2", "url": "https://github.com/acme/repo/pull/7#issuecomment-2"}}}}}"#;
const PR_ID_ANSWER: &str = r#"{"data": {"repository": {"pullRequest": {"id": "PR_node"}}}}"#;

fn item(id: &str, kind: &str, path: Value, url: &str) -> Value {
    json!({"id": id, "kind": kind, "last": id, "author": "reviewer", "bot": false, "path": path, "outdated": false,
           "url": url, "excerpt": "This retry never backs off."})
}

fn list() -> Value {
    json!([
        item(
            "PRRT_1",
            "thread",
            json!("src/payout.rs"),
            "https://github.com/acme/pay/pull/812#discussion_r9"
        ),
        item("IC_1", "comment", Value::Null, COMMENT_URL),
        item(
            "PRR_1",
            "review",
            Value::Null,
            "https://github.com/acme/pay/pull/812#pullrequestreview-3"
        )
    ])
}

/// Plan `pay` with `status` PR, the list stored, and the mock `gh` as runner rules (`fx` routes `gh` to `pr_view`'s
/// handler, which the rules precede); every call fails when `fail`.
fn setup(status: &str, fail: bool) -> Pay {
    let pay = fx(status);
    let has = |text: &'static str| move |c: &Call| c.args.join("\n").contains(text);
    let runner = (*pay.fx.fakes.runner).clone();
    let _ = if fail {
        runner.on(
            "gh",
            |_| true,
            Reply::exit(1, "", "HTTP 502: bad gateway\n"),
        )
    } else {
        runner
            .on(
                "gh",
                has("addPullRequestReviewThreadReply"),
                Reply::ok(THREAD_ANSWER),
            )
            .on("gh", has("addComment"), Reply::ok(COMMENT_ANSWER))
            .on("gh", has("pullRequest"), Reply::ok(PR_ID_ANSWER))
    };
    let sql = format!(
        "UPDATE pull_requests SET feedback = '{}' WHERE number = 812;",
        list()
    );
    pay.store()
        .write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into))
        .unwrap();
    pay
}

fn mutations(pay: &Pay) -> Vec<String> {
    pay.fx
        .calls("gh")
        .into_iter()
        .map(|c| c.args.join("\n"))
        .filter(|a| a.contains("addPullRequestReviewThreadReply") || a.contains("addComment"))
        .collect()
}

fn count(pay: &Pay, table: &str) -> i64 {
    pay.store()
        .read(|c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?))
        .unwrap()
}

fn feedback(pay: &Pay) -> Option<String> {
    pay.store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT feedback FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn a_reply_to_a_thread_posts_the_thread_reply_mutation_with_the_marker_and_prints_its_url() {
    let pay = setup("open", false);

    let out = pay
        .fx
        .cpd(&[
            "pr",
            "reply",
            "--pr",
            "repo",
            "--plan",
            "pay",
            "--to",
            "PRRT_1",
            "--body",
            "Fixed in abc.",
        ])
        .ok();

    assert_eq!(
        out,
        json!({"ok": true, "replied": {"pr": "repo", "to": "PRRT_1", "url": THREAD_URL}})
    );
    let posted = mutations(&pay);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(
        posted[0].contains("addPullRequestReviewThreadReply"),
        "{}",
        posted[0]
    );
    assert!(posted[0].contains("PRRT_1"), "{}", posted[0]);
    assert!(
        posted[0].contains(&format!("Fixed in abc.\n\n{MARKER}")),
        "{}",
        posted[0]
    );
}

#[test]
fn a_reply_to_a_comment_or_review_posts_an_issue_comment_quoting_its_url_and_ending_with_the_marker()
 {
    for (to, url) in [
        ("IC_1", COMMENT_URL),
        (
            "PRR_1",
            "https://github.com/acme/pay/pull/812#pullrequestreview-3",
        ),
    ] {
        let pay = setup("open", false);

        let out = pay
            .fx
            .cpd(&[
                "pr", "reply", "--pr", "repo", "--plan", "pay", "--to", to, "--body", "Done.",
            ])
            .ok();

        assert_eq!(
            out,
            json!({"ok": true, "replied": {"pr": "repo", "to": to, "url": "https://github.com/acme/repo/pull/7#issuecomment-2"}})
        );
        let posted = mutations(&pay);
        assert_eq!(posted.len(), 1, "{posted:?}");
        assert!(posted[0].contains("addComment"), "{}", posted[0]);
        assert!(
            !posted[0].contains("addPullRequestReviewThreadReply"),
            "{}",
            posted[0]
        );
        assert!(posted[0].contains("PR_node"), "{}", posted[0]);
        assert!(
            posted[0].contains(&format!("Re: {url}\n\nDone.\n\n{MARKER}")),
            "{}",
            posted[0]
        );
        // The pull request's node id is read first.
        assert!(
            pay.fx
                .calls("gh")
                .iter()
                .any(|c| c.args.join("\n").contains("pullRequest"))
        );
    }
}

#[test]
fn the_body_can_come_from_a_file() {
    let pay = setup("open", false);
    let file = pay.fx.env.root().join("body.md");
    fs::write(&file, "From a file.\n").unwrap();

    let out = pay
        .fx
        .cpd(&[
            "pr",
            "reply",
            "--pr",
            "repo",
            "--plan",
            "pay",
            "--to",
            "PRRT_1",
            "--body-file",
            file.to_str().unwrap(),
        ])
        .ok();

    assert_eq!(out["ok"], true);
    let posted = mutations(&pay);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(
        posted[0].contains(&format!("From a file.\n\n{MARKER}")),
        "{}",
        posted[0]
    );
}

#[test]
fn an_unknown_item_is_refused_feedback_not_found_and_posts_nothing() {
    let pay = setup("open", false);

    let run = pay.fx.cpd(&[
        "pr", "reply", "--pr", "repo", "--plan", "pay", "--to", "NOPE", "--body", "Hi.",
    ]);

    assert_eq!(
        (run.error()["code"].as_str().unwrap(), run.exit),
        ("feedback_not_found", 3)
    );
    assert_eq!(
        run.error()["details"],
        json!({"pr": "repo", "to": "NOPE", "open": ["PRRT_1", "IC_1", "PRR_1"]})
    );
    assert!(pay.fx.calls("gh").is_empty());
}

#[test]
fn a_pr_that_is_not_open_is_refused_pr_not_open_and_posts_nothing() {
    let pay = setup("waiting", false);

    let run = pay.fx.cpd(&[
        "pr", "reply", "--pr", "repo", "--plan", "pay", "--to", "PRRT_1", "--body", "Hi.",
    ]);

    assert_eq!(
        (run.error()["code"].as_str().unwrap(), run.exit),
        ("pr_not_open", 3)
    );
    assert!(pay.fx.calls("gh").is_empty());
}

#[test]
fn an_empty_body_or_both_or_neither_body_option_is_a_usage_error() {
    let pay = setup("open", false);
    let file = pay.fx.env.root().join("body.md");
    fs::write(&file, "x").unwrap();
    let file = file.to_str().unwrap();
    let base = [
        "pr", "reply", "--pr", "repo", "--plan", "pay", "--to", "PRRT_1",
    ];
    let cases: Vec<Vec<&str>> = vec![
        vec!["--body", ""],
        vec!["--body", "  \n "],
        vec!["--body", "a", "--body-file", file],
        vec![],
    ];

    for extra in cases {
        let args: Vec<&str> = base.iter().chain(extra.iter()).copied().collect();
        let run = pay.fx.cpd(&args);
        assert_eq!(
            (run.error()["code"].as_str().unwrap(), run.exit),
            ("usage", 2),
            "{args:?}"
        );
    }
    assert!(pay.fx.calls("gh").is_empty());
}

#[test]
fn a_gh_failure_is_gh_failed_and_writes_nothing_in_the_store() {
    let pay = setup("open", true);
    let before = (count(&pay, "txns"), count(&pay, "events"), feedback(&pay));

    let run = pay.fx.cpd(&[
        "pr", "reply", "--pr", "repo", "--plan", "pay", "--to", "PRRT_1", "--body", "Hi.",
    ]);

    assert_eq!(
        (run.error()["code"].as_str().unwrap(), run.exit),
        ("gh_failed", 1)
    );
    assert_eq!(
        (count(&pay, "txns"), count(&pay, "events"), feedback(&pay)),
        before
    );
}

#[test]
fn it_never_resolves_a_thread_and_writes_no_event() {
    let pay = setup("open", false);
    let before = (count(&pay, "txns"), count(&pay, "events"), feedback(&pay));

    pay.fx
        .cpd(&[
            "pr", "reply", "--pr", "repo", "--plan", "pay", "--to", "PRRT_1", "--body", "Done.",
        ])
        .ok();

    let calls: Vec<String> = pay
        .fx
        .calls("gh")
        .into_iter()
        .map(|c| c.args.join("\n"))
        .collect();
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|c| !c.contains("resolveReviewThread")),
        "{calls:?}"
    );
    assert_eq!(
        (count(&pay, "txns"), count(&pay, "events"), feedback(&pay)),
        before
    );
}
