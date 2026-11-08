//! The poll's persisted containment flag (contract §2.2 "Ready to merge", commands.md §7.6; Plan `merge-uptodate`):
//! each pass sets and refreshes `pull_requests.base_contained` for an open pull request whose `base_ref` is its
//! `default_branch`, from the fresh gh comparison the merge gate reads at merge time; a stacked pull request is not
//! subject to the rule and its flag stays NULL. In process on the janitor's `Rig`: `gh` is a scripted route on the
//! `FakeRunner`, the store is the rig's temp database. No process, no network, no real git.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// The `gh` of the poll: the open pull request `acme/pay#801` is clean and passed; the head it reports follows the
/// round, and the compare answers whether the asked head contains `main`.
#[derive(Default)]
struct Gh {
    round: AtomicUsize,
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
                Reply::ok(&search_reply(self.head()))
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
                let status = if endpoint.ends_with(BEHIND) {
                    "behind"
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

fn search_reply(head: &str) -> String {
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
        "state": "OPEN",
        "createdAt": "opened",
        "updatedAt": "U1",
        "mergedAt": null,
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
/// pass writes), its base `base` and head `head`, `gh` as [`Gh`], `$CPD_HOME` the rig's.
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
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

/// One pass of `pr poll` on the rig's Ports.
fn poll(rig: &Rig) {
    Ports::scope(&rig.fakes.ports, || {
        super::poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .expect("the poll succeeds");
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
