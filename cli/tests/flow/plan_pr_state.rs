//! Guarantee (c) of docs/architecture/swr-github.md §10: a Manager reads each Workflow's PR state from the cache with one
//! `plan show` (`checks`, `review_decision`, `feedback_open`, `head_oid` of its `pull_request`) and the open PR counts by
//! checks with `plan list` (`open_pr_checks`: `{passed, pending, failed}` counts of the Plan's open PRs).
//! The command tree runs in this process on a temporary `$CPD_HOME`; no `gh`.

use crate::common;
use crate::plan_show::{Pay, fx};

use serde_json::json;

/// Workflow `key` of `pay-1`, open, on PR `number` with `checks`, `head`, and two open feedback items.
fn open_pr(fx: &common::fx::Fx, key: &str, number: i64, checks: &str, head: &str) {
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    mergeable, merge_state, head_oid, feedback)
           VALUES ('acme/{key}', {number}, 'https://github.com/acme/{key}/pull/{number}', 'T', 0, 'open', 'changes_requested', '{checks}',
                   't', 't', 'mergeable', 'clean', '{head}',
                   '[{{\"id\":\"a\",\"kind\":\"comment\",\"last\":\"t\",\"author\":\"bob\",\"bot\":false,\"path\":null,\"outdated\":false,\"url\":\"u\",\"excerpt\":\"x\"}},{{\"id\":\"b\",\"kind\":\"comment\",\"last\":\"t\",\"author\":\"bob\",\"bot\":false,\"path\":null,\"outdated\":false,\"url\":\"u\",\"excerpt\":\"x\"}}]');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                                created_txn, created_at, started_at)
           VALUES ('pay-1', '{key}', '/r/{key}', 'main', 'open', 'cpd/pay-1/{key}', '/r/pay.{key}', {number},
                   'https://github.com/acme/{key}/pull/{number}', 't',
                   (SELECT id FROM pull_requests WHERE number = {number}), cpd_txn(), 't', 't');"
    ));
}

#[test]
fn gh_plan_show_lists_pr_state_for_each_workflow() {
    let fx = fx("dispatch");
    open_pr(&fx, "api", 7, "passed", "h7");
    open_pr(&fx, "db", 8, "pending", "h8");
    fx.pr("web", "waiting", &[]);
    let show = fx.show();
    let by_key = |key: &str| {
        show["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["key"] == key)
            .unwrap()
            .clone()
    };
    let state = |key: &str| {
        let p = &by_key(key)["pull_request"];
        json!([
            p["checks"],
            p["review_decision"],
            p["feedback_open"],
            p["head_oid"]
        ])
    };
    assert_eq!(
        state("api"),
        json!(["passed", "changes_requested", 2, "h7"])
    );
    assert_eq!(
        state("db"),
        json!(["pending", "changes_requested", 2, "h8"])
    );
    assert!(by_key("web")["pull_request"].is_null(), "no PR, no state");
}

#[test]
fn gh_plan_list_includes_open_pr_counts_by_checks() {
    let fx = fx("dispatch");
    open_pr(&fx, "api", 7, "passed", "h7");
    open_pr(&fx, "db", 8, "pending", "h8");
    open_pr(&fx, "ui", 9, "failed", "h9");
    open_pr(&fx, "cli", 10, "passed", "h10");
    fx.pr("web", "waiting", &[]);
    let list = fx.list();
    assert_eq!(
        list["plans"][0]["open_pr_checks"],
        json!({"passed": 2, "pending": 1, "failed": 1})
    );
}
