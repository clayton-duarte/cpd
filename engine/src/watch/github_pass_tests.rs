//! E5 owns the GitHub SWR pass (docs/architecture/swr.md §12, sdk.md, swr-github.md §4): one engine tick's E5 steps run
//! the source's `fetch` next to its outbox drain, so the pass's read fills `pull_requests.head_oid`, `swr_repos` and the
//! `github` endpoint. A `Fakes` runner scripts the one `gh api graphql` call; no `gh`, git, tmux or network runs.

use std::sync::Arc;

use serde_json::{Value, json};

use super::pr;
use super::testkit::Fx;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ports::Fakes;
use cpd_core::swr::endpoint;

/// The head the scripted reply carries; the test asserts `pull_requests.head_oid` becomes it.
const HEAD: &str = "a1";

/// GitHub's answer to the one `gh api graphql` call: the viewer, the (empty) searches, the repository's metadata and the
/// pull request the reply carries (`p0` and `m0` are the first heavy and the first repository of the pass's plan).
struct Gh;

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        assert_eq!(
            &call.args[..2],
            ["api", "graphql"],
            "the pass sends one GraphQL read: {:?}",
            call.args
        );
        Reply::ok(
            &json!({
                "data": {
                    "viewer": {"login": "ada"},
                    "rateLimit": {"remaining": 4000, "resetAt": "2026-01-01T01:00:00.000Z", "cost": 1},
                    "authored": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []},
                    "reviews": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []},
                    "p0": {"pullRequest": pull_request()},
                    "m0": {
                        "defaultBranchRef": {"name": "main"},
                        "isArchived": false,
                        "autoMergeAllowed": true,
                        "squashMergeAllowed": true,
                        "mergeCommitAllowed": false,
                        "rebaseMergeAllowed": false,
                        "deleteBranchOnMerge": true
                    }
                }
            })
            .to_string(),
        )
    }
}

/// What E5's legacy `cpd pr poll` prints: no pull request changed, room in the rate limit.
fn poll_ok() -> String {
    json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}})
        .to_string()
}

/// The pull request `acme/pay#801` as the reply carries it: open, with the head [`HEAD`].
fn pull_request() -> Value {
    json!({
        "number": 801,
        "state": "OPEN",
        "isDraft": false,
        "mergeable": "MERGEABLE",
        "mergeStateStatus": "CLEAN",
        "reviewDecision": null,
        "baseRefName": "main",
        "headRefName": "feat",
        "headRefOid": HEAD,
        "updatedAt": "U1",
        "title": "Charge the card at checkout",
        "url": "https://github.com/acme/pay/pull/801",
        "createdAt": "2026-10-01T00:00:00.000Z",
        "mergedAt": null,
        "author": {"login": "ada"},
        "repository": {"nameWithOwner": "acme/pay", "defaultBranchRef": {"name": "main"}, "isArchived": false},
        "labels": {"nodes": []},
        "autoMergeRequest": null,
        "commits": {"nodes": [{"commit": {"oid": HEAD, "statusCheckRollup": {"state": "SUCCESS", "contexts": {"nodes": []}}}}]},
        "latestReviews": {"nodes": []},
        "reviewThreads": {"nodes": []},
        "comments": {"nodes": []},
        "viewerDidAuthor": false,
        "viewerLatestReviewRequest": null
    })
}

#[test]
fn a_tick_fills_the_head_oid_from_the_github_pass() {
    // The home, on a `Fakes` runner that answers the pass's `gh api graphql` and E5's legacy poll cleanly (so a tick
    // leaves no failed-process Event behind).
    let mut fx = Fx::new("");
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe("/opt/cpd/bin/cpd");
    let _ = (*fakes.runner).clone().route("gh", Arc::new(Gh)).on(
        "cpd",
        |_| true,
        Reply::ok(&poll_ok()),
    );
    fx.engine.ports = fakes.ports.clone();
    // An open pull request of the Workflow's repo, with the head the last legacy poll left (none yet): the pass is the
    // only thing that fills it (air-n82).
    fx.fixture(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, head_oid)
         VALUES ('acme/pay', 801, 'https://github.com/acme/pay/pull/801', 'Charge the card at checkout', 0, 'open', 'none', 'passed',
                 '2026-10-01T00:00:00.000Z', '2026-10-01T00:00:00.000Z', 1, NULL);",
    );

    // One engine tick's E5 steps, which own the GitHub source: after them the SWR pass has run against the scripted
    // reply. (`pr::start` starts E5's poll and `pr::reap` runs E5's in-tick GitHub work; `pr::finish` drains whatever
    // poll a start left, so no thread outlives the test.)
    pr::reap(&mut fx.engine);
    pr::start(&mut fx.engine);
    pr::finish(&mut fx.engine);

    assert_eq!(
        fx.query::<Option<String>>(
            "SELECT head_oid FROM pull_requests WHERE repo = 'acme/pay' AND number = 801"
        ),
        Some(HEAD.to_owned()),
        "the pass writes the head the reply's headRefOid carries"
    );
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM swr_repos WHERE repo = 'acme/pay'"),
        1,
        "the pass stores the repository's row"
    );
    assert_eq!(
        endpoint::get(&mut fx.engine.store, "github")
            .unwrap()
            .expect("the pass touched the github endpoint")
            .status
            .as_deref(),
        Some("success"),
        "the github endpoint is success"
    );
}
