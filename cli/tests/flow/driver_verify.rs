//! A `verify` Job is run by the driver like a `docs` Job (contract §2.2 "Job kinds", §7.7; commands.md §16.1): its Worker Spec has no
//! test section and no guarded-files section, its checks are the repo's quick check only, and it ends `merged`. The harness is
//! `driver_open`'s. Written by Job verify-tests.

use std::fs;

use serde_json::json;

use super::driver::PLAN;
use super::driver_open::{live, submit};
use super::job::job_row;

#[test]
fn a_verify_job_runs_and_merges_like_a_docs_job() {
    let live = live("");
    live.fx.env.owner_policy("acme", "[gate]\nmerge = \"local\"\npr = \"local\"\n");
    live.fx.env.repo_policy("acme", "pay", "[hooks]\n[hooks.pre-commit]\ngate = \"make quick\"\n");
    live.fx.git(&live.lead, &["commit", "-q", "--allow-empty", "-m", "hooks"]);
    // The PR is `tdd`; its impl Job `build` is merged (its commit is on the Lead Branch); the first Job `api` is not wanted.
    let merged = live.commit(&live.lead, "build.txt", "build\n", "build");
    let fp = "0".repeat(64);
    live.fixture(&format!(
        "DROP TRIGGER prs_shape_fixed;
         UPDATE prs SET shape = 'tdd' WHERE key = 'repo';
         UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'api';
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, merged_sha, created_txn, created_at)
         VALUES ('pay', 'repo', 'build', 'Build', 'S2', 'Build.', 'Because.', '[\"build.txt\"]', '[\"it works\"]', '[]', 'merged', 0, 'impl', '{fp}', '{merged}', cpd_txn(), 't');
         CREATE TRIGGER prs_shape_fixed BEFORE UPDATE OF shape ON prs
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;"
    ));
    let spec = live.fx.env.root().join("verify-spec.json");
    fs::write(&spec, r#"{"goal": "Check it end to end.", "context": "The PR is built.", "touches": ["e2e/"], "acceptance": ["e2e passes"], "out_of_scope": []}"#).unwrap();
    let added = live
        .fx
        .cpd(&["job", "add", "e2e", "--title", "E2E", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--kind", "verify", "--needs", "build", "--pr", "repo", "--plan", PLAN])
        .ok();
    assert_eq!((&added["job"]["kind"], &added["job"]["status"]), (&json!("verify"), &json!("queued")), "{added}");

    let (attempt, submitted) = live.run_job("e2e", |live, id| {
        let text = fs::read_to_string(live.fx.env.cpd_home().join(format!("run/pay/specs/{id}.md"))).unwrap();
        live.commit(&live.worktree(id), "e2e.txt", "checks\n", "e2e");
        (text, submit(live, id))
    });
    let (text, submitted) = submitted;
    assert!(text.contains("Check it end to end."), "{text}");
    assert!(!text.contains("### Tests"), "no test section: {text}");
    assert!(!text.contains("Guarded files"), "no guarded-files section: {text}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(live.job_status("e2e"), "merged");
    assert_ne!(job_row(&live.fx.env, "pay", "e2e", "merged_sha"), serde_json::Value::Null);
    // The only check is the repo's quick check: no test run, no PR Gate.
    let runs = live.gate_runs();
    assert!(!runs.is_empty() && runs.iter().all(|r| r.starts_with("make quick")), "{runs:?}");
    assert!(live.lead.join("e2e.txt").exists(), "the Job's commit is on the Lead Branch");
}
