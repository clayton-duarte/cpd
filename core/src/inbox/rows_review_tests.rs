//! Red tests of the review half of the two-row label and of the GraphQL `title` (pro-d171, assumption A3): the
//! `review-requested` label carries the GitHub title on Row 2, `plan-review` its review title.

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Kind};
use super::rules::{Team, conditions_with};
use super::team::{Lookup, github_lookup_with, parse_lookup};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn sql(store: &mut Store, statement: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(statement)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// A Context `ctx` with Manager `mgr` and Plan `pay` (phase `plan`) led by `l-pay`.
fn fixture() -> (tempfile::TempDir, CpdHome, Store) {
    let (dir, home, mut store) = store();
    sql(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Pay', 'plan', 'l-pay', '{T}', NULL, NULL);"
        ),
    );
    (dir, home, store)
}

fn of(store: &mut Store, home: &CpdHome, kind: Kind) -> Vec<Condition> {
    conditions_with(store, home, NOW, 20, 1, None)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == kind)
        .collect()
}

fn refs() -> Vec<(String, u64)> {
    vec![("acme/pay".to_owned(), 812)]
}

/// A GraphQL reply for `acme/pay#812`: open, no review; with `title` when given.
fn reply(title: Option<&str>) -> Value {
    let mut workflow = json!({"isDraft": false, "state": "OPEN", "reviewDecision": null, "author": {"login": "ryan"},
                        "latestReviews": {"nodes": []}});
    if let Some(title) = title {
        workflow["title"] = json!(title);
    }
    json!({"data": {"viewer": {"login": "clayton-duarte"}, "p0": {"pullRequest": workflow}}})
}

#[test]
fn a_review_requested_label_has_the_pull_request_title_on_row_two() {
    let (_dir, home, mut store) = store();
    let ts = NOW.unix_timestamp() as f64 - 3600.0;
    let post = json!({"repo": "acme/pay", "number": 812, "ts": ts, "ts_raw": "1.1", "author": "Ryan Case", "primary": true,
                      "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    let path = home.run_dir().join("team-review.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let cache = json!({"version": 1, "polled_at": "2026-01-01T00:00:00.000Z", "channel": "C02JU6C2Z1D", "read": 10, "posts": [post]});
    std::fs::write(path, cache.to_string()).unwrap();

    let lookup: Lookup = parse_lookup(&reply(Some("Add retries")), &refs()).unwrap();
    let bots = ["github-actions".to_owned()];
    let mut github = |_: &[(String, u64)]| -> Result<Lookup, CpdError> { Ok(lookup.clone()) };
    let team = Team {
        bot_reviewers: &bots,
        github: &mut github,
    };
    let all = conditions_with(&mut store, &home, NOW, 20, 1, Some(team)).unwrap();
    let reviews: Vec<Condition> = all
        .into_iter()
        .filter(|c| c.kind == Kind::ReviewRequested)
        .collect();
    assert_eq!(reviews.len(), 1, "{reviews:?}");
    assert_eq!(reviews[0].label, "review pay#812\nAdd retries");
}

#[test]
fn a_plan_review_label_has_the_review_title_on_row_two_and_none_without_a_title() {
    let insert = "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)";
    let (_dir, home, mut store) = fixture();
    sql(
        &mut store,
        &format!("{insert} VALUES ('pay', 'r1', 1, '{FP}', 'pending', '{T}', NULL);"),
    );
    sql(
        &mut store,
        "UPDATE plan_reviews SET title = 'Add retries' WHERE id = 'r1'",
    );
    let titled = of(&mut store, &home, Kind::PlanReview);
    assert_eq!(
        titled.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["pay plan review\nAdd retries"]
    );

    let (_dir, home, mut store) = fixture();
    sql(
        &mut store,
        &format!("{insert} VALUES ('pay', 'r1', 1, '{FP}', 'pending', '{T}', NULL);"),
    );
    let untitled = of(&mut store, &home, Kind::PlanReview);
    assert_eq!(
        untitled
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["pay plan review"]
    );
}

fn fake(reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on("gh", |_| true, reply)
}

fn home() -> CpdHome {
    CpdHome::resolve(Some("/cpd-home".into()), None).unwrap()
}

#[test]
fn github_lookup_with_asks_for_the_title_of_each_pull_request() {
    let runner = fake(Reply::ok(&reply(Some("Add retries")).to_string()));
    github_lookup_with(&runner, &home(), &refs()).unwrap();
    let query = runner.calls()[0].args[3].clone();
    let start = query
        .find("pullRequest(number: 812) {")
        .unwrap_or_else(|| panic!("no pullRequest selection: {query}"));
    let selection = &query[start..];
    let selection = &selection[..selection.find("latestReviews").unwrap_or(selection.len())];
    assert!(
        selection.contains("title"),
        "title is not selected inside pullRequest(..) {{ .. }}: {query}"
    );
}

#[test]
fn github_lookup_with_refuses_a_pull_request_reply_without_a_title() {
    let runner = fake(Reply::ok(&reply(None).to_string()));
    let err = github_lookup_with(&runner, &home(), &refs()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GhFailed);
    assert!(err.to_string().contains("title"), "{err}");
}

#[test]
fn parse_lookup_refuses_a_pull_request_without_a_title_and_names_the_field() {
    let message = parse_lookup(&reply(None), &refs()).unwrap_err();
    assert!(message.contains("title"), "{message}");
    assert!(parse_lookup(&reply(Some("x")), &refs()).is_ok());
}
