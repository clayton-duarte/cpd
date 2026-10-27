//! A Job added to an `open` PR whose `open` Job is merged is chained after that Job (docs/commands.md §5.1): its needs are the chain
//! leaves, merged with any explicit `--needs`. Written by Job chain-tests.

use serde_json::{Value, json};

use super::job::{count, events, fixture, plan, pull_request, setup, spec_file};
use crate::common::fx::Fx;
use crate::common::refused;

/// Plan `pl` in `deploy` whose PR `fix` is `open`, with its merged `open` Job `open-fix` and one earlier merged work Job `early`.
fn open_pr(fx: &Fx) {
    setup(&fx.env);
    plan(&fx.env, "pl", "deploy");
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('pl', 'fix', '/repo', 'main', 'waiting', 'cpd/pl/fix', 'tdd', cpd_txn(), 't');
             UPDATE prs SET status = 'dispatch', lead_worktree = '/wt2', started_at = 't' WHERE key = 'fix';
             {}
             UPDATE prs SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/fix/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/fix') WHERE key = 'fix';",
            pull_request("fix", 2, "open")
        ),
    );
    seed(fx, "early", "work", "merged", &[]);
    seed(fx, "open-fix", "open", "merged", &[]);
}

/// A Job row of PR `fix` as the engine and cpd leave it, in `status`, needing `needs`.
fn seed(fx: &Fx, key: &str, kind: &str, status: &str, needs: &[&str]) {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "f".repeat(64)) };
    let sha = if status == "merged" { "'c0ffee'" } else { "NULL" };
    let reason = if status == "cancelled" { "'x', 't'" } else { "NULL, NULL" };
    let cause = if status == "failed" { "'timed_out'" } else { "NULL" };
    let mut sql = format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, failed_cause, merged_sha, cancel_reason, cancel_requested_at, appended, kind, fingerprint, created_txn, created_at)
         VALUES ('pl', 'fix', '{key}', 'Title of {key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', {cause}, {sha}, {reason}, 1, '{kind}', {fingerprint}, cpd_txn(), 't');"
    );
    for need in needs {
        sql += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pl', '{key}', '{need}');");
    }
    fixture(&fx.env, &sql);
}

fn add(fx: &Fx, key: &str, extra: &[&str]) -> crate::common::Run {
    let spec = spec_file(&fx.env);
    let mut args = vec!["job", "add", key, "--title", "Fix", "--tier", "S2", "--spec", &spec, "--pr", "fix", "--plan", "pl"];
    args.extend(extra);
    fx.cpd(&args)
}

fn needs(out: &Value) -> Value {
    out["job"]["needs"].clone()
}

#[test]
fn a_fix_added_to_an_open_pr_needs_the_open_job() {
    let fx = Fx::new();
    open_pr(&fx);
    let out = add(&fx, "fix1", &[]).ok();
    assert_eq!(needs(&out), json!(["open-fix"]));
    let added = events(&fx, "job-added");
    assert_eq!(added.last().unwrap()["data"]["needs"], json!(["open-fix"]));
}

#[test]
fn a_second_fix_needs_the_first_fix_only() {
    let fx = Fx::new();
    open_pr(&fx);
    seed(&fx, "fix1", "work", "merged", &["open-fix"]);
    assert_eq!(needs(&add(&fx, "fix2", &[]).ok()), json!(["fix1"]));
}

#[test]
fn a_cancelled_fix_is_not_a_leaf_so_its_predecessor_is() {
    let fx = Fx::new();
    open_pr(&fx);
    seed(&fx, "fix1", "work", "merged", &["open-fix"]);
    seed(&fx, "fix2", "work", "cancelled", &["fix1"]);
    assert_eq!(needs(&add(&fx, "fix3", &[]).ok()), json!(["fix1"]));
}

#[test]
fn an_explicit_needs_is_merged_with_the_leaves() {
    let fx = Fx::new();
    open_pr(&fx);
    seed(&fx, "fix1", "work", "merged", &["open-fix"]);
    assert_eq!(needs(&add(&fx, "fix2", &["--needs", "early"]).ok()), json!(["early", "fix1"]));
}

#[test]
fn a_failed_leaf_refuses_needs_unavailable_and_adds_nothing() {
    let fx = Fx::new();
    open_pr(&fx);
    seed(&fx, "fix1", "work", "failed", &["open-fix"]);
    let writes = (count(&fx.env, "jobs"), count(&fx.env, "events"), count(&fx.env, "txns"));
    let e = refused(&add(&fx, "fix2", &[]), "needs_unavailable", 3);
    assert_eq!(e["details"]["jobs"], json!(["fix1"]));
    assert_eq!(e["details"]["pr"], "fix");
    assert_eq!((count(&fx.env, "jobs"), count(&fx.env, "events"), count(&fx.env, "txns")), writes);
}

#[test]
fn a_draft_added_to_an_open_pr_is_chained_and_a_draft_leaf_is_skipped() {
    let fx = Fx::new();
    open_pr(&fx);
    let first = add(&fx, "d1", &["--draft"]).ok();
    assert_eq!((&first["job"]["status"], needs(&first)), (&json!("draft"), json!(["open-fix"])));
    // The draft `d1` is no leaf: the next Job chains after the open Job again.
    assert_eq!(needs(&add(&fx, "d2", &["--draft"]).ok()), json!(["open-fix"]));
}

#[test]
fn an_update_job_is_never_a_leaf() {
    let fx = Fx::new();
    open_pr(&fx);
    seed(&fx, "update-fix", "update", "queued", &[]);
    assert_eq!(needs(&add(&fx, "fix1", &[]).ok()), json!(["open-fix"]));
}

#[test]
fn the_same_add_twice_is_already_even_after_a_later_fix() {
    let fx = Fx::new();
    open_pr(&fx);
    seed(&fx, "fix1", "work", "merged", &["open-fix"]);
    assert_eq!(needs(&add(&fx, "fix2", &[]).ok()), json!(["fix1"]));
    seed(&fx, "fix3", "work", "merged", &["fix2"]);
    let again = add(&fx, "fix2", &[]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(needs(&again), json!(["fix1"]));
}
