//! The `touches` check of a PR stacked on a base PR (commands.md §5.1, §5.2): it reads nothing, and passes, while the
//! base PR has not opened (`waiting` or `dispatch`); once the base PR is `open` it runs against the base PR's Lead Branch.

use std::path::PathBuf;

use serde_json::json;

use super::job::{count, fixture, job, job_row, spec_json, touches_fx, write_spec};
use crate::common::fx::Fx;
use crate::common::refused;

/// The `touches_fx` Plan `pl`, whose PR `repo` (Lead Branch `cpd/pl`) is in `base_status`, and a waiting PR `web` of the
/// same repo stacked on it (`base` = `cpd/pl`, `lead_worktree` null). When `branch` the Lead Branch exists in the repo.
fn stacked_fx(base_status: &str, branch: bool) -> (Fx, PathBuf) {
    let fx = touches_fx();
    let repo = fx.git_repo("pay");
    if branch {
        fx.git(&repo, &["branch", "cpd/pl", "main"]);
    }
    let set = match base_status {
        "waiting" => "",
        "dispatch" => ", lead_worktree = '/wt', started_at = 't'",
        "open" => {
            ", lead_worktree = '/wt', started_at = 't', pr_number = 1, pr_url = 'https://github.com/acme/pl/pull/1', pr_opened_at = 't', pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/pl')"
        }
        other => panic!("{other}"),
    };
    let mut sql = String::new();
    if base_status == "open" {
        sql += &super::job::pull_request("pl", 1, "open");
    }
    if base_status != "waiting" {
        sql += "UPDATE workflows SET status = 'dispatch', lead_worktree = '/wt', started_at = 't' WHERE plan = 'pl' AND key = 'repo';";
    }
    sql += &format!("UPDATE workflows SET status = '{base_status}'{set} WHERE plan = 'pl' AND key = 'repo';");
    sql += "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
            SELECT 'pl', 'web', repo, 'cpd/pl', 'waiting', 'pl/web', cpd_txn(), 't' FROM workflows WHERE plan = 'pl' AND key = 'repo';";
    fixture(&fx.env, &sql);
    (fx, repo)
}

fn add_into(fx: &Fx, pr: &str, key: &str, touches: &[&str]) -> crate::common::Run {
    let mut spec = spec_json();
    spec["touches"] = json!(touches);
    let spec = write_spec(&fx.env, &format!("{key}.json"), &spec.to_string());
    fx.cpd(&["job", "add", key, "--title", &format!("Title of {key}"), "--tier", "S2", "--spec", &spec, "--workflow", pr, "--plan", "pl"])
}

#[test]
fn job_add_skips_the_touches_check_for_a_pr_stacked_on_a_base_pr_that_has_not_opened() {
    // The base PR is waiting and its Lead Branch does not exist.
    let (fx, _) = stacked_fx("waiting", false);
    add_into(&fx, "web", "w1", &["nope/x.rs"]).ok();
    assert_eq!(job_row(&fx.env, "pl", "w1", "touches"), "[\"nope/x.rs\"]");
    // The check of an unstacked PR is still on.
    refused(
        &add_into(&fx, "repo", "r1", &["nope/x.rs"]),
        "touches_missing",
        3,
    );
    drop(fx);

    // The base PR is dispatch and its Lead Branch exists, without the path.
    let (fx, _) = stacked_fx("dispatch", true);
    add_into(&fx, "web", "w2", &["nope/x.rs"]).ok();
    assert_eq!(job_row(&fx.env, "pl", "w2", "touches"), "[\"nope/x.rs\"]");
}

#[test]
fn job_edit_skips_the_touches_check_for_a_pr_stacked_on_a_base_pr_that_has_not_opened() {
    let (fx, _) = stacked_fx("waiting", false);
    fx.cpd(&["job", "add", "k", "--draft", "--title", "Title of k", "--workflow", "web", "--plan", "pl"]).ok();
    let mut spec = spec_json();
    spec["touches"] = json!(["nope/x.rs"]);
    let spec = write_spec(&fx.env, "edit.json", &spec.to_string());
    job(&fx, &["edit", "k", "--tier", "S2", "--spec", &spec], "pl").ok();
    assert_eq!(job_row(&fx.env, "pl", "k", "touches"), "[\"nope/x.rs\"]");
}

#[test]
fn job_add_checks_the_touches_of_a_pr_stacked_on_an_open_base_pr_against_its_branch() {
    let (fx, _) = stacked_fx("open", true);
    let (txns, evs, jobs) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );
    let error = refused(
        &add_into(&fx, "web", "w1", &["nope/x.rs"]),
        "touches_missing",
        3,
    );
    assert_eq!(error["details"]["tree"], json!("cpd/pl"), "{error}");
    assert_eq!(error["details"]["missing"], json!(["nope/x.rs"]), "{error}");
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, jobs)
    );
    add_into(&fx, "web", "w2", &["src/a.rs", "src/new.rs"]).ok();
}
