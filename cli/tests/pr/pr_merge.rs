//! `pr merge` (commands.md §9.10), run as the built binary with a fake `gh` that records its argv.
//! The command needs no Plan, no Context and no database row.

use crate::common;

use common::{TestEnv, refused};
use serde_json::json;

const REF: &str = "acme/api#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A `gh` that answers `pr view` with `view`, and `pr merge` with `merge_exit`.
fn gh(env: &TestEnv, view: &str, merge_exit: i32) {
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\"pr view\") echo '{view}'; exit 0;;\n\"pr merge\") echo 'merge said no' >&2; exit {merge_exit};;\nesac\nexit 9"
    ));
}

fn view(state: &str, merge_state: &str) -> String {
    json!({"state": state, "mergeStateStatus": merge_state, "headRefOid": SHA, "title": "Add the thing"}).to_string()
}

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

#[test]
fn a_clean_open_pull_request_is_squash_merged_with_the_head_it_was_checked_at() {
    let env = TestEnv::new();
    gh(&env, &view("OPEN", "CLEAN"), 0);
    let run = env.cpd(&["pr", "merge", REF]);
    assert_eq!(run.ok(), json!({"ok": true, "repo": "acme/api", "number": 7, "method": "squash", "branch_deleted": true}));
    assert_eq!(
        env.calls("gh"),
        [
            argv(&["pr", "view", "7", "--repo", "acme/api", "--json", "state,mergeStateStatus,headRefOid,title"]),
            argv(&["pr", "merge", "7", "--repo", "acme/api", "--squash", "--delete-branch", "--match-head-commit", SHA]),
        ]
    );
}

#[test]
fn a_pull_request_that_is_not_open_and_clean_is_refused_before_any_merge() {
    for (state, merge_state) in [("OPEN", "BLOCKED"), ("OPEN", "DIRTY"), ("OPEN", "UNSTABLE"), ("MERGED", "CLEAN"), ("MERGED", "UNKNOWN"), ("CLOSED", "CLEAN")] {
        let env = TestEnv::new();
        gh(&env, &view(state, merge_state), 0);
        let run = env.cpd(&["pr", "merge", REF]);
        let error = refused(&run, "merge_not_clean", 3);
        assert_eq!(error["details"], json!({"repo": "acme/api", "number": 7, "state": state, "merge_state": merge_state}), "{state} {merge_state}");
        let calls = env.calls("gh");
        assert_eq!(calls.len(), 1, "{state} {merge_state}: {calls:?}");
        assert_eq!(calls[0][..2], ["pr", "view"], "{state} {merge_state}");
    }
}

#[test]
fn a_failing_pr_view_is_gh_failed() {
    let env = TestEnv::new();
    env.fake_gh("echo 'HTTP 502' >&2; exit 1");
    let error = refused(&env.cpd(&["pr", "merge", REF]), "gh_failed", 1);
    assert_eq!(
        error["details"]["argv"],
        json!(["gh", "pr", "view", "7", "--repo", "acme/api", "--json", "state,mergeStateStatus,headRefOid,title"])
    );
    assert_eq!(error["details"]["exit"], 1);
    assert_eq!(error["details"]["stderr"], "HTTP 502");
    assert_eq!(env.calls("gh").len(), 1);
}

#[test]
fn a_pr_view_that_is_not_json_of_the_fields_is_gh_failed() {
    let env = TestEnv::new();
    env.fake_gh("echo '{\"state\":\"OPEN\"}'");
    refused(&env.cpd(&["pr", "merge", REF]), "gh_failed", 1);
    assert_eq!(env.calls("gh").len(), 1);
}

#[test]
fn a_failing_pr_merge_is_gh_failed() {
    let env = TestEnv::new();
    gh(&env, &view("OPEN", "CLEAN"), 1);
    let error = refused(&env.cpd(&["pr", "merge", REF]), "gh_failed", 1);
    assert_eq!(
        error["details"]["argv"],
        json!(["gh", "pr", "merge", "7", "--repo", "acme/api", "--squash", "--delete-branch", "--match-head-commit", SHA])
    );
    assert_eq!(error["details"]["exit"], 1);
    assert_eq!(error["details"]["stderr"], "merge said no");
    assert_eq!(env.calls("gh").len(), 2);
}

#[test]
fn a_bad_argument_is_usage_and_gh_is_not_called() {
    for bad in ["foo", "acme/api#x", "acme/api#0", "acme/api", "#7", "acme/#7", "/api#7", "a/b/c#7"] {
        let env = TestEnv::new();
        gh(&env, &view("OPEN", "CLEAN"), 0);
        refused(&env.cpd(&["pr", "merge", bad]), "usage", 2);
        assert!(env.calls("gh").is_empty(), "{bad}");
    }
    let env = TestEnv::new();
    refused(&env.cpd(&["pr", "merge"]), "usage", 2);
}
