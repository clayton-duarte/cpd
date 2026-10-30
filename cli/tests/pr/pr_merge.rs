//! `pr merge` (commands.md §9.10), run in-process on `run_with` with a stateful `gh` handler that the runner records.
//! The command needs no Plan, no Context and no database row.

use crate::common;

use std::sync::{Arc, Mutex};

use common::fx::Fx;
use common::{TestEnv, refused};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::inbox::model::reconcile;
use cpd_core::inbox::rules::conditions;
use cpd_core::store::Store;
use serde_json::json;

const REF: &str = "acme/api#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// The in-process `gh`: `pr view` answers `before` until `pr merge` ran and `after` from then on, `pr merge` exits
/// `merge_exit` (saying `merge said no` when it fails), `api graphql` (the poll's) answers `graphql`; any other call exits 9.
struct Gh {
    before: String,
    after: String,
    merge_exit: i32,
    graphql: Mutex<Reply>,
    /// What `repo view` answers; `{"deleteBranchOnMerge": true}` unless a test sets it.
    repo: Mutex<Reply>,
    merged: Mutex<bool>,
}

impl Gh {
    fn set_graphql(&self, reply: Reply) {
        *self.graphql.lock().unwrap() = reply;
    }

    fn set_repo(&self, reply: Reply) {
        *self.repo.lock().unwrap() = reply;
    }
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..2] {
            ["pr", "view"] => Reply::ok(&format!("{}\n", if *self.merged.lock().unwrap() { &self.after } else { &self.before })),
            ["pr", "merge"] => {
                *self.merged.lock().unwrap() = true;
                if self.merge_exit == 0 { Reply::ok("") } else { Reply::exit(self.merge_exit, "", "merge said no\n") }
            }
            ["repo", "view"] => self.repo.lock().unwrap().clone(),
            ["api", "graphql"] => self.graphql.lock().unwrap().clone(),
            _ => Reply::exit(9, "", ""),
        }
    }
}

/// Installs a [`Gh`] on `fx`'s runner and returns it, for the test that changes what the poll's `api graphql` says.
fn install(fx: &Fx, before: &str, after: &str, merge_exit: i32, graphql: Reply) -> Arc<Gh> {
    let gh = Arc::new(Gh { before: before.to_owned(), after: after.to_owned(), merge_exit, graphql: Mutex::new(graphql), repo: Mutex::new(Reply::ok("{\"deleteBranchOnMerge\": true}\n")), merged: Mutex::new(false) });
    let _ = (*fx.fakes.runner).clone().route("gh", gh.clone());
    gh
}

/// A `gh` that answers `pr view` with `view`, and `pr merge` with `merge_exit`.
fn gh(fx: &Fx, view: &str, merge_exit: i32) {
    install(fx, view, view, merge_exit, Reply::exit(9, "", ""));
}

/// The argv of every `gh` call so far.
fn calls(fx: &Fx) -> Vec<Vec<String>> {
    fx.calls("gh").into_iter().map(|call| call.args).collect()
}

fn view(state: &str, merge_state: &str) -> String {
    json!({"state": state, "mergeStateStatus": merge_state, "headRefOid": SHA, "title": "Add the thing"}).to_string()
}

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

#[test]
fn a_clean_open_pull_request_is_squash_merged_with_the_head_it_was_checked_at() {
    let fx = Fx::with_ports();
    gh(&fx, &view("OPEN", "CLEAN"), 0);
    let run = fx.cpd(&["pr", "merge", REF]);
    assert_eq!(run.ok(), json!({"ok": true, "repo": "acme/api", "number": 7, "method": "squash", "stored": false, "warnings": []}));
    assert_eq!(
        calls(&fx),
        [
            argv(&["pr", "view", "7", "--repo", "acme/api", "--json", "state,mergeStateStatus,headRefOid,title,mergedAt"]),
            argv(&["pr", "merge", "7", "--repo", "acme/api", "--squash", "--match-head-commit", SHA]),
            argv(&["pr", "view", "7", "--repo", "acme/api", "--json", "state,mergeStateStatus,headRefOid,title,mergedAt"]),
            argv(&["repo", "view", "acme/api", "--json", "deleteBranchOnMerge"]),
        ]
    );
}

#[test]
fn a_pull_request_that_is_not_open_and_clean_is_refused_before_any_merge() {
    for (state, merge_state) in [("OPEN", "BLOCKED"), ("OPEN", "DIRTY"), ("OPEN", "UNSTABLE"), ("MERGED", "CLEAN"), ("MERGED", "UNKNOWN"), ("CLOSED", "CLEAN")] {
        let fx = Fx::with_ports();
        gh(&fx, &view(state, merge_state), 0);
        let run = fx.cpd(&["pr", "merge", REF]);
        let error = refused(&run, "merge_not_clean", 3);
        assert_eq!(error["details"], json!({"repo": "acme/api", "number": 7, "state": state, "merge_state": merge_state}), "{state} {merge_state}");
        let calls = calls(&fx);
        assert_eq!(calls.len(), 1, "{state} {merge_state}: {calls:?}");
        assert_eq!(calls[0][..2], ["pr", "view"], "{state} {merge_state}");
    }
}

#[test]
fn a_failing_pr_view_is_gh_failed() {
    let fx = Fx::with_ports();
    fx.fake_gh(Reply::exit(1, "", "HTTP 502\n"));
    let error = refused(&fx.cpd(&["pr", "merge", REF]), "gh_failed", 1);
    assert_eq!(
        error["details"]["argv"],
        json!(["gh", "pr", "view", "7", "--repo", "acme/api", "--json", "state,mergeStateStatus,headRefOid,title,mergedAt"])
    );
    assert_eq!(error["details"]["exit"], 1);
    assert_eq!(error["details"]["stderr"], "HTTP 502");
    assert_eq!(calls(&fx).len(), 1);
}

#[test]
fn a_pr_view_that_is_not_json_of_the_fields_is_gh_failed() {
    let fx = Fx::with_ports();
    fx.fake_gh(Reply::ok("{\"state\":\"OPEN\"}\n"));
    refused(&fx.cpd(&["pr", "merge", REF]), "gh_failed", 1);
    assert_eq!(calls(&fx).len(), 1);
}

#[test]
fn a_failing_pr_merge_is_gh_failed() {
    let fx = Fx::with_ports();
    gh(&fx, &view("OPEN", "CLEAN"), 1);
    let error = refused(&fx.cpd(&["pr", "merge", REF]), "gh_failed", 1);
    assert_eq!(
        error["details"]["argv"],
        json!(["gh", "pr", "merge", "7", "--repo", "acme/api", "--squash", "--match-head-commit", SHA])
    );
    assert_eq!(error["details"]["exit"], 1);
    assert_eq!(error["details"]["stderr"], "merge said no");
    assert_eq!(calls(&fx).len(), 2);
}

#[test]
fn a_merge_into_a_repo_with_auto_delete_off_warns_that_the_branch_stays() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    let gh = gh_read_back(&fx, &view_merged_at("OPEN", "CLEAN", None), &view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT)), Reply::exit(1, "", ""));
    gh.set_repo(Reply::ok("{\"deleteBranchOnMerge\": false}\n"));
    let out = fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["stored"], true, "{out}");
    let warnings = out["warnings"].as_array().unwrap_or_else(|| panic!("a warnings array: {out}"));
    assert_eq!(warnings.len(), 1, "{out}");
    assert_eq!(warnings[0]["code"], "auto_delete_off", "{out}");
    let text = warnings[0]["text"].as_str().unwrap();
    assert!(text.contains("acme/api") && text.contains("delete_branch_on_merge"), "{text}");
}

#[test]
fn a_merge_whose_repo_setting_cannot_be_read_warns_and_still_succeeds() {
    for repo in [Reply::exit(1, "", "HTTP 502\n"), Reply::ok("not json\n")] {
        let fx = Fx::with_ports();
        let env = &fx.env;
        row_with_ready_item(env);
        let gh = gh_read_back(&fx, &view_merged_at("OPEN", "CLEAN", None), &view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT)), Reply::exit(1, "", ""));
        gh.set_repo(repo);
        let out = fx.cpd(&["pr", "merge", REF]).ok();
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(out["stored"], true, "the store write is the same as without the failure: {out}");
        let warnings = out["warnings"].as_array().unwrap_or_else(|| panic!("a warnings array: {out}"));
        assert_eq!(warnings.len(), 1, "{out}");
        assert_eq!(warnings[0]["code"], "auto_delete_unread", "{out}");
        assert_stored_merged(env, Some(MERGED_AT));
    }
}

#[test]
fn a_bad_argument_is_usage_and_gh_is_not_called() {
    for bad in ["foo", "acme/api#x", "acme/api#0", "acme/api", "#7", "acme/#7", "/api#7", "a/b/c#7"] {
        let fx = Fx::with_ports();
        gh(&fx, &view("OPEN", "CLEAN"), 0);
        refused(&fx.cpd(&["pr", "merge", bad]), "usage", 2);
        assert!(calls(&fx).is_empty(), "{bad}");
    }
    let fx = Fx::with_ports();
    refused(&fx.cpd(&["pr", "merge"]), "usage", 2);
}

const SUBJECT: &str = "pr:acme/api#7";
const MERGED_AT: &str = "2026-10-21T15:01:30Z";

/// A `gh` that answers `pr view` with `view`, `pr merge` with exit 0, and every `api graphql` call (the poll's) with a failure.
fn gh_failing_poll(fx: &Fx, view: &str) {
    install(fx, view, view, 0, Reply::exit(1, "", ""));
}

fn view_merged_at(state: &str, merge_state: &str, merged_at: Option<&str>) -> String {
    json!({"state": state, "mergeStateStatus": merge_state, "headRefOid": SHA, "title": "Add the thing", "mergedAt": merged_at}).to_string()
}

/// A `gh` whose `pr view` answers `before` until `pr merge` ran and `after` from then on; `api graphql` answers `graphql`.
fn gh_read_back(fx: &Fx, before: &str, after: &str, graphql: Reply) -> Arc<Gh> {
    install(fx, before, after, 0, graphql)
}

/// The GraphQL search answer listing acme/api#7 with `state`, mergeable UNKNOWN, mergeStateStatus CLEAN.
fn search_listing(state: &str) -> Reply {
    let node = format!(
        r#"{{"repository":{{"nameWithOwner":"acme/api","defaultBranchRef":{{"name":"main"}}}},"number":7,"url":"https://github.com/acme/api/pull/7","title":"Add the thing","isDraft":false,"state":"{state}","createdAt":"t","updatedAt":"U1","mergedAt":null,"baseRefName":"main","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"CLEAN","commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    );
    let search = format!(
        r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-22T11:00:00Z"}}}}}}"#
    );
    Reply::ok(&search)
}

/// A stored open, clean, authored pull request row of acme/api#7, with an open pr-ready item opened as E8 does.
fn row_with_ready_item(env: &TestEnv) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    env.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merge_state, authored, base_ref, default_branch)
         VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add the thing', 0, 'open', 'none', 'none', 't', 't', 'clean', 1, 'main', 'main')",
    );
    let mut store = Store::open(&home, 5000).unwrap();
    let found = conditions(&mut store, &home).unwrap();
    assert_eq!(found.iter().filter(|c| c.subject == SUBJECT).count(), 1, "{found:?}");
    reconcile(&mut store, &found, "2026-10-21T15:00:00.000Z").unwrap();
    assert_eq!(env.query::<Option<String>>(&item("closed_at")), None);
}

fn item(column: &str) -> String {
    format!("SELECT {column} FROM inbox_items WHERE kind = 'pr-ready' AND subject = '{SUBJECT}'")
}

/// The row is merged, the item is closed, both at the same transaction's time.
fn assert_stored_merged(env: &TestEnv, merged_at: Option<&str>) {
    assert_eq!(env.query::<String>("SELECT state FROM pull_requests WHERE number = 7"), "merged");
    let stored = env.query::<Option<String>>("SELECT merged_at FROM pull_requests WHERE number = 7");
    assert!(stored.is_some(), "merged_at is set");
    if let Some(at) = merged_at {
        assert_eq!(stored.as_deref(), Some(at));
    }
    let closed = env.query::<Option<String>>(&item("closed_at"));
    assert!(closed.is_some(), "the pr-ready item is closed");
    assert_eq!(closed, Some(env.query::<String>("SELECT changed_at FROM pull_requests WHERE number = 7")), "one transaction's time");
}

#[test]
fn a_successful_merge_stores_the_merge_and_closes_its_pr_ready_item_when_the_poll_after_it_fails_hs_d526() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    gh_read_back(&fx, &view_merged_at("OPEN", "CLEAN", None), &view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT)), Reply::exit(1, "", ""));
    fx.cpd(&["pr", "merge", REF]).ok();
    assert_ne!(fx.cpd(&["pr", "poll"]).exit, 0, "the poll fails");
    assert_stored_merged(env, None);
}

#[test]
fn a_precheck_that_reads_merged_stores_the_merge_and_closes_its_pr_ready_item_hs_d526() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    gh_failing_poll(&fx, &view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT)));
    let error = refused(&fx.cpd(&["pr", "merge", REF]), "merge_not_clean", 3);
    assert_eq!(error["details"]["state"], "MERGED");
    assert!(calls(&fx).iter().all(|c| c[..2] != ["pr", "merge"]), "{:?}", calls(&fx));
    assert_stored_merged(env, Some(MERGED_AT));
}

#[test]
fn a_merge_of_an_owned_pull_request_records_the_prs_merge_hs_d526() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'deploy', 'lead1', '1999-12-31T00:00:00.000Z', '2000-01-01T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                          pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay-1', 'api', '/r', 'main', 'api', 'open', 'cpd/pay-1/api', '/w', cpd_txn(), 't', 't',
                   7, 'https://github.com/acme/api/pull/7', 't', (SELECT id FROM pull_requests WHERE number = 7));",
    );
    gh_read_back(&fx, &view_merged_at("OPEN", "CLEAN", None), &view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT)), Reply::exit(1, "", ""));
    fx.cpd(&["pr", "merge", REF]).ok();
    assert_ne!(fx.cpd(&["pr", "poll"]).exit, 0, "the poll fails");
    assert_eq!(env.query::<String>("SELECT status FROM prs WHERE key = 'api'"), "merged");
    let events = fx.cpd(&["events", "list", "--kind", "pr-merged", "--plan", "pay-1"]).ok();
    assert_eq!(events["events"].as_array().unwrap().len(), 1, "{events}");
}

#[test]
fn a_poll_that_finds_the_pull_request_merged_closes_its_pr_ready_item_in_the_same_transaction_hs_n404() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    let node = format!(
        r#"{{"repository":{{"nameWithOwner":"acme/api"}},"number":7,"url":"https://github.com/acme/api/pull/7","title":"Add the thing","isDraft":false,"state":"MERGED","createdAt":"t","updatedAt":"U1","mergedAt":"{MERGED_AT}","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    );
    let search = format!(
        r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-22T11:00:00Z"}}}}}}"#
    );
    fx.fake_gh(Reply::ok(&search));
    fx.cpd(&["pr", "poll"]).ok();
    assert_stored_merged(env, None);
}

#[test]
fn a_poll_whose_search_still_lists_a_stored_merge_open_leaves_it_merged_and_e8_opens_no_pr_ready_hs_d550() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    let open = view_merged_at("OPEN", "CLEAN", None);
    let merged = view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT));
    let gh = gh_read_back(&fx, &open, &merged, Reply::exit(1, "", ""));
    fx.cpd(&["pr", "merge", REF]).ok();
    gh.set_graphql(search_listing("OPEN"));
    fx.cpd(&["pr", "poll"]).ok();
    assert_eq!(env.query::<String>("SELECT state FROM pull_requests WHERE number = 7"), "merged");
    assert_eq!(env.query::<Option<String>>("SELECT merged_at FROM pull_requests WHERE number = 7").as_deref(), Some(MERGED_AT));
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let found = conditions(&mut store, &home).unwrap();
    reconcile(&mut store, &found, "2026-10-21T15:00:05.000Z").unwrap();
    assert_eq!(
        env.query::<i64>(&format!("SELECT count(*) FROM inbox_items WHERE kind = 'pr-ready' AND subject = '{SUBJECT}' AND closed_at IS NULL")),
        0,
        "no pr-ready item is open"
    );
}

#[test]
fn a_merge_whose_read_back_still_reads_open_stores_nothing_hs_d550() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    let open = view_merged_at("OPEN", "CLEAN", None);
    gh_read_back(&fx, &open, &open, Reply::exit(1, "", ""));
    let out = fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(out["stored"], false, "{out}");
    assert_eq!(env.query::<String>("SELECT state FROM pull_requests WHERE number = 7"), "open");
    assert_eq!(env.query::<Option<String>>(&item("closed_at")), None);
}

#[test]
fn a_merge_stores_the_merged_at_its_read_back_reads_hs_d550() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    gh_read_back(&fx, &view_merged_at("OPEN", "CLEAN", None), &view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT)), Reply::exit(1, "", ""));
    let out = fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(out["stored"], true, "{out}");
    assert_stored_merged(env, Some(MERGED_AT));
}

#[test]
fn a_poll_that_lists_a_stored_merge_open_opens_no_write_transaction_hs_d550() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    row_with_ready_item(env);
    let open = view_merged_at("OPEN", "CLEAN", None);
    let merged = view_merged_at("MERGED", "UNKNOWN", Some(MERGED_AT));
    let gh = gh_read_back(&fx, &open, &merged, Reply::exit(1, "", ""));
    fx.cpd(&["pr", "merge", REF]).ok();
    gh.set_graphql(search_listing("OPEN"));
    let row = "SELECT state || '|' || ifnull(merged_at, '') || '|' || changed_at || '|' || merge_state || '|' || review_decision FROM pull_requests WHERE number = 7";
    let (txns, before) = (env.query::<i64>("SELECT count(*) FROM txns"), env.query::<String>(row));
    fx.cpd(&["pr", "poll"]).ok();
    assert_eq!(env.query::<i64>("SELECT count(*) FROM txns"), txns, "no write transaction");
    assert_eq!(env.query::<String>(row), before);
}

/// The `pr view` answer of a clean open pull request whose head's rollup is `rollup`.
fn view_with_rollup(rollup: serde_json::Value) -> String {
    json!({"state": "OPEN", "mergeStateStatus": "CLEAN", "headRefOid": SHA, "title": "Add the thing", "statusCheckRollup": rollup}).to_string()
}

fn guardian() -> serde_json::Value {
    json!({"__typename": "StatusContext", "context": "GitGuardian Security Checks", "state": "SUCCESS"})
}

fn gate(status: &str, conclusion: serde_json::Value) -> serde_json::Value {
    json!({"__typename": "CheckRun", "name": "gate", "status": status, "conclusion": conclusion})
}

const REQUIRED: &str = "[pr]\nrequired_checks = [\"gate\"]\n";

fn merged(fx: &Fx) -> bool {
    calls(fx).iter().any(|c| c[..2] == ["pr", "merge"])
}

/// Merges `acme/api#7` under the repo policy `policy` whose head's rollup is `rollup`; the refusal's details when it is refused.
fn refused_with(rollup: serde_json::Value, policy: &str) -> (serde_json::Value, Fx) {
    let fx = Fx::with_ports();
    fx.env.repo_policy("acme", "api", policy);
    gh(&fx, &view_with_rollup(rollup), 0);
    let error = refused(&fx.cpd(&["pr", "merge", REF]), "required_check_not_passed", 3);
    (error["details"].clone(), fx)
}

#[test]
fn merge_refuses_a_clean_pr_whose_required_check_is_missing() {
    let (details, fx) = refused_with(json!([guardian()]), REQUIRED);
    assert_eq!(details, json!({"repo": "acme/api", "number": 7, "missing": ["gate"], "pending": [], "failed": []}));
    assert!(!merged(&fx), "{:?}", calls(&fx));
}

#[test]
fn merge_refuses_a_clean_pr_whose_required_check_is_pending() {
    let (details, fx) = refused_with(json!([guardian(), gate("IN_PROGRESS", json!(null))]), REQUIRED);
    assert_eq!(details, json!({"repo": "acme/api", "number": 7, "missing": [], "pending": ["gate"], "failed": []}));
    assert!(!merged(&fx), "{:?}", calls(&fx));
}

#[test]
fn merge_refuses_a_clean_pr_whose_required_check_failed() {
    let (details, fx) = refused_with(json!([guardian(), gate("COMPLETED", json!("FAILURE"))]), REQUIRED);
    assert_eq!(details, json!({"repo": "acme/api", "number": 7, "missing": [], "pending": [], "failed": ["gate"]}));
    assert!(!merged(&fx), "{:?}", calls(&fx));
}

#[test]
fn merge_proceeds_when_the_required_check_passed() {
    let fx = Fx::with_ports();
    fx.env.repo_policy("acme", "api", REQUIRED);
    gh(&fx, &view_with_rollup(json!([guardian(), gate("COMPLETED", json!("SUCCESS"))])), 0);
    assert_eq!(fx.cpd(&["pr", "merge", REF]).ok()["ok"], true);
    assert!(merged(&fx));
}

#[test]
fn merge_with_an_empty_required_list_ignores_the_head_checks() {
    for policy in ["[pr]\nrequired_checks = []\n", ""] {
        let fx = Fx::with_ports();
        if !policy.is_empty() {
            fx.env.repo_policy("acme", "api", policy);
        }
        gh(&fx, &view_with_rollup(json!([guardian()])), 0);
        assert_eq!(fx.cpd(&["pr", "merge", REF]).ok()["ok"], true, "{policy:?}");
        assert!(merged(&fx), "{policy:?}");
    }
}
