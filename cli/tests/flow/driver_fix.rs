//! A feedback fix: a `work` Job run against an `open` PR is pushed by its own merge (pro-d68), with no `update` Job and no PR Gate. An
//! adopted PR is pushed the same way once it is `open`, and not before. The harness is `driver_open`'s. Written by Job fix-path-tests.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpd_core::host::runner::Reply;

use super::driver::PLAN;
use super::driver_open::{Live, open_live, open_pr_live, submit};

/// Adds the `work` Job `key` to PR `repo` of the live Plan.
fn add_fix(live: &Live, key: &str) {
    let spec = live.fx.env.root().join("fix-spec.json");
    fs::write(&spec, r#"{"goal": "Fix it.", "context": "Review feedback.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    let out = live.fx.cpd(&["job", "add", key, "--title", "Fix", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--workflow", "repo", "--plan", PLAN]).ok();
    assert_eq!(out["job"]["status"], "queued", "{out}");
}

/// Runs the fix Job `key` to its merge: the Worker commits `<key>.txt` and submits. Returns the commit.
fn run_fix(live: &Live, key: &str) -> String {
    let (attempt, (commit, submitted)) = live.run_job(key, |live, id| {
        let commit = live.commit(&live.worktree(id), &format!("{key}.txt"), "fixed\n", key);
        (commit, submit(live, id))
    });
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&serde_json::json!("merged"), &serde_json::json!("merged")), "{attempt}");
    assert_eq!(live.job_status(key), "merged");
    commit
}

fn update_jobs(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM jobs WHERE kind = 'update'")
}

/// The PR's head branch is `feat/x` on origin, as adopting it records (`head_ref` is set at insert only: the fixture replaces the row).
fn adopt(live: &Live) {
    live.fx.git(&live.repo, &["push", "-q", "origin", "main:refs/heads/feat/x"]);
    live.fixture(
        "CREATE TEMP TABLE adopted AS SELECT * FROM workflows WHERE key = 'repo';
         UPDATE adopted SET head_ref = 'feat/x';
         INSERT OR REPLACE INTO workflows SELECT * FROM adopted;
         DROP TABLE adopted;",
    );
}

#[test]
fn a_fix_job_on_an_open_pr_is_pushed_by_its_own_merge() {
    let live = open_pr_live();
    add_fix(&live, "fix");
    let before = live.gate_runs();

    run_fix(&live, "fix");

    let lead = live.lead_sha();
    assert!(live.lead.join("fix.txt").exists());
    assert_eq!(live.remote("cpd/pay/repo"), lead, "the Job's merge pushed the Lead Branch");
    assert_eq!(update_jobs(&live), 0, "no update Job");
    assert!(live.gate_runs()[before.len()..].iter().all(|r| !r.starts_with("make test")), "no PR Gate (pre-merge) ran: {:?}", live.gate_runs());
    assert!(live.gh_pr_calls().is_empty());
}

#[test]
fn a_fix_job_on_an_open_adopted_pr_is_pushed_by_its_own_merge() {
    let live = open_pr_live();
    adopt(&live);
    add_fix(&live, "fix");
    let before = live.gate_runs();

    run_fix(&live, "fix");

    let lead = live.lead_sha();
    assert_eq!(live.remote("feat/x"), lead, "the Job's merge pushed the Lead Branch to the pull request's head");
    assert_eq!(update_jobs(&live), 0, "no update Job");
    assert!(live.gate_runs()[before.len()..].iter().all(|r| !r.starts_with("make test")), "no PR Gate (pre-merge) ran: {:?}", live.gate_runs());
    assert!(live.gh_pr_calls().is_empty());
}

#[test]
fn a_work_job_of_an_adopted_pr_that_has_not_opened_is_not_pushed() {
    let live = open_live();
    adopt(&live);
    assert_eq!(live.value::<String>("SELECT status FROM workflows WHERE key = 'repo'"), "dispatch");
    add_fix(&live, "fix");
    let head = live.remote("feat/x");

    run_fix(&live, "fix");

    assert_ne!(live.lead_sha(), head, "the Lead Branch moved");
    assert_eq!(live.remote("feat/x"), head, "a red test must never reach a live pull request: nothing is pushed before the PR opens");
}

#[test]
fn a_failed_push_of_a_fix_job_leaves_it_merged_and_reports_push_failed() {
    let live = open_pr_live();
    add_fix(&live, "fix");
    let blocked = Arc::new(AtomicBool::new(true));
    let flag = blocked.clone();
    live.runner().on("git", move |c| flag.load(Ordering::SeqCst) && c.args.iter().any(|a| a == "push"), Reply::exit(1, "", "remote: nope\n"));

    run_fix(&live, "fix");

    assert_eq!(live.job_status("fix"), "merged");
    assert_ne!(live.remote("cpd/pay/repo"), live.lead_sha(), "the push failed");
    let errors: Vec<_> = live.plan_events("error").into_iter().map(|e| e.1).filter(|d| d["code"] == "push_failed").collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
}
