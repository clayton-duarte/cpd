//! `pr poll` cleans nothing (commands.md §9.2 "After a Workflow is merged or cancelled", contract §7.2): the clean-up
//! of merged and cancelled Workflows is `cpd janitor`'s (`janitor_tests.rs`). In process on the janitor's `Rig`: the git is
//! the in-process `Universe`, `gh` and tmux are scripted on the runner. No test starts a process or sleeps.

use std::sync::Arc;

use rusqlite::OptionalExtension;

use super::PollArgs;
use super::poll::{Trigger, poll, queue_update_for};
use crate::args::PlanArg;
use crate::config::Tier;
use crate::host::runner::{Call, Handler, Reply};
use crate::ids::{PlanId, WorkflowKey};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::store::Store;

/// The in-process `gh` of the poll: no open pull request in the search, `merged` pull requests in the lookup.
struct Gh {
    merged: Vec<i64>,
}

fn node(number: i64) -> String {
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":{number},"url":"https://github.com/acme/pay/pull/{number}","title":"Workflow {number}",
        "isDraft":false,"state":"MERGED","createdAt":"opened","updatedAt":"U1","mergedAt":"2026-10-14T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
        "reviewThreads":{{"nodes":[]}},"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    )
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                Reply::ok(
                    r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#,
                )
            }
            ["api", "graphql", "-f", query, ..] if query.contains("p0: repository") => {
                let aliases: Vec<String> = self
                    .merged
                    .iter()
                    .enumerate()
                    .map(|(i, n)| format!(r#""p{i}":{{"pullRequest":{}}}"#, node(*n)))
                    .collect();
                Reply::ok(&format!(r#"{{"data":{{{}}}}}"#, aliases.join(",")))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

#[test]
fn pr_poll_removes_and_reports_nothing_of_a_merged_or_cancelled_pr() {
    let mut rig = Rig::new();
    // `api` is open and GitHub says it merged; `web` is cancelled already.
    rig.workflow("api", "open");
    rig.workflow("web", "cancelled");
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(Gh { merged: vec![801] }));

    // `Env::open_with` reads `$CPD_HOME`: the one variable this test sets, to its own temp dir (as `inbox/snooze_time_tests.rs`).
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let out = Ports::scope(&rig.fakes.ports, || {
        poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .unwrap();

    let status: String = {
        let mut store = crate::store::Store::open(&rig.home, 5000).unwrap();
        store
            .read(|c| {
                Ok(
                    c.query_row("SELECT status FROM workflows WHERE key = 'api'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap()
    };
    assert_eq!(status, "merged", "the poll saw the merge: {out:?}");
    for workflow in &rig.workflows {
        assert_eq!(
            rig.left(workflow),
            [true; 6],
            "{} Workflow {}: the poll leaves every worktree, registration and branch",
            workflow.status,
            workflow.key
        );
    }
    let kills: Vec<Call> = rig
        .fakes
        .runner
        .calls()
        .into_iter()
        .filter(|c| c.program.ends_with("tmux") && c.args.iter().any(|a| a.starts_with("kill-")))
        .collect();
    assert!(kills.is_empty(), "the poll kills no window: {kills:?}");
    let cleanup: Vec<_> = rig
        .errors()
        .into_iter()
        .filter(|(_, data)| data["code"] == "cleanup_failed")
        .collect();
    assert_eq!(
        cleanup,
        Vec::new(),
        "the poll appends no cleanup_failed Event"
    );
}

/// The in-process `gh` of the required-checks tests: the search lists the one open Workflow `acme/pay#801` with the bulk rollup
/// `rollup` (None is a null rollup), the per-check read lists `nodes`, the comments read is empty; any other call exits 9.
struct OpenGh {
    rollup: Option<&'static str>,
    nodes: Vec<serde_json::Value>,
}

impl Handler for OpenGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                let rollup = self
                    .rollup
                    .map_or("null".to_owned(), |s| format!(r#"{{"state":"{s}"}}"#));
                let node = format!(
                    r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":801,"url":"https://github.com/acme/pay/pull/801","title":"Workflow 801",
                    "isDraft":false,"state":"OPEN","createdAt":"opened","updatedAt":"U1","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
                    "reviewThreads":{{"nodes":[]}},"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
                );
                Reply::ok(&format!(
                    r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#
                ))
            }
            ["api", "graphql", "-f", query, ..] if query.contains("reviewThreads") => {
                let empty = serde_json::json!({"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []});
                let mut data = serde_json::json!({"viewer": {"login": "me"}});
                for i in 0..query.matches(": repository(").count() {
                    data[format!("p{i}")] = serde_json::json!({"pullRequest": {"id": format!("PR_{i}"), "updatedAt": "U1", "reviewThreads": empty, "comments": empty, "reviews": empty}});
                }
                Reply::ok(&serde_json::json!({"data": data}).to_string())
            }
            ["api", "graphql", "-f", query, ..] if query.contains("contexts(first: 100") => {
                let page = serde_json::json!({"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": self.nodes});
                let pull = serde_json::json!({"labels": {"nodes": []}, "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": page}}}]}});
                Reply::ok(&serde_json::json!({"data": {"p0": {"pullRequest": pull}}}).to_string())
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

fn guardian() -> serde_json::Value {
    serde_json::json!({"__typename": "StatusContext", "context": "GitGuardian Security Checks", "state": "SUCCESS"})
}

/// Polls the open Workflow `acme/pay#801` under the repo policy file `policy` and returns the `checks` its row stores.
fn stored_checks(
    rollup: Option<&'static str>,
    nodes: Vec<serde_json::Value>,
    policy: &str,
) -> String {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let dir = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pay.toml"), policy).unwrap();
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(OpenGh { rollup, nodes }));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let out = Ports::scope(&rig.fakes.ports, || {
        poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .unwrap();
    let mut store = crate::store::Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 801",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap_or_else(|e| panic!("{e:?} {out:?}"))
}

const REQUIRED: &str = "[pr]\nrequired_checks = [\"gate\"]\n";

#[test]
fn poll_stores_pending_when_a_required_check_is_missing_on_a_passed_rollup() {
    assert_eq!(
        stored_checks(Some("SUCCESS"), vec![guardian()], REQUIRED),
        "pending"
    );
}

#[test]
fn poll_stores_pending_when_a_required_check_has_no_rollup_at_all() {
    assert_eq!(stored_checks(None, vec![], REQUIRED), "pending");
}

#[test]
fn poll_with_an_empty_required_list_stores_the_rollup_as_it_does_today() {
    assert_eq!(
        stored_checks(
            Some("SUCCESS"),
            vec![guardian()],
            "[pr]\nrequired_checks = []\n"
        ),
        "passed"
    );
    assert_eq!(
        stored_checks(None, vec![], "[pr]\nrequired_checks = []\n"),
        "none"
    );
}

#[test]
fn poll_stores_passed_when_the_required_check_passed() {
    let gate = serde_json::json!({"__typename": "CheckRun", "name": "gate", "status": "COMPLETED", "conclusion": "SUCCESS"});
    assert_eq!(
        stored_checks(Some("SUCCESS"), vec![guardian(), gate], REQUIRED),
        "passed"
    );
}

// --- the widened update-Job trigger (contract §3.7 E5, §3.8; commands.md §9.2; hs-d794, hs-n617) ---
//
// The poll queues a Workflow's `update` Job for two causes: the conflict (unchanged, `update_for_conflict`)
// and a base Workflow's merge that repoints the pull request to the default branch with the default branch
// ahead of the Lead Branch without a conflict, `reason` `base-merged`. The ahead decision is computed
// from git in the poll's read phase; these tests pass it as the boolean directly.

/// A Plan `pay` in `deploy` with the Workflows `keys` `open` (the janitor `Rig`).
fn open_rig(keys: &[&'static str]) -> Rig {
    let mut rig = Rig::new();
    for key in keys {
        rig.workflow(key, "open");
    }
    rig
}

fn tier() -> Tier {
    Tier::parse("S2").unwrap()
}

/// The keys of every `update` Job of Plan `pay`, sorted.
fn update_jobs(store: &mut Store) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT key FROM jobs WHERE plan = 'pay' AND kind = 'update' ORDER BY key",
            )?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The `reason` of the latest `job-added` Event of Plan `pay`, if any.
fn last_reason(store: &mut Store) -> Option<String> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT json_extract(data, '$.reason') FROM events WHERE kind = 'job-added' AND plan = 'pay' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
        })
        .unwrap()
}

#[test]
fn a_base_merge_with_main_ahead_queues_an_update_job() {
    let rig = open_rig(&["api"]);
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let (pay, api) = (
        PlanId::parse("pay").unwrap(),
        WorkflowKey::parse("api").unwrap(),
    );

    // The repoint happened (GitHub moved the Workflow to the default branch) and main is ahead without a conflict.
    let queued = store
        .write("pr poll", |tx| {
            queue_update_for(tx, &pay, &api, Trigger::BaseMerged, true, tier())
        })
        .unwrap()
        .expect("a base merge with main ahead queues one update Job");

    assert_eq!(
        update_jobs(&mut store),
        vec![queued.as_str().to_owned()],
        "exactly one update Job"
    );
    assert_eq!(
        last_reason(&mut store).as_deref(),
        Some("base-merged"),
        "the Job's cause is base-merged"
    );
}

#[test]
fn a_base_merge_with_main_contained_queues_nothing() {
    let rig = open_rig(&["api", "web"]);
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let (pay, api, web) = (
        PlanId::parse("pay").unwrap(),
        WorkflowKey::parse("api").unwrap(),
        WorkflowKey::parse("web").unwrap(),
    );

    let (ahead, contained) = store
        .write("pr poll", |tx| {
            // Control: the very same repoint with the default branch ahead does queue.
            let ahead = queue_update_for(tx, &pay, &api, Trigger::BaseMerged, true, tier())?;
            // The default branch is already contained in the Lead Branch: nothing.
            let contained = queue_update_for(tx, &pay, &web, Trigger::BaseMerged, false, tier())?;
            Ok((ahead, contained))
        })
        .unwrap();

    let ahead = ahead.expect("the control: an ahead default branch queues its update Job");
    assert_eq!(
        contained, None,
        "a default branch already contained queues nothing"
    );
    assert_eq!(
        update_jobs(&mut store),
        vec![ahead.as_str().to_owned()],
        "only the ahead Workflow's update Job exists"
    );
}

#[test]
fn a_conflict_still_queues_an_update_job() {
    let rig = open_rig(&["api"]);
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let (pay, api) = (
        PlanId::parse("pay").unwrap(),
        WorkflowKey::parse("api").unwrap(),
    );

    let queued = store
        .write("pr poll", |tx| {
            queue_update_for(tx, &pay, &api, Trigger::Conflict, false, tier())
        })
        .unwrap()
        .expect("the conflict still queues its update Job");

    assert_eq!(
        update_jobs(&mut store),
        vec![queued.as_str().to_owned()],
        "exactly one update Job"
    );
    assert_eq!(
        last_reason(&mut store).as_deref(),
        Some("conflict"),
        "the conflict cause is unchanged"
    );
}

#[test]
fn an_update_job_already_live_is_not_duplicated() {
    let rig = open_rig(&["api"]);
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let (pay, api) = (
        PlanId::parse("pay").unwrap(),
        WorkflowKey::parse("api").unwrap(),
    );

    let first = store
        .write("pr poll", |tx| {
            queue_update_for(tx, &pay, &api, Trigger::BaseMerged, true, tier())
        })
        .unwrap()
        .expect("the first repoint queues its update Job");

    // A queued update Job is not duplicated by the next repoint.
    let queued_again = store
        .write("pr poll", |tx| {
            queue_update_for(tx, &pay, &api, Trigger::BaseMerged, true, tier())
        })
        .unwrap();
    assert_eq!(queued_again, None, "a queued update Job is not duplicated");
    assert_eq!(update_jobs(&mut store).len(), 1, "still one update Job");

    // A running update Job is not duplicated either (the Job's own push is no new transition).
    store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE jobs SET status = 'running' WHERE key = ?1",
                [first.as_str()],
            )?;
            Ok(())
        })
        .unwrap();
    let running_again = store
        .write("pr poll", |tx| {
            queue_update_for(tx, &pay, &api, Trigger::BaseMerged, true, tier())
        })
        .unwrap();
    assert_eq!(
        running_again, None,
        "a running update Job is not duplicated"
    );
    assert_eq!(
        update_jobs(&mut store),
        vec![first.as_str().to_owned()],
        "still exactly the one update Job"
    );
}

// --- a cancelled Workflow whose pull request GitHub reopened (pro-d362, E5) ---

/// The in-process `gh` of the rebind: the search lists `acme/pay#801` as `state` (a closed pull request is not in the search),
/// `gh pr list --head` answers as the reopen's [`crate::workflow::reopen_tests::Gh`]; any other call exits 9.
struct ReopenGh {
    state: &'static str,
}

impl Handler for ReopenGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["pr", "list", ..] => crate::workflow::reopen_tests::Gh {
                heads: vec![("cpd/pay/api".to_owned(), 801, self.state)],
            }
            .handle(call),
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                let nodes = if self.state == "OPEN" {
                    r#"{"repository":{"nameWithOwner":"acme/pay"},"number":801,"url":"https://github.com/acme/pay/pull/801","title":"T",
                    "isDraft":false,"state":"OPEN","createdAt":"opened","updatedAt":"U1","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
                    "reviewThreads":{"nodes":[]},"commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}}"#
                } else {
                    ""
                };
                let count = usize::from(!nodes.is_empty());
                Reply::ok(&format!(
                    r#"{{"data":{{"search":{{"issueCount":{count},"nodes":[{nodes}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#
                ))
            }
            ["api", "graphql", "-f", query, ..] if query.contains("reviewThreads") => {
                let empty = serde_json::json!({"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []});
                let mut data = serde_json::json!({"viewer": {"login": "me"}});
                for i in 0..query.matches(": repository(").count() {
                    data[format!("p{i}")] = serde_json::json!({"pullRequest": {"id": format!("PR_{i}"), "updatedAt": "U1", "reviewThreads": empty, "comments": empty, "reviews": empty}});
                }
                Reply::ok(&serde_json::json!({"data": data}).to_string())
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A rig whose Plan `pay` is `done` with its one Workflow `api` cancelled, keeping its pull request `acme/pay#801` (stored
/// `closed`), the Lead asleep; GitHub reports that pull request as `state`. With `published` its Lead Branch is on `origin`.
fn reopened_rig(state: &'static str, published: bool) -> Rig {
    use crate::workflow::reopen_tests::{cancelled, publish, rig};
    let rig = rig("done");
    cancelled(&rig, "pay", "api", Some(801));
    if published {
        publish(&rig, "cpd/pay/api");
    }
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(ReopenGh { state }));
    rig
}

fn poll_once(rig: &Rig) -> Result<crate::output::Output, crate::error::CpdError> {
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    Ports::scope(&rig.fakes.ports, || {
        poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
}

/// `(workflow status, plan phase, done_at is null, lead asleep_at is null, pull request state)`.
fn reopened_state(rig: &Rig) -> (String, String, bool, bool, String) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT w.status, p.phase, p.done_at IS NULL, s.asleep_at IS NULL, q.state
                   FROM workflows w JOIN plans p ON p.id = w.plan JOIN sessions s ON s.id = p.lead_session
                   LEFT JOIN pull_requests q ON q.number = 801 WHERE w.key = 'api'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?)
        })
        .unwrap()
}

fn event_count(rig: &Rig, kind: &str) -> i64 {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM events WHERE kind = ?1", [kind], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

fn last_txn(rig: &Rig) -> i64 {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| Ok(c.query_row("SELECT coalesce(max(id), 0) FROM txns", [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn poll_rebinds_a_cancelled_workflow_whose_pull_request_was_reopened() {
    let rig = reopened_rig("OPEN", true);
    let out = poll_once(&rig).unwrap();
    assert_eq!(
        reopened_state(&rig),
        ("open".into(), "deploy".into(), true, true, "open".into()),
        "the poll rebinds the Workflow and moves the Plan back to deploy: {out:?}"
    );
    assert_eq!(event_count(&rig, "workflow-reopened"), 1);
    assert_eq!(event_count(&rig, "plan-phase"), 1);
}

#[test]
fn poll_leaves_a_cancelled_workflow_with_a_closed_pull_request() {
    let rig = reopened_rig("CLOSED", true);
    poll_once(&rig).unwrap();
    assert_eq!(
        reopened_state(&rig),
        (
            "cancelled".into(),
            "done".into(),
            false,
            false,
            "closed".into()
        )
    );
    assert_eq!(event_count(&rig, "workflow-reopened"), 0);
    // The control: the same rig with GitHub reporting the pull request open does rebind it.
    let open = reopened_rig("OPEN", true);
    poll_once(&open).unwrap();
    assert_eq!(reopened_state(&open).0, "open", "control");
}

#[test]
fn poll_rebind_is_idempotent_and_writes_nothing_the_second_time() {
    let rig = reopened_rig("OPEN", true);
    poll_once(&rig).unwrap();
    assert_eq!(reopened_state(&rig).0, "open", "the first poll rebinds");
    let (txn, events) = (last_txn(&rig), event_count(&rig, "workflow-reopened"));
    poll_once(&rig).unwrap();
    assert_eq!(last_txn(&rig), txn, "the second poll writes no transaction");
    assert_eq!(event_count(&rig, "workflow-reopened"), events);
    assert_eq!(reopened_state(&rig).0, "open");
}

#[test]
fn a_failed_rebind_leaves_the_workflow_cancelled_and_reports_one_error() {
    // The Lead Branch is not on `origin`: the git fake refuses to recreate the Lead worktree from it.
    let rig = reopened_rig("OPEN", false);
    poll_once(&rig).unwrap();
    poll_once(&rig).unwrap();
    let (status, phase, ..) = reopened_state(&rig);
    assert_eq!((status.as_str(), phase.as_str()), ("cancelled", "done"));
    assert_eq!(
        rig.errors().len(),
        1,
        "one error Event for the refused rebind, not one per tick: {:?}",
        rig.errors()
    );
}

/// [`ReopenGh`] listing `acme/pay#801` OPEN in the search (so the poll keeps a stored open `#801` open), but with no pull request
/// for any head: `gh pr list --head` answers `[]`.
struct NoHeadGh(ReopenGh);

impl Handler for NoHeadGh {
    fn handle(&self, call: &Call) -> Reply {
        if call.args.starts_with(&["pr".to_owned(), "list".to_owned()]) {
            return Reply::ok("[]");
        }
        self.0.handle(call)
    }
}

fn pr_list_calls(rig: &Rig) -> usize {
    (*rig.fakes.runner)
        .clone()
        .calls()
        .iter()
        .filter(|c| c.args.starts_with(&["pr".to_owned(), "list".to_owned()]))
        .count()
}

fn legacy_status(rig: &Rig) -> (String, String) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT w.status, p.phase FROM workflows w JOIN plans p ON p.id = w.plan WHERE w.key = 'api'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn poll_makes_no_gh_pr_list_call_and_no_error_for_a_legacy_cancelled_workflow() {
    use crate::workflow::reopen_tests::{cancelled, rig};
    let rig = rig("done");
    cancelled(&rig, "pay", "api", None);
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(ReopenGh { state: "CLOSED" }));
    for _ in 0..2 {
        poll_once(&rig).unwrap();
        assert_eq!(pr_list_calls(&rig), 0, "no `gh pr list` for a legacy row");
        assert!(rig.errors().is_empty(), "{:?}", rig.errors());
        assert_eq!(legacy_status(&rig), ("cancelled".into(), "done".into()));
    }
}

#[test]
fn poll_skips_a_legacy_cancelled_workflow_not_open_on_github_without_an_error() {
    use crate::workflow::reopen_tests::{cancelled, rig, stored_pull};
    let rig = rig("done");
    cancelled(&rig, "pay", "api", None);
    // The unowned open row is `acme/pay#801`: the scripted search lists it OPEN, so the poll keeps it open.
    stored_pull(&rig, 801, "open");
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(NoHeadGh(ReopenGh { state: "OPEN" })));
    for _ in 0..2 {
        poll_once(&rig).unwrap();
        assert!(rig.errors().is_empty(), "{:?}", rig.errors());
        assert_eq!(legacy_status(&rig), ("cancelled".into(), "done".into()));
    }
}
