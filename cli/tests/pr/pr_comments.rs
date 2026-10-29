//! `pr poll` and the comments read (contract §7.6, §5.2 `pr-feedback`): one `gh api graphql` call for the open pull requests a
//! Plan owns whose `updatedAt` moved, the `pull_requests` row (`updated_at`, `feedback`) and the reason `comments`. All in-process
//! over the mock `gh` of `pr.rs`: no real GitHub.

use serde_json::{Value, json};

use super::pr::{Poll, Rows, feedback, node, pay, poll_fx};

const MARKER: &str = "<!-- cpd:reply -->";

fn page(nodes: Vec<Value>) -> Value {
    json!({"totalCount": nodes.len(), "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": nodes})
}

fn comment(id: &str, login: &str, body: &str, at: &str) -> Value {
    json!({"id": id, "author": {"login": login}, "body": body, "url": format!("https://github.com/acme/pay/pull/812#{id}"), "createdAt": at})
}

fn thread(id: &str, comments: Vec<Value>) -> Value {
    json!({"id": id, "isResolved": false, "isOutdated": false, "path": "src/a.rs", "comments": page(comments)})
}

/// An open thread of reviewer `rev` whose last comment is `last`.
fn open_thread(id: &str, last: &str) -> Value {
    thread(id, vec![comment(last, "rev", "Please change this", "2026-10-20T09:00:00Z")])
}

/// GitHub's answer for pull request 812: its `updatedAt` and its threads.
fn answer(updated_at: &str, threads: Vec<Value>) -> String {
    answer_with_comments(updated_at, threads, vec![])
}

/// Like `answer`, with issue comments on the conversation too.
fn answer_with_comments(updated_at: &str, threads: Vec<Value>, comments: Vec<Value>) -> String {
    let empty = page(vec![]);
    json!({"data": {"viewer": {"login": "me"}, "rateLimit": {"cost": 1, "remaining": 4900, "resetAt": "2026-10-20T11:00:00Z"},
        "p0": {"pullRequest": {"id": "PR_1", "updatedAt": updated_at, "reviewThreads": page(threads), "comments": page(comments), "reviews": empty}}}})
    .to_string()
}

/// The search lists pull request 812 as updated at `updated_at`.
fn search_at(fx: &Poll, updated_at: &str) {
    fx.search(&[pay("OPEN", None, None).replace(r#""updatedAt":"U1""#, &format!(r#""updatedAt":"{updated_at}""#))]);
}

/// Scripts the comments read to `text`.
fn read_says(fx: &Poll, text: String) {
    *fx.gh.comments.lock().unwrap() = Some(text);
}

/// One poll with pull request 812 at `updated_at` and `threads` on it.
fn poll_with(fx: &Poll, updated_at: &str, threads: Vec<Value>) -> Value {
    search_at(fx, updated_at);
    read_says(fx, answer(updated_at, threads));
    fx.poll()
}

/// The argv of every comments read so far.
fn reads(fx: &Poll) -> Vec<Vec<String>> {
    fx.gh_calls().into_iter().filter(|c| c.iter().any(|a| a.contains("reviewThreads"))).collect()
}

/// `(updated_at, feedback)` of the row of pull request `number`; None when the migration has not added the columns.
fn stored(fx: &Poll, number: i64) -> Option<(Option<String>, Option<Value>)> {
    fx.store()
        .read(|c| {
            Ok(c.query_row("SELECT updated_at, feedback FROM pull_requests WHERE number = ?1", [number], |r| {
                let feedback: Option<String> = r.get(1)?;
                Ok((r.get::<_, Option<String>>(0)?, feedback.map(|f| serde_json::from_str::<Value>(&f).unwrap())))
            })?)
        })
        .ok()
}

/// The `id`s of the stored items of pull request 812.
fn stored_ids(fx: &Poll) -> Vec<String> {
    let (_, feedback) = stored(fx, 812).expect("the row has `updated_at` and `feedback`");
    feedback.expect("the list was read").as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn an_owned_open_pr_whose_updated_at_moved_is_read_once_and_stored() {
    let fx = poll_fx("deploy");

    poll_with(&fx, "U1", vec![open_thread("T1", "C1")]);

    let calls = reads(&fx);
    assert_eq!(calls.len(), 1, "one extra call: {:?}", fx.gh_calls());
    assert_eq!(calls[0][..3], ["api", "graphql", "-f"]);
    assert!(calls[0][3].contains(r#"name: "pay""#) && calls[0][3].contains("812"), "{}", calls[0][3]);
    let (updated_at, feedback) = stored(&fx, 812).expect("the row has `updated_at` and `feedback`");
    assert_eq!(updated_at.as_deref(), Some("U1"));
    let items = feedback.expect("the list was read");
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(
        items[0],
        json!({"id": "T1", "kind": "thread", "last": "C1", "author": "rev", "bot": false, "path": "src/a.rs", "outdated": false,
               "url": "https://github.com/acme/pay/pull/812#C1", "excerpt": "Please change this"})
    );
}

#[test]
fn a_poll_whose_updated_at_did_not_move_makes_no_comments_call_and_writes_nothing() {
    let fx = poll_fx("deploy");
    poll_with(&fx, "U1", vec![open_thread("T1", "C1")]);
    assert_eq!(reads(&fx).len(), 1, "the first poll reads");
    let (txns, events) = (fx.txns(), fx.event_count());

    // The same `updatedAt`, a different answer on GitHub's side: not asked.
    let out = poll_with(&fx, "U1", vec![open_thread("T1", "C1"), open_thread("T2", "C9")]);

    assert_eq!(reads(&fx).len(), 1, "no second call");
    assert_eq!(out["feedback"], json!([]));
    assert_eq!((fx.txns(), fx.event_count()), (txns, events), "nothing written");
    assert_eq!(stored_ids(&fx), ["T1"]);
}

#[test]
fn a_pr_no_plan_owns_is_never_read() {
    let fx = poll_fx("deploy");
    fx.search(&[pay("OPEN", None, None), node("acme/web", 902, "OPEN", None, None)]);

    fx.poll();

    let calls = reads(&fx);
    assert_eq!(calls.len(), 1, "the owned pull request is read: {:?}", fx.gh_calls());
    assert!(!calls[0][3].contains("web") && !calls[0][3].contains("902"), "{}", calls[0][3]);
    assert_eq!(stored(&fx, 902).expect("the columns exist"), (None, None), "the unowned row is never read");
}

#[test]
fn the_first_read_tells_every_open_item_with_reason_comments() {
    let fx = poll_fx("deploy");
    assert_eq!(stored(&fx, 812), Some((None, None)), "a row never read: both NULL");

    let out = poll_with(&fx, "U1", vec![open_thread("T1", "C1"), open_thread("T2", "C2")]);

    assert_eq!(out["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, feedback("open", "none", "none", &["comments"]));
    assert!(events[0].0.contains("cpd pr view"), "{}", events[0].0);
}

#[test]
fn a_new_open_item_sends_pr_feedback_with_reason_comments_and_the_counts_in_its_text() {
    let fx = poll_fx("deploy");
    poll_with(&fx, "U1", vec![open_thread("T1", "C1")]);
    assert_eq!(fx.events("pr-feedback").len(), 1);

    let out = poll_with(&fx, "U2", vec![open_thread("T1", "C1"), open_thread("T2", "C2")]);

    assert_eq!(out["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 2, "told again of the one new item");
    assert_eq!(events[1].1, feedback("open", "none", "none", &["comments"]));
    let text = &events[1].0;
    assert!(text.contains("1 new") && text.contains("2 open") && text.contains("cpd pr view"), "{text}");
    assert_eq!(stored_ids(&fx), ["T1", "T2"]);
}

#[test]
fn an_item_that_closes_is_dropped_from_the_list_and_tells_nothing() {
    let fx = poll_fx("deploy");
    poll_with(&fx, "U1", vec![open_thread("T1", "C1"), open_thread("T2", "C2")]);
    assert_eq!(fx.events("pr-feedback").len(), 1);

    // T1 was resolved on GitHub.
    let out = poll_with(&fx, "U2", vec![open_thread("T2", "C2")]);

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(fx.events("pr-feedback").len(), 1, "closing tells nothing");
    assert_eq!(stored_ids(&fx), ["T2"]);
    assert_eq!(stored(&fx, 812).unwrap().0.as_deref(), Some("U2"));
}

#[test]
fn a_reviewer_reply_in_an_answered_thread_tells_again() {
    let fx = poll_fx("deploy");
    poll_with(&fx, "U1", vec![open_thread("T1", "C1")]);
    assert_eq!(fx.events("pr-feedback").len(), 1);

    // The Lead answered: the last comment carries the marker, the thread is no item.
    let answered = thread("T1", vec![comment("C1", "rev", "Change this", "t1"), comment("C2", "me", &format!("Done.\n\n{MARKER}"), "t2")]);
    let out = poll_with(&fx, "U2", vec![answered.clone()]);
    assert_eq!(out["feedback"], json!([]));
    assert_eq!(stored(&fx, 812).unwrap().1, Some(json!([])), "read, none open");
    assert_eq!(fx.events("pr-feedback").len(), 1);

    // The reviewer replies after the marker.
    let mut again = answered;
    again["comments"] = page(vec![
        comment("C1", "rev", "Change this", "t1"),
        comment("C2", "me", &format!("Done.\n\n{MARKER}"), "t2"),
        comment("C3", "rev", "Not like that", "t3"),
    ]);
    let out = poll_with(&fx, "U3", vec![again]);

    assert_eq!(out["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(fx.events("pr-feedback").len(), 2);
    assert_eq!(stored(&fx, 812).unwrap().1.unwrap()[0]["last"], "C3");
}

#[test]
fn a_failed_comments_read_fails_the_poll_loud_and_writes_nothing() {
    let fx = poll_fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    // Over 100 threads: GitHub's answer cannot be read whole.
    let mut over = answer("U9", vec![open_thread("T1", "C1")]);
    let mut value: Value = serde_json::from_str(&over).unwrap();
    value["data"]["p0"]["pullRequest"]["reviewThreads"] = json!({"totalCount": 101, "pageInfo": {"hasPreviousPage": true, "hasNextPage": true}, "nodes": [open_thread("T1", "C1")]});
    over = value.to_string();
    read_says(&fx, over);
    // The search also changed the review decision: stored with the rest of the poll, or not at all.
    fx.search(&[pay("OPEN", Some("CHANGES_REQUESTED"), None).replace(r#""updatedAt":"U1""#, r#""updatedAt":"U9""#)]);

    let error = fx.pr("poll").error();

    assert_eq!(error["code"], "gh_failed");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("acme/pay") && message.contains("812"), "{message}");
    assert_eq!((fx.txns(), fx.event_count()), (txns, events), "nothing written");
    assert_eq!(fx.pr_data().1, "none", "the search's review decision was not stored");
    assert_eq!(stored(&fx, 812), Some((None, None)));
}

#[test]
fn a_bot_typed_issue_comment_is_stored_as_no_item_and_tells_nothing() {
    let typed = |typename: &str, login: &str| {
        let mut c = comment("I1", login, "Scan summary", "2026-10-20T09:00:00Z");
        c["author"]["__typename"] = json!(typename);
        c
    };

    let fx = poll_fx("deploy");
    search_at(&fx, "U1");
    read_says(&fx, answer_with_comments("U1", vec![], vec![typed("Bot", "kodem-security")]));
    fx.poll();
    assert_eq!(stored_ids(&fx), Vec::<String>::new(), "a Bot's comment is no item");
    assert_eq!(fx.events("pr-feedback").len(), 0);

    let fx = poll_fx("deploy");
    search_at(&fx, "U1");
    read_says(&fx, answer_with_comments("U1", vec![], vec![typed("User", "rev")]));
    fx.poll();
    assert_eq!(stored_ids(&fx), ["I1"]);
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, feedback("open", "none", "none", &["comments"]));
}
