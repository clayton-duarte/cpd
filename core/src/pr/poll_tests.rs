//! `pr poll` cleans nothing (commands.md §9.2 "After a PR is merged or cancelled", contract §7.2): the clean-up
//! of merged and cancelled PRs is `cpd janitor`'s (`janitor_tests.rs`). In process on the janitor's `Rig`: the git is
//! the in-process `Universe`, `gh` and tmux are scripted on the runner. No test starts a process or sleeps.

use std::sync::Arc;

use super::PollArgs;
use super::poll::poll;
use crate::args::PlanArg;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;

/// The in-process `gh` of the poll: no open pull request in the search, `merged` pull requests in the lookup.
struct Gh {
    merged: Vec<i64>,
}

fn node(number: i64) -> String {
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":{number},"url":"https://github.com/acme/pay/pull/{number}","title":"PR {number}",
        "isDraft":false,"state":"MERGED","createdAt":"opened","updatedAt":"U1","mergedAt":"2026-10-14T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    )
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => Reply::ok(
                r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#,
            ),
            ["api", "graphql", "-f", query, ..] if query.contains("p0: repository") => {
                let aliases: Vec<String> = self.merged.iter().enumerate().map(|(i, n)| format!(r#""p{i}":{{"pullRequest":{}}}"#, node(*n))).collect();
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
    rig.pr("api", "open");
    rig.pr("web", "cancelled");
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Gh { merged: vec![801] }));

    // `Env::open_with` reads `$CPD_HOME`: the one variable this test sets, to its own temp dir (as `inbox/snooze_time_tests.rs`).
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let out = Ports::scope(&rig.fakes.ports, || poll(&rig.fakes.ports, PollArgs { plan: PlanArg { plan: None } })).unwrap();

    let status: String = {
        let mut store = crate::store::Store::open(&rig.home, 5000).unwrap();
        store.read(|c| Ok(c.query_row("SELECT status FROM prs WHERE key = 'api'", [], |r| r.get(0))?)).unwrap()
    };
    assert_eq!(status, "merged", "the poll saw the merge: {out:?}");
    for pr in &rig.prs {
        assert_eq!(rig.left(pr), [true; 6], "{} PR {}: the poll leaves every worktree, registration and branch", pr.status, pr.key);
    }
    let kills: Vec<Call> = rig.fakes.runner.calls().into_iter().filter(|c| c.program.ends_with("tmux") && c.args.iter().any(|a| a.starts_with("kill-"))).collect();
    assert!(kills.is_empty(), "the poll kills no window: {kills:?}");
    let cleanup: Vec<_> = rig.errors().into_iter().filter(|(_, data)| data["code"] == "cleanup_failed").collect();
    assert_eq!(cleanup, Vec::new(), "the poll appends no cleanup_failed Event");
}

/// The in-process `gh` of the required-checks tests: the search lists the one open PR `acme/pay#801` with the bulk rollup
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
                let rollup = self.rollup.map_or("null".to_owned(), |s| format!(r#"{{"state":"{s}"}}"#));
                let node = format!(
                    r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":801,"url":"https://github.com/acme/pay/pull/801","title":"PR 801",
                    "isDraft":false,"state":"OPEN","createdAt":"opened","updatedAt":"U1","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
                    "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
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

/// Polls the open PR `acme/pay#801` under the repo policy file `policy` and returns the `checks` its row stores.
fn stored_checks(rollup: Option<&'static str>, nodes: Vec<serde_json::Value>, policy: &str) -> String {
    let mut rig = Rig::new();
    rig.pr("api", "open");
    let dir = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pay.toml"), policy).unwrap();
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(OpenGh { rollup, nodes }));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let out = Ports::scope(&rig.fakes.ports, || poll(&rig.fakes.ports, PollArgs { plan: PlanArg { plan: None } })).unwrap();
    let mut store = crate::store::Store::open(&rig.home, 5000).unwrap();
    store.read(|c| Ok(c.query_row("SELECT checks FROM pull_requests WHERE number = 801", [], |r| r.get(0))?)).unwrap_or_else(|e| panic!("{e:?} {out:?}"))
}

const REQUIRED: &str = "[pr]\nrequired_checks = [\"gate\"]\n";

#[test]
fn poll_stores_pending_when_a_required_check_is_missing_on_a_passed_rollup() {
    assert_eq!(stored_checks(Some("SUCCESS"), vec![guardian()], REQUIRED), "pending");
}

#[test]
fn poll_stores_pending_when_a_required_check_has_no_rollup_at_all() {
    assert_eq!(stored_checks(None, vec![], REQUIRED), "pending");
}

#[test]
fn poll_with_an_empty_required_list_stores_the_rollup_as_it_does_today() {
    assert_eq!(stored_checks(Some("SUCCESS"), vec![guardian()], "[pr]\nrequired_checks = []\n"), "passed");
    assert_eq!(stored_checks(None, vec![], "[pr]\nrequired_checks = []\n"), "none");
}

#[test]
fn poll_stores_passed_when_the_required_check_passed() {
    let gate = serde_json::json!({"__typename": "CheckRun", "name": "gate", "status": "COMPLETED", "conclusion": "SUCCESS"});
    assert_eq!(stored_checks(Some("SUCCESS"), vec![guardian(), gate], REQUIRED), "passed");
}
