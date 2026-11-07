//! `pr merge` (commands.md §9.10, swr-github.md §9), run in-process on `run_with`: it checks the cached pull request and queues a
//! `merge` mutation; it never calls `gh` (the drain does, in `core/src/swr/source/github_write_tests.rs`). The command needs no Plan
//! and no Context.

use crate::common;

use common::fx::Fx;
use common::{TestEnv, refused};
use cpd_core::home::CpdHome;
use serde_json::{Value, json};

const REF: &str = "acme/api#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A cached open, clean, authored pull request `acme/api#7` whose head is `SHA`, and the repo's settings allowing `methods`
/// (squash, merge commit, rebase).
fn cached(env: &TestEnv, state: &str, merge_state: &str, methods: (bool, bool, bool)) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let merged_at = if state == "merged" { "'t'" } else { "NULL" };
    env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    merged_at, merge_state, authored, base_ref, default_branch, head_oid)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add the thing', 0, '{state}', 'none', 'passed', 't', 't',
                   {merged_at}, '{merge_state}', 1, 'main', 'main', '{SHA}');
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed, merge_commit_allowed,
                                rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/api', 'main', 0, 1, {}, {}, {}, 1, 't');",
        i32::from(methods.0),
        i32::from(methods.1),
        i32::from(methods.2)
    ));
}

fn gh_calls(fx: &Fx) -> usize {
    fx.calls("gh").len()
}

fn mutations(env: &TestEnv) -> Vec<(String, String, String, Value, Value)> {
    env.read(|c| {
        let mut stmt = c.prepare("SELECT target_key, op_kind, status, vars_json, patch_json FROM swr_mutations ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    serde_json::from_str(&r.get::<_, String>(3)?).unwrap(),
                    serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

#[test]
fn a_clean_open_pull_request_is_queued_with_the_head_it_was_cached_at() {
    let fx = Fx::with_ports();
    cached(&fx.env, "open", "clean", (true, false, false));
    let out = fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(out["queued"], true, "{out}");
    assert!(out["mutation_id"].is_i64(), "{out}");
    assert_eq!(
        (
            out["repo"].as_str(),
            out["number"].as_i64(),
            out["method"].as_str()
        ),
        (Some("acme/api"), Some(7), Some("squash"))
    );
    assert_eq!(out["effective"]["state"], "merging", "{out}");
    assert_eq!(
        mutations(&fx.env),
        [(
            "github:pr:acme/api#7".to_owned(),
            "merge".to_owned(),
            "pending".to_owned(),
            json!({"expected_head_oid": SHA, "method": "squash"}),
            json!({"state": "merging"})
        )]
    );
    assert_eq!(gh_calls(&fx), 0, "a command never calls GitHub for a write");
}

#[test]
fn the_first_method_the_repo_allows_is_the_one_queued() {
    for (methods, want) in [
        ((true, true, true), "squash"),
        ((false, true, true), "merge"),
        ((false, false, true), "rebase"),
    ] {
        let fx = Fx::with_ports();
        cached(&fx.env, "open", "clean", methods);
        assert_eq!(
            fx.cpd(&["pr", "merge", REF]).ok()["method"],
            want,
            "{methods:?}"
        );
        assert_eq!(mutations(&fx.env)[0].3["method"], want);
    }
}

#[test]
fn a_repo_that_allows_no_merge_method_is_refused_before_any_mutation() {
    let fx = Fx::with_ports();
    cached(&fx.env, "open", "clean", (false, false, false));
    let error = refused(&fx.cpd(&["pr", "merge", REF]), "no_merge_method", 3);
    assert_eq!(
        error["details"],
        json!({"repo": "acme/api", "number": 7, "squash": false, "merge": false, "rebase": false})
    );
    assert!(mutations(&fx.env).is_empty());
}

#[test]
fn a_pull_request_that_is_not_open_and_clean_is_refused_before_any_mutation() {
    for (state, merge_state) in [
        ("open", "blocked"),
        ("open", "dirty"),
        ("open", "unstable"),
        ("merged", "clean"),
        ("closed", "clean"),
    ] {
        let fx = Fx::with_ports();
        cached(&fx.env, state, merge_state, (true, false, false));
        let error = refused(&fx.cpd(&["pr", "merge", REF]), "merge_not_clean", 3);
        assert_eq!(
            error["details"],
            json!({"repo": "acme/api", "number": 7, "state": state, "merge_state": merge_state}),
            "{state} {merge_state}"
        );
        assert!(mutations(&fx.env).is_empty(), "{state} {merge_state}");
        assert_eq!(gh_calls(&fx), 0);
    }
}

#[test]
fn a_pull_request_the_poll_has_not_read_is_not_found_and_nothing_is_queued() {
    let fx = Fx::with_ports();
    cached(&fx.env, "open", "clean", (true, false, false));
    fx.env.sql("DELETE FROM pull_requests");
    refused(&fx.cpd(&["pr", "merge", REF]), "item_not_found", 4);
    assert!(mutations(&fx.env).is_empty());
}

#[test]
fn a_second_merge_of_the_same_head_queues_no_second_mutation() {
    let fx = Fx::with_ports();
    cached(&fx.env, "open", "clean", (true, false, false));
    let first = fx.cpd(&["pr", "merge", REF]).ok();
    let second = fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(first["mutation_id"], second["mutation_id"]);
    assert_eq!(mutations(&fx.env).len(), 1);
}

/// The head's rollup as the pass stored it in `swr_pr_detail`.
fn rollup(fx: &Fx, nodes: Value) {
    let detail = json!({"commits": {"nodes": [{"commit": {"oid": SHA, "statusCheckRollup": {"state": "SUCCESS", "contexts": {"nodes": nodes}}}}]}});
    fx.env.sql(&format!(
        "INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
           VALUES ('acme/api', 7, '{SHA}', 'U', 't', '{detail}', 'h')"
    ));
}

fn gate(status: &str, conclusion: Value) -> Value {
    json!({"__typename": "CheckRun", "name": "gate", "status": status, "conclusion": conclusion})
}

const REQUIRED: &str = "[pr]\nrequired_checks = [\"gate\"]\n";

#[test]
fn a_required_check_that_is_missing_pending_or_failed_refuses_the_merge() {
    // (the persisted readiness verdict, the head's cached rollup nodes, the details to check)
    for (verdict, nodes, want) in [
        (
            "pending",
            json!([]),
            json!({"missing": ["gate"], "pending": [], "failed": []}),
        ),
        (
            "pending",
            json!([gate("IN_PROGRESS", json!(null))]),
            json!({"missing": [], "pending": ["gate"], "failed": []}),
        ),
        (
            "failed",
            json!([gate("COMPLETED", json!("FAILURE"))]),
            json!({"missing": [], "pending": [], "failed": ["gate"]}),
        ),
    ] {
        let fx = Fx::with_ports();
        fx.env.repo_policy("acme", "api", REQUIRED);
        cached(&fx.env, "open", "clean", (true, false, false));
        readiness(&fx.env, verdict);
        rollup(&fx, nodes);
        let error = refused(
            &fx.cpd(&["pr", "merge", REF]),
            "required_check_not_passed",
            3,
        );
        for key in ["missing", "pending", "failed"] {
            assert_eq!(error["details"][key], want[key], "{key}");
        }
        assert!(mutations(&fx.env).is_empty());
    }
}

#[test]
fn a_passed_required_check_or_an_empty_required_list_queues_the_merge() {
    for (policy, nodes) in [
        (REQUIRED, json!([gate("COMPLETED", json!("SUCCESS"))])),
        ("[pr]\nrequired_checks = []\n", json!([])),
        ("", json!([])),
    ] {
        let fx = Fx::with_ports();
        if !policy.is_empty() {
            fx.env.repo_policy("acme", "api", policy);
        }
        cached(&fx.env, "open", "clean", (true, false, false));
        rollup(&fx, nodes);
        assert_eq!(
            fx.cpd(&["pr", "merge", REF]).ok()["queued"],
            true,
            "{policy:?}"
        );
    }
}

/// The pull request's persisted readiness verdict (`pull_requests.checks`), as the poll's live read stored it: the one
/// record the Inbox pr-ready readiness and (air-d48) `pr merge` both read. It is independent of the `swr_pr_detail`
/// rollup the tests write with [`rollup`], so a cache that lagged the poll cannot decide the merge.
fn readiness(env: &TestEnv, verdict: &str) {
    env.sql(&format!(
        "UPDATE pull_requests SET checks = '{verdict}' WHERE repo = 'acme/api' AND number = 7"
    ));
}

/// Regression, air-n24: the poll's live read stored readiness `passed`, but the `swr_pr_detail` cache still holds the
/// head's rollup from before the required `gate` existed (the stale #940 state). The required-check gate reads the
/// persisted readiness verdict, not the cache, so the merge queues instead of refusing `required_check_not_passed`.
/// On the buggy tree the refusal recomputes from the stale detail and refuses `missing: ["gate"]`.
#[test]
fn pr_merge_allows_when_readiness_passed_but_cached_detail_stale() {
    let fx = Fx::with_ports();
    fx.env.repo_policy("acme", "api", REQUIRED);
    cached(&fx.env, "open", "clean", (true, false, false));
    assert_eq!(
        fx.env.query::<String>(
            "SELECT checks FROM pull_requests WHERE repo = 'acme/api' AND number = 7"
        ),
        "passed"
    );
    rollup(&fx, json!([])); // the stale cache: no `gate` node at all
    let out = fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(out["effective"]["checks"], "passed", "{out}");
    assert_eq!(mutations(&fx.env).len(), 1, "the merge was queued");
}

/// The guard: a persisted readiness verdict that is not passed still refuses `required_check_not_passed`, whatever the
/// cached `swr_pr_detail` says, and the refusal names the real missing/pending/failed state. The last case is the
/// inverse of the regression: the cache says `gate` passed, yet the persisted verdict is not passed, so the merge must
/// refuse (on the buggy tree the cache wins and the merge is queued).
#[test]
fn pr_merge_refuses_when_readiness_not_passed() {
    // (persisted verdict, the head's cached rollup nodes, the details to check; `None`: only the code and exit)
    let cases: [(&str, Value, Option<Value>); 4] = [
        ("pending", json!([]), None),
        (
            "pending",
            json!([gate("IN_PROGRESS", json!(null))]),
            Some(json!({"missing": [], "pending": ["gate"], "failed": []})),
        ),
        // stale cache: it says `gate` passed, the persisted verdict says not passed
        (
            "pending",
            json!([gate("COMPLETED", json!("SUCCESS"))]),
            None,
        ),
        (
            "failed",
            json!([gate("COMPLETED", json!("FAILURE"))]),
            Some(json!({"missing": [], "pending": [], "failed": ["gate"]})),
        ),
    ];
    for (verdict, nodes, want) in cases {
        let fx = Fx::with_ports();
        fx.env.repo_policy("acme", "api", REQUIRED);
        cached(&fx.env, "open", "clean", (true, false, false));
        readiness(&fx.env, verdict);
        rollup(&fx, nodes);
        let error = refused(
            &fx.cpd(&["pr", "merge", REF]),
            "required_check_not_passed",
            3,
        );
        if let Some(want) = want {
            for key in ["missing", "pending", "failed"] {
                assert_eq!(error["details"][key], want[key], "verdict {verdict} {key}");
            }
        }
        assert!(mutations(&fx.env).is_empty(), "verdict {verdict}");
    }
}

#[test]
fn a_bad_argument_is_usage_and_nothing_is_queued() {
    for bad in [
        "foo",
        "acme/api#x",
        "acme/api#0",
        "acme/api",
        "#7",
        "acme/#7",
        "/api#7",
        "a/b/c#7",
    ] {
        let fx = Fx::with_ports();
        refused(&fx.cpd(&["pr", "merge", bad]), "usage", 2);
        assert_eq!(gh_calls(&fx), 0, "{bad}");
    }
    let fx = Fx::with_ports();
    refused(&fx.cpd(&["pr", "merge"]), "usage", 2);
}
