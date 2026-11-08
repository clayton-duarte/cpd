//! The poll's armed merge: a pull request whose merge intent was left by a behind refusal is merged once the required
//! checks pass on a head that contains the default branch. In process on the janitor's `Rig`: `gh` is a scripted
//! route on the `FakeRunner`, the store is the rig's temp database. No process, no network, no real git.

use std::sync::Arc;

use serde_json::json;

use crate::args::PlanArg;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::pr::PollArgs;
use crate::store::Store;

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// The `gh` of the poll: the open pull request `acme/pay#801` is clean and passed, its comments are empty, and its
/// head contains `main`.
struct Gh;

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                Reply::ok(&search_reply())
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
            ["api", _, ..] if args[1].starts_with("repos/acme/pay/compare/main...") => {
                Reply::ok(r#"{"status":"ahead"}"#)
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

fn search_reply() -> String {
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

/// A rig with the open Workflow `api` of Plan `pay` (pull request `acme/pay#801`), armed for merge, with its checks
/// still pending in the cache and the head `HEAD`; the poll's read says the checks passed and the head contains `main`.
fn rig() -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET base_ref = 'main', default_branch = 'main', head_oid = '{HEAD}',
                mergeable = 'mergeable', merge_state = 'clean', checks = 'pending', merge_armed = 1 WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Gh));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

#[test]
fn an_armed_pr_is_merged_once_the_checks_pass_on_a_contained_head() {
    let rig = rig();
    let _out = Ports::scope(&rig.fakes.ports, || {
        super::poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .expect("the poll succeeds");

    let mut store = Store::open(&rig.home, 5000).unwrap();
    let merges: Vec<String> = store
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT target_key FROM swr_mutations WHERE op_kind = 'merge'")?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(
        merges,
        vec!["github:pr:acme/pay#801".to_owned()],
        "the armed PR's merge is enqueued"
    );
}
