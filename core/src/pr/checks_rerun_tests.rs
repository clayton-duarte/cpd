//! Checks of a re-run and the head the poll stores (pro-d467, pro-d469 item 2, pro-d470): the summary comes from the latest
//! attempt of each check on the current head; `head_oid` is stored from the fresh `headRefOid` on every poll before any
//! compare, checks or Event logic, so a replaced (force-pushed) head never aborts the poll on a compare 404.
//! In process on the janitor's `Rig`: `gh` is a scripted route on the `FakeRunner`. No process, no network, no real git.

use std::sync::Arc;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use crate::args::PlanArg;
use crate::error::CpdError;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::pr::PollArgs;
use crate::pr::poll::{PollPlan, build_poll_query};
use crate::pr::status::ChecksSummary;
use crate::store::Store;

const OLD: &str = "0000000000000000000000000000000000000001";
const NEW: &str = "0000000000000000000000000000000000000002";
const ANY: &str = "[pr.feedback]\nchecks = \"any\"\n";

/// The `gh` of the poll: `acme/pay#801` is open, its head on GitHub is [`NEW`]; the compare of [`OLD`] (a replaced, gone
/// commit) answers HTTP 404, any other head is ahead of `main`. `listed` is whether the search lists the pull request
/// (Light read) or only the direct read finds it. Its rollup is `rollup`, its checks `contexts`.
struct Gh {
    listed: bool,
    rollup: &'static str,
    contexts: Vec<Value>,
}

impl Gh {
    fn node(&self) -> Value {
        json!({
            "repository": {"nameWithOwner": "acme/pay", "defaultBranchRef": {"name": "main"}, "isArchived": false},
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
            "headRefOid": NEW,
            "mergeable": "MERGEABLE",
            "mergeStateStatus": "CLEAN",
            "viewerDidAuthor": true,
            "viewerLatestReviewRequest": null,
            "reviewThreads": {"nodes": []},
            "commits": {"nodes": [{"commit": {"oid": NEW, "statusCheckRollup": {"state": self.rollup}}}]}
        })
    }
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                let nodes = if self.listed {
                    vec![self.node()]
                } else {
                    vec![]
                };
                Reply::ok(
                    &json!({"data": {
                        "search": {"issueCount": nodes.len(), "nodes": nodes},
                        "reviewRequests": {"issueCount": 0, "nodes": []},
                        "rateLimit": {"cost": 1, "remaining": 4987, "resetAt": "2026-10-20T11:00:00Z"}
                    }})
                    .to_string(),
                )
            }
            ["api", "graphql", "-f", query, ..] if query.contains("contexts(first: 100") => {
                let page = json!({"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": self.contexts});
                let pull = json!({"labels": {"nodes": []}, "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": page}}}]}});
                Reply::ok(&json!({"data": {"p0": {"pullRequest": pull}}}).to_string())
            }
            // The direct read of a pull request no search listed.
            ["api", "graphql", "-f", query, ..] if query.contains("viewerDidAuthor") => {
                Reply::ok(&json!({"data": {"p0": {"pullRequest": self.node()}}}).to_string())
            }
            ["api", "graphql", "-f", query, ..] if query.contains("reviewThreads") => {
                let empty = json!({"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []});
                let mut data = json!({"viewer": {"login": "me"}});
                for i in 0..query.matches(": repository(").count() {
                    data[format!("p{i}")] = json!({"pullRequest": {"id": format!("PR_{i}"), "updatedAt": "U1", "reviewThreads": empty, "comments": empty, "reviews": empty}});
                }
                Reply::ok(&json!({"data": data}).to_string())
            }
            ["api", endpoint, ..] if endpoint.starts_with("repos/acme/pay/compare/") => {
                if endpoint.ends_with(OLD) {
                    Reply::exit(1, "", "gh: Not Found (HTTP 404)\n")
                } else {
                    Reply::ok(r#"{"status":"ahead"}"#)
                }
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A rig with the open Workflow `api` (pull request `acme/pay#801`) whose store holds the replaced head [`OLD`], under
/// the policy `ANY`, `gh` as `gh`, `$CPD_HOME` the rig's.
fn rig(gh: Gh) -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let dir = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pay.toml"), ANY).unwrap();
    rig.sql(&format!(
        "UPDATE pull_requests SET base_ref = 'main', default_branch = 'main', head_oid = '{OLD}', checks = 'failed',
                mergeable = 'mergeable', merge_state = 'clean' WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(gh));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

fn poll(rig: &Rig) -> Result<(), CpdError> {
    Ports::scope(&rig.fakes.ports, || {
        super::poll::poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .map(|_| ())
}

fn column(rig: &Rig, column: &str) -> Option<String> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                &format!("SELECT {column} FROM pull_requests WHERE number = 801"),
                [],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?)
        })
        .unwrap()
        .flatten()
}

/// Attempt 1 of the workflow's `build` failed, attempt 2 (newer) is still running.
fn rerun() -> Vec<Value> {
    let run = |status: &str, conclusion: Value, at: &str| {
        json!({"__typename": "CheckRun", "name": "build", "status": status, "conclusion": conclusion,
               "startedAt": at, "checkSuite": {"workflowRun": {"workflow": {"name": "CI"}}}})
    };
    vec![
        run("COMPLETED", json!("FAILURE"), "2026-11-09T10:00:00Z"),
        run("IN_PROGRESS", Value::Null, "2026-11-09T10:30:00Z"),
    ]
}

#[test]
fn in_progress_rerun_attempt_is_pending_not_failed() {
    // The hydrated (Heavy) contexts carry the two times, so they reduce to the latest attempt.
    let query = build_poll_query(&PollPlan {
        heavy: vec![("acme/pay".to_owned(), 801)],
        ..PollPlan::default()
    });
    let heavy = &query[query.find("fragment Heavy").expect("the Heavy fragment")..];
    assert!(
        heavy.contains("startedAt"),
        "the Heavy selection reads startedAt of a CheckRun: {heavy}"
    );
    assert!(
        heavy.contains("createdAt")
            && heavy.contains("... on StatusContext { context state createdAt")
            || heavy.contains("context state targetUrl createdAt")
            || heavy.contains("context state createdAt"),
        "the Heavy selection reads createdAt of a StatusContext: {heavy}"
    );

    // A hydrated detail that carries them reduces to the latest attempt: pending.
    let detail = json!({"commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": rerun()}}}}]}});
    let checks = super::gh::checks_of_detail(&detail);
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], false),
        ChecksSummary::Pending
    );

    // The poll of a re-run on the current head stores pending.
    let rig = rig(Gh {
        listed: true,
        rollup: "FAILURE",
        contexts: rerun(),
    });
    poll(&rig).expect("the poll succeeds");
    assert_eq!(column(&rig, "checks").as_deref(), Some("pending"));
}

#[test]
fn head_oid_stored_on_light_and_heavy_poll() {
    // Light: the search lists the pull request.
    let light = rig(Gh {
        listed: true,
        rollup: "SUCCESS",
        contexts: vec![],
    });
    let _ = poll(&light);
    assert_eq!(
        column(&light, "head_oid").as_deref(),
        Some(NEW),
        "a Light poll stores the fresh headRefOid"
    );

    // Heavy: no search lists it, the direct read does.
    let heavy = rig(Gh {
        listed: false,
        rollup: "SUCCESS",
        contexts: vec![],
    });
    let _ = poll(&heavy);
    assert_eq!(
        column(&heavy, "head_oid").as_deref(),
        Some(NEW),
        "a direct (Heavy) read stores the fresh headRefOid"
    );
}

#[test]
fn reminder_suppressed_while_check_in_progress() {
    // The reminder fires on `pull_request.checks == failed` (pi/lib/reminder.ts): after a push replaced the head, attempt 1
    // failed and attempt 2 runs on the current head, the stored checks must be pending, so no reminder fires.
    let rig = rig(Gh {
        listed: true,
        rollup: "FAILURE",
        contexts: rerun(),
    });
    let _ = poll(&rig);
    assert_eq!(
        column(&rig, "checks").as_deref(),
        Some("pending"),
        "attempt 1 failed and attempt 2 is in progress: pending, not failed"
    );
    assert_eq!(column(&rig, "head_oid").as_deref(), Some(NEW));
}

#[test]
fn replaced_head_sha_does_not_abort_poll() {
    // The store holds a head GitHub no longer has: the compare of it is a 404. The poll stores the fresh head first, so it
    // compares that one and succeeds.
    let rig = rig(Gh {
        listed: true,
        rollup: "SUCCESS",
        contexts: vec![],
    });
    poll(&rig).expect("a replaced head's compare 404 does not abort the poll");
    assert_eq!(column(&rig, "head_oid").as_deref(), Some(NEW));
    assert_eq!(column(&rig, "base_contained").as_deref(), Some("1"));
}
