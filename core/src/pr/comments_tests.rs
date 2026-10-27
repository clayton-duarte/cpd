//! `comments::parse`, `comments::new_items` and the pinned query: the feedback items from GitHub's answer.

use serde_json::{Value, json};

use super::comments::{Item, Kind, MARKER, Read, new_items, parse, query};
use crate::error::ErrorCode;

const PRS: [(&str, i64); 1] = [("acme/pay", 7)];

fn comment(id: &str, login: &str, body: &str, at: &str) -> Value {
    json!({"id": id, "author": {"login": login}, "body": body, "url": format!("https://github.com/acme/pay/pull/7#{id}"), "createdAt": at})
}

fn connection(nodes: Vec<Value>) -> Value {
    json!({"totalCount": nodes.len(), "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": nodes})
}

/// A connection holding 101: the one GitHub does not let the poll read whole.
fn over(nodes: Vec<Value>) -> Value {
    json!({"totalCount": 101, "pageInfo": {"hasPreviousPage": true, "hasNextPage": true}, "nodes": nodes})
}

fn thread(id: &str, resolved: bool, outdated: bool, comments: Vec<Value>) -> Value {
    json!({"id": id, "isResolved": resolved, "isOutdated": outdated, "path": "src/a.rs", "comments": connection(comments)})
}

fn review(id: &str, state: &str, login: &str, body: &str, at: &str) -> Value {
    json!({"id": id, "state": state, "body": body, "author": {"login": login}, "url": format!("https://github.com/acme/pay/pull/7#{id}"), "createdAt": at})
}

fn pull_request(threads: Value, comments: Value, reviews: Value) -> Value {
    json!({"id": "PR_1", "updatedAt": "2026-10-20T10:00:00Z", "reviewThreads": threads, "comments": comments, "reviews": reviews})
}

fn answer(pull_requests: Vec<Value>) -> String {
    let mut data = json!({"viewer": {"login": "me"}, "rateLimit": {"cost": 1, "remaining": 4900, "resetAt": "2026-10-20T11:00:00Z"}});
    for (i, p) in pull_requests.into_iter().enumerate() {
        data[format!("p{i}")] = json!({"pullRequest": p});
    }
    json!({"data": data}).to_string()
}

fn read(threads: Vec<Value>, comments: Vec<Value>, reviews: Vec<Value>) -> Read {
    let p = pull_request(connection(threads), connection(comments), connection(reviews));
    let mut reads = parse(&PRS, &answer(vec![p]), &[]).unwrap();
    assert_eq!(reads.len(), 1);
    reads.remove(0)
}

fn threads_only(threads: Vec<Value>) -> Vec<Item> {
    read(threads, vec![], vec![]).items
}

fn marked(text: &str) -> String {
    format!("{text}\n\n{MARKER}")
}

fn find<'a>(items: &'a [Item], id: &str) -> &'a Item {
    items.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("no item {id} in {items:?}"))
}

fn ids(items: &[Item]) -> Vec<&str> {
    let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn a_thread_is_open_until_it_is_resolved() {
    let comments = vec![
        comment("C1", "rev", "Please   rename\nthis", "2026-10-20T09:00:00Z"),
        comment("C2", "rev", "  Or  keep it,\n\n  I do not mind. ", "2026-10-20T09:05:00Z"),
    ];
    let read = read(vec![thread("T1", false, false, comments.clone())], vec![], vec![]);

    assert_eq!((read.repo.as_str(), read.number, read.updated_at.as_str()), ("acme/pay", 7, "2026-10-20T10:00:00Z"));
    assert_eq!(
        read.items,
        [Item {
            id: "T1".into(),
            kind: Kind::Thread,
            last: "C2".into(),
            author: "rev".into(),
            bot: false,
            path: Some("src/a.rs".into()),
            outdated: false,
            url: "https://github.com/acme/pay/pull/7#C2".into(),
            excerpt: "Or keep it, I do not mind.".into(),
        }]
    );
    assert_eq!(serde_json::to_value(&read.items[0]).unwrap()["kind"], "thread");

    assert_eq!(threads_only(vec![thread("T1", true, false, comments)]), []);
}

#[test]
fn a_thread_whose_last_comment_carries_the_marker_is_addressed() {
    let comments = vec![comment("C1", "rev", "Rename this", "t1"), comment("C2", "me", &marked("Done, renamed."), "t2")];

    assert_eq!(threads_only(vec![thread("T1", false, false, comments)]), []);
}

#[test]
fn a_reviewer_replying_after_the_marker_opens_the_thread_again() {
    let comments = vec![
        comment("C1", "rev", "Rename this", "t1"),
        comment("C2", "me", &marked("Done, renamed."), "t2"),
        comment("C3", "rev", "Not that name", "t3"),
    ];

    let items = threads_only(vec![thread("T1", false, false, comments)]);

    assert_eq!(ids(&items), ["T1"]);
    assert_eq!((items[0].last.as_str(), items[0].author.as_str(), items[0].excerpt.as_str()), ("C3", "rev", "Not that name"));
}

#[test]
fn an_outdated_thread_is_open_and_flagged() {
    let comments = vec![comment("C1", "rev", "Rename this", "t1")];

    let items = threads_only(vec![thread("T1", false, true, comments.clone())]);
    assert_eq!(ids(&items), ["T1"]);
    assert!(items[0].outdated);

    assert_eq!(threads_only(vec![thread("T1", true, true, comments)]), [], "resolved wins");
}

#[test]
fn a_thread_started_by_a_bot_is_a_bot_item_even_when_a_human_spoke_last() {
    let comments = |first: &str| vec![comment("C1", first, "Possible bug", "t1"), comment("C2", "rev", "Agreed, fix it", "t2")];

    let items = threads_only(vec![thread("T1", false, false, comments("copilot[bot]"))]);
    assert_eq!((items[0].author.as_str(), items[0].bot, items[0].last.as_str()), ("copilot[bot]", true, "C2"));

    // A login listed in `inbox.bot_reviewers` is a bot too.
    let p = pull_request(connection(vec![thread("T1", false, false, comments("sonar"))]), connection(vec![]), connection(vec![]));
    let items = parse(&PRS, &answer(vec![p]), &["sonar".to_owned()]).unwrap().remove(0).items;
    assert_eq!((items[0].author.as_str(), items[0].bot), ("sonar", true));

    // A human's thread is no bot item.
    let items = threads_only(vec![thread("T1", false, false, comments("rev"))]);
    assert!(!items[0].bot);
}

#[test]
fn an_issue_comment_is_open_until_a_later_marked_comment() {
    let before = vec![comment("I1", "rev", "Why this approach?", "2026-10-20T09:00:00Z")];
    let items = read(vec![], before.clone(), vec![]).items;
    assert_eq!(
        items,
        [Item {
            id: "I1".into(),
            kind: Kind::Comment,
            last: "I1".into(),
            author: "rev".into(),
            bot: false,
            path: None,
            outdated: false,
            url: "https://github.com/acme/pay/pull/7#I1".into(),
            excerpt: "Why this approach?".into(),
        }]
    );
    assert_eq!(serde_json::to_value(&items[0]).unwrap()["kind"], "comment");

    // A later marked comment answers every comment before it, and only those.
    let mut comments = before;
    comments.push(comment("I2", "me", &marked("Because of X."), "2026-10-20T09:30:00Z"));
    assert_eq!(read(vec![], comments.clone(), vec![]).items, []);
    comments.push(comment("I3", "rev", "And Y?", "2026-10-20T09:45:00Z"));
    assert_eq!(ids(&read(vec![], comments, vec![]).items), ["I3"]);

    // A marked comment made earlier answers nothing.
    let comments = vec![comment("I0", "me", &marked("Hello."), "2026-10-20T08:00:00Z"), comment("I1", "rev", "Why?", "2026-10-20T09:00:00Z")];
    assert_eq!(ids(&read(vec![], comments, vec![]).items), ["I1"]);

    // Ties by `createdAt` go by the answer's order: the marked comment listed after is later.
    let comments = vec![comment("I1", "rev", "Why?", "t"), comment("I2", "me", &marked("Because."), "t")];
    assert_eq!(read(vec![], comments, vec![]).items, []);
}

#[test]
fn the_users_own_and_bots_top_level_comments_are_never_items() {
    let comments = vec![
        comment("I1", "me", "A note to myself", "t1"),
        comment("I2", "dependabot[bot]", "Bumped", "t2"),
        comment("I3", "sonar", "Quality gate failed", "t3"),
        comment("I4", "rev", "A human says something", "t4"),
    ];
    let reviews = vec![review("R1", "COMMENTED", "me", "My own review", "t5"), review("R2", "COMMENTED", "copilot[bot]", "Overview", "t6"), review("R3", "CHANGES_REQUESTED", "sonar", "No", "t7")];
    let p = pull_request(connection(vec![]), connection(comments), connection(reviews));

    let items = parse(&PRS, &answer(vec![p]), &["sonar".to_owned()]).unwrap().remove(0).items;

    assert_eq!(ids(&items), ["I4"]);
}

#[test]
fn a_commented_or_changes_requested_review_with_a_body_is_an_item_and_an_empty_or_approved_one_is_not() {
    let reviews = vec![
        review("R1", "COMMENTED", "rev", "Some thoughts", "t1"),
        review("R2", "CHANGES_REQUESTED", "rev", "  Needs work\n here ", "t2"),
        review("R3", "APPROVED", "rev", "LGTM", "t3"),
        review("R4", "COMMENTED", "rev", "", "t4"),
        review("R5", "COMMENTED", "rev", "  \n ", "t5"),
        review("R6", "DISMISSED", "rev", "Stale", "t6"),
        review("R7", "PENDING", "rev", "Draft", "t7"),
    ];

    let items = read(vec![], vec![], reviews).items;

    assert_eq!(ids(&items), ["R1", "R2"]);
    assert_eq!(
        *find(&items, "R2"),
        Item {
            id: "R2".into(),
            kind: Kind::Review,
            last: "R2".into(),
            author: "rev".into(),
            bot: false,
            path: None,
            outdated: false,
            url: "https://github.com/acme/pay/pull/7#R2".into(),
            excerpt: "Needs work here".into(),
        }
    );
    assert_eq!(serde_json::to_value(find(&items, "R1")).unwrap()["kind"], "review");

    // A later marked issue comment answers the review; an earlier one does not.
    let timed = vec![
        review("R1", "COMMENTED", "rev", "First", "2026-10-20T09:01:00Z"),
        review("R2", "CHANGES_REQUESTED", "rev", "Second", "2026-10-20T09:03:00Z"),
    ];
    let answered = vec![comment("I1", "me", &marked("Done."), "2026-10-20T09:02:00Z")];
    assert_eq!(ids(&read(vec![], answered, timed).items), ["R2"]);
}

#[test]
fn more_than_100_threads_comments_or_reviews_is_gh_failed() {
    let one = || comment("C1", "rev", "x", "t1");
    let t = || thread("T1", false, false, vec![one()]);
    let cases = [
        pull_request(over(vec![t()]), connection(vec![]), connection(vec![])),
        pull_request(connection(vec![t()]), over(vec![one()]), connection(vec![])),
        pull_request(connection(vec![t()]), connection(vec![]), over(vec![review("R1", "COMMENTED", "rev", "x", "t1")])),
        pull_request(
            connection(vec![json!({"id": "T1", "isResolved": false, "isOutdated": false, "path": "a", "comments": over(vec![one()])})]),
            connection(vec![]),
            connection(vec![]),
        ),
    ];

    for (i, case) in cases.into_iter().enumerate() {
        let err = parse(&PRS, &answer(vec![case]), &[]).expect_err(&format!("case {i}"));
        assert_eq!(err.code(), ErrorCode::GhFailed, "case {i}");
        assert!(err.message().contains("acme/pay") && err.message().contains('7'), "case {i}: {}", err.message());
    }
}

#[test]
fn a_missing_pull_request_or_an_errors_array_is_gh_failed() {
    let good = || pull_request(connection(vec![]), connection(vec![]), connection(vec![]));

    // The pull request is null (NOT_FOUND).
    let mut missing: Value = serde_json::from_str(&answer(vec![good()])).unwrap();
    missing["data"]["p0"]["pullRequest"] = Value::Null;
    missing["errors"] = json!([{"type": "NOT_FOUND", "path": ["p0", "pullRequest"], "message": "Could not resolve to a PullRequest"}]);
    assert_eq!(parse(&PRS, &missing.to_string(), &[]).unwrap_err().code(), ErrorCode::GhFailed);

    // Null with no `errors` is as missing.
    let mut null: Value = serde_json::from_str(&answer(vec![good()])).unwrap();
    null["data"]["p0"]["pullRequest"] = Value::Null;
    assert_eq!(parse(&PRS, &null.to_string(), &[]).unwrap_err().code(), ErrorCode::GhFailed);

    // An alias is absent: two pairs, one answer.
    let two = [("acme/pay", 7), ("acme/web", 9)];
    assert_eq!(parse(&two, &answer(vec![good()]), &[]).unwrap_err().code(), ErrorCode::GhFailed);

    // Good data with an `errors` array is no answer.
    let mut errors: Value = serde_json::from_str(&answer(vec![good()])).unwrap();
    errors["errors"] = json!([{"message": "Something went wrong"}]);
    assert_eq!(parse(&PRS, &errors.to_string(), &[]).unwrap_err().code(), ErrorCode::GhFailed);

    // Not JSON.
    assert_eq!(parse(&PRS, "not json", &[]).unwrap_err().code(), ErrorCode::GhFailed);

    // The good answer parses, so that the failures above are the answers' and not the helpers'.
    assert_eq!(parse(&PRS, &answer(vec![good()]), &[]).unwrap().len(), 1);
}

fn item(id: &str, last: &str) -> Item {
    Item {
        id: id.into(),
        kind: Kind::Thread,
        last: last.into(),
        author: "rev".into(),
        bot: false,
        path: None,
        outdated: false,
        url: "u".into(),
        excerpt: "e".into(),
    }
}

#[test]
fn new_items_are_those_whose_last_comment_is_unknown() {
    let before = [item("T1", "C1"), item("T2", "C2")];
    let after = [item("T1", "C1"), item("T2", "C5"), item("T3", "C6")];

    let new: Vec<&str> = new_items(Some(&before), &after).iter().map(|i| i.id.as_str()).collect();
    assert_eq!(new, ["T2", "T3"], "a thread whose last comment changed is new again");

    assert!(new_items(Some(&after), &after).is_empty());
    assert!(new_items(Some(&after), &before[..1]).is_empty(), "an item that closed tells nothing");
    let first: Vec<&str> = new_items(None, &after).iter().map(|i| i.id.as_str()).collect();
    assert_eq!(first, ["T1", "T2", "T3"], "None counts as empty");
    assert!(new_items(None, &[]).is_empty());
    assert!(new_items(Some(&[]), &[]).is_empty());
}

#[test]
fn the_query_selects_the_fields_the_contract_reads() {
    let text = query(&[("acme/pay", 7), ("acme/web", 9)]);

    for needle in [
        "p0: repository(owner: \"acme\", name: \"pay\")",
        "pullRequest(number: 7)",
        "p1: repository(owner: \"acme\", name: \"web\")",
        "pullRequest(number: 9)",
        "reviewThreads(first: 100)",
        "isResolved",
        "isOutdated",
        "viewer { login }",
        "rateLimit { cost remaining resetAt }",
        "updatedAt",
        "comments(last: 100)",
        "reviews(last: 100)",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in {text}");
    }
}
