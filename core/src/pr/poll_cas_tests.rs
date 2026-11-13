//! The poll's follow of a repointed stacked pull request: a pull request merged in the same pass is not followed, so the
//! pass does not roll back with `cas_lost`. In process on the janitor's `Rig`; `gh` is a scripted route on the `FakeRunner`.

use std::sync::Arc;

use serde_json::json;

use crate::args::PlanArg;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::pr::PollArgs;
use crate::store::Store;

/// The `gh` of the poll: the search lists `acme/pay#801` (Workflow `a`) and `#802` (Workflow `b`) as `states`, both with
/// `baseRefName` `main` for `#802`.
struct Gh {
    a: &'static str,
    b: &'static str,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                Reply::ok(&search_reply(self.a, self.b))
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
            ["api", _, ..] if args[1].starts_with("repos/acme/pay/compare/") => {
                Reply::ok(r#"{"status":"ahead"}"#)
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

fn node(number: u32, state: &str) -> serde_json::Value {
    let merged = state == "MERGED";
    json!({
        "repository": {
            "nameWithOwner": "acme/pay",
            "defaultBranchRef": {"name": "main"},
            "isArchived": false
        },
        "number": number,
        "url": format!("https://github.com/acme/pay/pull/{number}"),
        "title": format!("Workflow {number}"),
        "isDraft": false,
        "state": state,
        "createdAt": "opened",
        "updatedAt": "U2",
        "mergedAt": if merged { json!("2026-01-01T00:00:00Z") } else { json!(null) },
        "baseRefName": "main",
        "reviewDecision": null,
        "latestOpinionatedReviews": {"nodes": []},
        "mergeable": "MERGEABLE",
        "mergeStateStatus": "CLEAN",
        "viewerDidAuthor": true,
        "viewerLatestReviewRequest": null,
        "reviewThreads": {"nodes": []},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
    })
}

fn search_reply(a: &str, b: &str) -> String {
    json!({
        "data": {
            "search": {"issueCount": 2, "nodes": [node(801, a), node(802, b)]},
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

/// A rig with open Workflow `a` (`#801`) and open Workflow `b` (`#802`) stacked on `a`'s Lead Branch.
fn rig(a: &'static str, b: &'static str) -> Rig {
    let mut rig = Rig::new();
    rig.workflow("a", "open");
    rig.workflow("b", "open");
    rig.sql(
        "UPDATE workflows SET base = 'cpd/pay/a' WHERE key = 'b';
         UPDATE pull_requests SET base_ref = 'main', default_branch = 'main';
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, 't');",
    );
    let owner = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("owner.toml"), "").unwrap();
    // The model catalog a followed repoint reads for its update Job's tier: a `pi` install in a temp dir (on PATH), `node`
    // a scripted reply.
    let agent = rig.home.root().join("agent");
    let release = agent.join("install/releases/1.0.0/node_modules/@earendil-works");
    for entry in ["pi-coding-agent", "pi-ai"] {
        std::fs::create_dir_all(release.join(entry).join("dist")).unwrap();
        std::fs::write(release.join(entry).join("dist/index.js"), "").unwrap();
    }
    std::fs::write(agent.join("install/current-version"), "1.0.0\n").unwrap();
    std::fs::create_dir_all(agent.join("bin")).unwrap();
    let pi = agent.join("bin/pi");
    std::fs::write(&pi, "").unwrap();
    std::fs::set_permissions(&pi, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    unsafe { std::env::set_var("PATH", agent.join("bin")) };
    let _ = (*rig.fakes.runner).clone().on(
        "node",
        |_| true,
        Reply::ok(r#"{"O":["off","low","medium","high"],"S":["off","low","medium","high"],"H":["off","low","medium","high"],"G":["off"],"J":["off"]}"#),
    );
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(Gh { a, b }));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

fn run(rig: &Rig) {
    Ports::scope(&rig.fakes.ports, || {
        super::poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .expect("the poll pass succeeds, no cas_lost");
}

/// `(workflow key, status, base)` of every Workflow, and the Event kinds of the pass.
fn state(rig: &Rig) -> (Vec<(String, String, String)>, Vec<String>) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            let w = c
                .prepare("SELECT key, status, base FROM workflows ORDER BY key")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let e = c
                .prepare("SELECT kind FROM events ORDER BY id")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok((w, e))
        })
        .unwrap()
}

#[test]
fn stacked_pr_repointed_and_merged_in_one_pass_does_not_cas_lost() {
    let rig = rig("MERGED", "MERGED");
    run(&rig);
    let (workflows, events) = state(&rig);
    assert!(
        workflows.iter().all(|(_, status, _)| status == "merged"),
        "both Workflows are merged: {workflows:?}"
    );
    assert_eq!(
        events.iter().filter(|k| *k == "pr-merged").count(),
        2,
        "pr-merged for both: {events:?}"
    );
}

#[test]
fn only_the_base_merged_and_the_stacked_pr_open_follows_the_repoint() {
    let rig = rig("MERGED", "OPEN");
    run(&rig);
    let (workflows, events) = state(&rig);
    assert_eq!(
        workflows,
        vec![
            ("a".into(), "merged".into(), "main".into()),
            ("b".into(), "open".into(), "main".into()),
        ]
    );
    assert!(
        events.iter().any(|k| k == "workflow-base-changed"),
        "{events:?}"
    );
}
