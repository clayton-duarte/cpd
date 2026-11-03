//! Scout-first Grounding: `cpd plan phase plan` is refused `explore_pending` while an explore Job (kind `scout` or
//! `research`) of the Plan is queued, running or failed (commands.md §4.4); merged and cancelled ones do not block.

use crate::common;

use crate::common::session::setup_with_fork;
use common::TestEnv;
use common::fx::Fx;
use common::refused;
use serde_json::json;

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// An explore Job of Plan `pay` (its `pr` NULL) of `kind` in `status`, touching `explore/<key>.md`.
fn explore(env: &TestEnv, key: &str, kind: &str, status: &str) {
    env.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint,
                           merged_sha, summary, failed_cause, cancel_reason, cancel_requested_at, created_txn, created_at)
         VALUES ('pay', NULL, '{key}', 'Title {key}', 'S2', 'Goal.', 'Context.', '[\"explore/{key}.md\"]', '[\"ok\"]', '[]', '{kind}', '{status}', 1, '{FP}',
                 iif('{status}' = 'merged', 'abc123', NULL), iif('{status}' = 'merged', 'Found.', NULL),
                 iif('{status}' = 'failed', 'check_failed', NULL),
                 iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');"
    ));
}

fn phase(fx: &Fx) -> String {
    fx.env
        .query::<String>("SELECT phase FROM plans WHERE id = 'pay'")
}

/// Plan `pay` in phase grounding; `plan phase plan` is refused with exactly `jobs`, and nothing changes.
fn assert_blocked(fx: &Fx, jobs: &[&str]) {
    let events = || fx.env.query::<i64>("SELECT count(*) FROM events");
    let before = events();
    let error = refused(
        &fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]),
        "explore_pending",
        3,
    );
    assert_eq!(
        error["details"],
        json!({"plan": "pay", "jobs": jobs}),
        "{error}"
    );
    assert_eq!(phase(fx), "grounding");
    assert_eq!(events(), before);
}

fn assert_allowed(fx: &Fx) {
    let out = fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "phase": "plan"}));
    assert_eq!(phase(fx), "plan");
}

fn world() -> Fx {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx
}

#[test]
fn plan_phase_is_refused_for_a_queued_explore_job() {
    let fx = world();
    explore(&fx.env, "scout-a", "scout", "queued");
    assert_blocked(&fx, &["scout-a"]);
    // Control: once merged, the same command succeeds.
    fx.env.sql(
        "UPDATE jobs SET status = 'merged', merged_sha = 'abc123', summary = 'Found.' WHERE plan = 'pay' AND key = 'scout-a'",
    );
    assert_allowed(&fx);
}

#[test]
fn plan_phase_is_refused_for_a_running_explore_job() {
    let fx = world();
    explore(&fx.env, "scout-a", "scout", "running");
    assert_blocked(&fx, &["scout-a"]);
}

#[test]
fn plan_phase_is_refused_for_a_failed_explore_job() {
    let fx = world();
    explore(&fx.env, "study", "research", "failed");
    assert_blocked(&fx, &["study"]);
}

#[test]
fn plan_phase_lists_every_pending_explore_job_sorted() {
    let fx = world();
    explore(&fx.env, "z-scout", "scout", "queued");
    explore(&fx.env, "a-research", "research", "running");
    explore(&fx.env, "m-done", "scout", "merged");
    explore(&fx.env, "n-gone", "research", "cancelled");
    assert_blocked(&fx, &["a-research", "z-scout"]);
}
