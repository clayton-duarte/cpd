//! The GitHub budget rules (docs/architecture/swr-github.md §3.1, §3.2, §4.2, §4.3; gh-budget-and-swr, pro-d474, pro-d477):
//! `PeerLight` for the `reviews` search, `HEAVY_CAP` 5 and `ABSENT_CAP` 10 (authored or owned first, then the oldest), the
//! Slack post of a merged pull request going inactive, a repository that resolves to null, and the cold LISTED memory.
//! `gh` is the in-process `World` of `github_tests` (or a handler here); time is virtual; nothing sleeps.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::github_tests::{Rig, pr};
use crate::host::runner::{Call, Handler, Reply};
use crate::pr::poll::{PollPlan, build_poll_query};

/// The text of `fragment <name> on PullRequest { ... }`: from its keyword to the next fragment or the document's end.
fn fragment(q: &str, name: &str) -> String {
    let start = q
        .find(&format!("fragment {name} on PullRequest"))
        .unwrap_or_else(|| panic!("the document defines fragment {name}: {q}"));
    let rest = &q[start + 1..];
    let end = rest.find("fragment ").map_or(q.len(), |i| start + 1 + i);
    q[start..end].to_owned()
}

/// The pull request numbers of the `p<i>` aliases of a document, in order.
fn aliased(q: &str) -> Vec<i64> {
    q.split("pullRequest(number: ")
        .skip(1)
        .map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap()
        })
        .collect()
}

fn created(rig: &mut Rig, n: i64, day: u32, authored: bool, review_requested: bool) {
    rig.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
           authored, review_requested, head_oid, gh_updated_at)
         VALUES ('acme/pay', {n}, 'u', 'PR {n}', 0, 'open', 'none', 'passed', '2026-10-{day:02}T00:00:00Z', 't',
           {}, {}, 'a1', 'U1')",
        i32::from(authored),
        i32::from(review_requested)
    ));
}

#[test]
fn reviews_search_uses_light_fragment() {
    let q = build_poll_query(&PollPlan::default());
    let (_, reviews) = q.split_once("reviews: search").expect("the reviews search");
    let reviews = reviews.split(" authored: search").next().unwrap();
    let reviews = &reviews[..reviews.find(" } }").map_or(reviews.len(), |i| i + 4)];
    assert!(reviews.contains("...PeerLight"), "{reviews}");
    assert!(!reviews.contains("...Light"), "{reviews}");
    let peer = fragment(&q, "PeerLight");
    for field in [
        "mergeable",
        "mergeStateStatus",
        "autoMergeRequest",
        "reviewThreads",
    ] {
        assert!(!peer.contains(field), "PeerLight omits {field}: {peer}");
    }
    let (_, authored) = q.split_once("authored: search").unwrap();
    let authored = authored.split(" reviews: search").next().unwrap();
    assert!(
        authored.contains("...Light") && !authored.contains("...PeerLight"),
        "the authored search keeps Light: {authored}"
    );
}

#[test]
fn peer_light_omits_mergeable_and_merge_state() {
    let q = build_poll_query(&PollPlan::default());
    let peer = fragment(&q, "PeerLight");
    assert!(!peer.contains("mergeable"), "{peer}");
    assert!(!peer.contains("mergeStateStatus"), "{peer}");
    for kept in ["headRefOid", "updatedAt", "latestReviews", "labels"] {
        assert!(peer.contains(kept), "PeerLight still reads {kept}: {peer}");
    }
    let light = fragment(&q, "Light");
    assert!(
        light.contains("mergeable") && light.contains("mergeStateStatus"),
        "Light keeps them for the User's own pull requests: {light}"
    );
}

#[test]
fn heavy_cap_is_five() {
    let mut rig = Rig::new();
    for n in 1..=12 {
        // No detail row: every one needs the Heavy selection.
        let p = pr(n);
        rig.seed(&p, true);
        rig.state().world.insert(("acme/pay".into(), n), p);
    }
    rig.pass(0).unwrap();
    let q = rig.graphql().remove(0);
    assert_eq!(q.matches("...Heavy").count(), 5, "{q}");
}

#[test]
fn aliases_per_pass_capped_at_ten() {
    let mut rig = Rig::new();
    for n in 1..=30 {
        let p = pr(n);
        rig.seed(&p, true);
        rig.seed_detail(&p);
        rig.state().world.insert(("acme/pay".into(), n), p);
    }
    // The first pass warms the listed memory (no search lists them); the second reads them as absent.
    rig.pass(0).unwrap();
    rig.pass(60).unwrap();
    let q = rig.graphql().pop().unwrap();
    assert_eq!(aliased(&q).len(), 10, "{q}");
}

#[test]
fn absent_cap_ten_oldest_first() {
    let mut rig = Rig::new();
    // Authored 1..=14, the higher the number the older (created on day 20 - n); two peer review requests, the oldest of all.
    for n in 1..=14 {
        created(&mut rig, n, u32::try_from(20 - n).unwrap(), true, false);
        let p = pr(n);
        rig.seed_detail(&p);
        rig.state().world.insert(("acme/pay".into(), n), p);
    }
    for n in [101, 102] {
        created(&mut rig, n, 1, false, true);
        rig.state().world.insert(("acme/pay".into(), n), pr(n));
    }
    rig.pass(0).unwrap();
    rig.pass(60).unwrap();
    let q = rig.graphql().pop().unwrap();
    let mut got = aliased(&q);
    got.sort_unstable();
    assert_eq!(
        got,
        (5..=14).collect::<Vec<i64>>(),
        "authored first, then the oldest by creation: {q}"
    );
}

#[test]
fn merged_pr_sets_slack_post_inactive() {
    let mut rig = Rig::new();
    let mut merged = pr(7);
    merged.state = "MERGED";
    rig.state().world.insert(("acme/pay".into(), 7), merged);
    rig.sql(&format!(
        "INSERT INTO swr_slack_posts (channel, ts, repo, number, primary_pr, author_is_me, reactions_json, fetched_at)
         VALUES ('C1', '{}.000100', 'acme/pay', 7, 1, 0, '{{}}', 't')",
        rig.t0.unix_timestamp()
    ));
    rig.pass(0).unwrap();
    assert!(
        rig.text("SELECT inactive_at FROM swr_slack_posts WHERE number = 7")
            .is_some(),
        "the read of a merged pull request closes its Slack post"
    );
}

/// GitHub's answer when every repository of the document is gone: the `m<i>` aliases are null.
struct Gone;

impl Handler for Gone {
    fn handle(&self, call: &Call) -> Reply {
        let query = call
            .args
            .iter()
            .find_map(|a| a.strip_prefix("query="))
            .unwrap_or_default();
        let mut data = serde_json::Map::new();
        data.insert("viewer".into(), json!({"login": "ada"}));
        data.insert(
            "rateLimit".into(),
            json!({"remaining": 4000, "resetAt": "2030-01-01T00:00:00Z", "cost": 1}),
        );
        for alias in ["authored", "reviews"] {
            data.insert(
                alias.into(),
                json!({"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []}),
            );
        }
        for i in 0..query.matches(" m").count() {
            if query.contains(&format!("m{i}: repository(")) {
                data.insert(format!("m{i}"), serde_json::Value::Null);
            }
        }
        Reply::ok(&json!({"data": data}).to_string())
    }
}

#[test]
fn null_repo_is_archived_and_not_requeried() {
    let mut rig = Rig::new();
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Gone));
    rig.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
           authored, head_oid, gh_updated_at)
         VALUES ('acme/gone', 1, 'u', 'PR 1', 0, 'open', 'none', 'passed', 'c', 't', 1, 'a1', 'U1')",
    );
    rig.sql(
        "INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
         VALUES ('acme/gone', 1, 'a1', 'U1', 't', '{}', 'h')",
    );
    rig.pass(0).unwrap();
    assert_eq!(
        rig.int("SELECT archived FROM pull_requests WHERE repo = 'acme/gone'"),
        1,
        "the rows of a repository that resolves to null are archived"
    );
    assert!(
        rig.text("SELECT inactive_at FROM swr_repos WHERE repo = 'acme/gone'")
            .is_some(),
        "and the repository is tombstoned"
    );
    // Well past the 24 h metadata TTL: a tombstoned repository is still never queried.
    rig.pass(48 * 3600).unwrap();
    let queries = rig.graphql();
    assert_eq!(queries.len(), 2);
    assert!(
        !queries[1].contains("name: \"gone\""),
        "the second pass does not query the repository: {}",
        queries[1]
    );
}

#[test]
fn cold_listed_reconciles_stale_open_rows() {
    // A fresh engine memory (a restart): nothing is known as listed. The stored open row is stale: GitHub says it merged,
    // and the search no longer lists it.
    let mut rig = Rig::new();
    let stored = pr(1);
    rig.seed(&stored, true);
    rig.seed_detail(&stored);
    let mut merged = pr(1);
    merged.state = "MERGED";
    rig.state().world.insert(("acme/pay".into(), 1), merged);
    rig.pass(0).unwrap();
    let q = rig.graphql().remove(0);
    assert_eq!(aliased(&q), vec![1], "the cold pass reads the row as absent: {q}");
    assert_eq!(rig.pr_state(1).as_deref(), Some("merged"));
    let _ = Duration::ZERO;
}
