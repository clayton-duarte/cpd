//! The poll's persisted containment flag (contract §2.2 "Ready to merge", commands.md §7.6; Plan `merge-uptodate`):
//! each pass sets and refreshes `pull_requests.base_contained` for an open pull request whose `base_ref` is its
//! `default_branch`, from the fresh gh comparison the merge gate reads at merge time; a stacked pull request is not
//! subject to the rule and its flag stays NULL. In process on the janitor's `Rig`: `gh` is a scripted route on the
//! `FakeRunner`, the store is the rig's temp database. No process, no network, no real git.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rusqlite::OptionalExtension;
use serde_json::json;

use crate::args::PlanArg;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::pr::PollArgs;
use crate::store::Store;

const CONTAINED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BEHIND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// A head the pull request had before a force-push: GitHub answers 404 to a compare on it.
const REPLACED: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// The `gh` of the poll: the open pull request `acme/pay#801` is clean and passed; the head it reports follows the
/// round, and the compare answers whether the asked head contains `main`.
#[derive(Default)]
struct Gh {
    round: AtomicUsize,
    /// The search lists the pull request as `MERGED` instead of `OPEN`.
    merged: AtomicBool,
    /// The endpoint of every compare call.
    compares: Mutex<Vec<String>>,
}

impl Gh {
    /// The head the searches and the direct head read report this round.
    fn head(&self) -> &'static str {
        if self.round.load(Ordering::SeqCst) == 0 {
            CONTAINED
        } else {
            BEHIND
        }
    }

    /// The compare endpoints called so far.
    fn compared(&self) -> Vec<String> {
        self.compares.lock().unwrap().clone()
    }

    /// Moves the head the fake GitHub reports, as a push between polls does.
    fn bump(&self) {
        self.round.fetch_add(1, Ordering::SeqCst);
    }
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                Reply::ok(&search_reply(
                    self.head(),
                    self.merged.load(Ordering::SeqCst),
                ))
            }
            ["api", "graphql", "-f", query, ..] if query.contains("reviewThreads") => {
                Reply::ok(&comments_reply())
            }
            ["api", "graphql", "-f", query, ..] if query.contains("contexts(first: 100") => {
                Reply::ok(&checks_reply())
            }
            ["api", "graphql", "-f", query, ..] if query.contains("p0: repository") => {
                Reply::ok(r#"{"data":{"p0":{"pullRequest":null}}}"#)
            }
            // The containment comparison the merge gate reads: `ahead`/`identical` mean the head contains the base's head.
            ["api", endpoint, ..] if endpoint.starts_with("repos/acme/pay/compare/") => {
                self.compares.lock().unwrap().push(endpoint.to_owned());
                let status = if endpoint.ends_with(BEHIND) {
                    "behind"
                } else if endpoint.ends_with(REPLACED) {
                    // GitHub no longer has the replaced commit.
                    return Reply::exit(1, "", "gh: Not Found (HTTP 404)\n");
                } else {
                    "ahead"
                };
                Reply::ok(&format!(r#"{{"status":"{status}"}}"#))
            }
            ["pr", "view", _, "--repo", _, "--json", "headRefOid"] => {
                Reply::ok(&format!(r#"{{"headRefOid":"{}"}}"#, self.head()))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

fn search_reply(head: &str, merged: bool) -> String {
    let node = json!({
        "repository": {
            "nameWithOwner": "acme/pay",
            "defaultBranchRef": {"name": "main"},
            "isArchived": false
        },
        "number": 801,
        "url": "https://github.com/acme/pay/pull/801",
        "title": "Workflow 801",
        "isDraft": false,
        "state": if merged { "MERGED" } else { "OPEN" },
        "createdAt": "opened",
        "updatedAt": "U1",
        "mergedAt": if merged { json!("2026-11-08T12:00:00Z") } else { json!(null) },
        "baseRefName": "main",
        "reviewDecision": null,
        "latestOpinionatedReviews": {"nodes": []},
        "headRefOid": head,
        "mergeable": "MERGEABLE",
        "mergeStateStatus": "CLEAN",
        "viewerDidAuthor": true,
        "viewerLatestReviewRequest": null,
        "reviewThreads": {"nodes": []},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
    });
    json!({
        "data": {
            "search": {"issueCount": 1, "nodes": [node]},
            "reviewRequests": {"issueCount": 0, "nodes": []},
            "rateLimit": {"cost": 1, "remaining": 4987, "resetAt": "2026-10-20T11:00:00Z"}
        }
    })
    .to_string()
}

fn comments_reply() -> String {
    json!({
        "data": {
            "viewer": {"login": "me"},
            "p0": {
                "pullRequest": {
                    "id": "PR_1",
                    "updatedAt": "U1",
                    "reviewThreads": {"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []},
                    "comments": {"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []},
                    "reviews": {"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []}
                }
            }
        }
    })
    .to_string()
}

fn checks_reply() -> String {
    json!({
        "data": {
            "p0": {
                "pullRequest": {
                    "labels": {"nodes": []},
                    "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": []
                    }}}}]}
                }
            }
        }
    })
    .to_string()
}

/// A rig with the open Workflow `api` of Plan `pay` (pull request `acme/pay#801`) whose stored row is stale (so each
/// pass writes), its base `base` and head `head`, `gh` as [`Gh`], its home the rig's own temp home.
fn rig(gh: Arc<Gh>, base: &str, head: &str) -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET base_ref = '{base}', default_branch = 'main', head_oid = '{head}', title = 'stale',
                mergeable = 'mergeable', merge_state = 'clean', checks = 'passed' WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    let _ = (*rig.fakes.runner).clone().route("gh", gh);
    rig
}

/// One pass of `pr poll` on the rig's Ports.
fn try_poll(rig: &Rig) -> Result<(), crate::error::CpdError> {
    Ports::scope(&rig.fakes.ports, || {
        super::poll_in(
            &rig.fakes.ports,
            &rig.home,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .map(drop)
}

/// One pass of `pr poll`, which must succeed.
fn poll(rig: &Rig) {
    try_poll(rig).expect("the poll succeeds");
}

/// The stored `head_oid` of `number`.
fn head_oid(rig: &Rig, number: i64) -> Option<String> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT head_oid FROM pull_requests WHERE number = ?1",
                [number],
                |r| r.get::<_, Option<String>>(0),
            )?)
        })
        .unwrap()
}

/// The stored `base_contained` of `number`: `None` when NULL.
fn flag(rig: &Rig, number: i64) -> Option<i64> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT base_contained FROM pull_requests WHERE number = ?1",
                [number],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?)
        })
        .unwrap()
        .flatten()
}

#[test]
fn the_poll_persists_the_containment_flag() {
    // A head that contains the current default-branch head: the flag is set to 1.
    let gh = Arc::new(Gh::default());
    let default = rig(gh.clone(), "main", CONTAINED);
    poll(&default);
    assert_eq!(
        flag(&default, 801),
        Some(1),
        "the poll persists base_contained 1 for a head that contains main"
    );

    // The head moves behind the default branch's head: the flag is refreshed to 0.
    gh.bump();
    default.sql(&format!(
        "UPDATE pull_requests SET head_oid = '{BEHIND}', title = 'stale2' WHERE number = 801;"
    ));
    poll(&default);
    assert_eq!(
        flag(&default, 801),
        Some(0),
        "the poll refreshes base_contained to 0 when the head moves behind main"
    );

    // A stacked pull request (its base is another Workflow's Lead Branch): not subject to the rule, the flag stays NULL.
    let stacked = rig(Arc::new(Gh::default()), "feature", CONTAINED);
    poll(&stacked);
    assert_eq!(
        flag(&stacked, 801),
        None,
        "a stacked pull request's base_contained stays NULL"
    );
}

#[test]
fn base_contained_from_poll_read() {
    // The stored head is stale (the pull request was pushed since): the flag comes from the fresh `headRefOid` of the
    // poll's read, never from the stored head, and the stored head is updated from it first.
    let gh = Arc::new(Gh::default());
    let rig = rig(gh.clone(), "main", REPLACED);
    poll(&rig);
    assert_eq!(
        flag(&rig, 801),
        Some(1),
        "base_contained is derived from the head the poll read reports"
    );
    assert_eq!(
        head_oid(&rig, 801).as_deref(),
        Some(CONTAINED),
        "head_oid is updated from the poll read before the containment check"
    );
    assert!(
        gh.compared().iter().all(|e| !e.ends_with(REPLACED)),
        "no compare is made on the stale stored head: {:?}",
        gh.compared()
    );
}

#[test]
fn replaced_head_sha_compare_404_does_not_abort_poll() {
    // Force-push: the stored head is gone from GitHub (a compare on it is HTTP 404). The poll must not abort; the flag is
    // computed on the fresh head (here behind main, so 0).
    let gh = Arc::new(Gh::default());
    gh.bump();
    let rig = rig(gh.clone(), "main", REPLACED);
    let result = try_poll(&rig);
    assert!(
        result.is_ok(),
        "a 404 on a replaced head never aborts the poll: {result:?}"
    );
    assert_eq!(
        flag(&rig, 801),
        Some(0),
        "base_contained comes from the fresh headRefOid, which is behind main"
    );
    assert!(
        gh.compared().iter().all(|e| !e.ends_with(REPLACED)),
        "no compare on the replaced head: {:?}",
        gh.compared()
    );
}

#[test]
fn merged_pr_closes_stale_pr_ready() {
    // The row is already stored `merged` (a header write stores a merge without closing the Inbox item) and its `pr-ready`
    // item is still open: a poll that sees the pull request MERGED closes it, whether or not the upsert changed the row.
    let gh = Arc::new(Gh::default());
    gh.merged.store(true, Ordering::SeqCst);
    let rig = rig(gh, "main", CONTAINED);
    rig.sql(
        "UPDATE pull_requests SET state = 'merged', merged_at = '2026-11-08T12:00:00Z', title = 'Workflow 801',
                created_at = 'opened', authored = 1 WHERE number = 801;
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
           VALUES ('pr-ready', 'yellow', 'pr:acme/pay#801', 'pay#801 ready', 't');",
    );
    poll(&rig);
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let open: i64 = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM inbox_items WHERE kind = 'pr-ready' AND subject = 'pr:acme/pay#801' AND closed_at IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(open, 0, "the stale pr-ready item of a merged PR is closed");
}
