//! An explore Job (`kind` `scout`/`research`, `jobs.pr` NULL) belongs to the Plan, not to a PR: its Attempt is in no
//! Workflow's cost or active time (docs/contract.md 2.2 'Totals', docs/commands.md 13.2 'PR boxes'), and the Plan view
//! still renders (hs-n566). Read at a fixed `now`: no wall clock, no sleeps, no external process.

use cpd_core::ids::PlanId;
use cpd_core::store::Store;

use super::cost_tests::near;
use super::elapsed_tests::{job, run, store_with};
use crate::read;

const NOW_MS: i64 = 1_800_000_000_000;
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

/// An explore Job `key` (kind `scout`, `pr` NULL) with the given status.
fn explore_job(key: &str, status: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind)
         VALUES ('pay', NULL, '{key}', 'Explore', 'H1', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', 'scout');"
    )
}

/// One ended Attempt `<key>-a1` of Job `key` from `from` to `to` (`HH:MM:SS`) with a stored cost.
fn ended_attempt(key: &str, from: &str, to: &str, cost: &str) -> String {
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
         VALUES ('pay', '{key}-a1', '{key}', 1, 'S3', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--{key}-a1', '/w/{key}',
                 '2026-10-18T{from}.000Z', '2026-10-18T{to}.000Z', '{{\"cost_usd\":{cost}}}');"
    )
}

fn fixture() -> (tempfile::TempDir, Store) {
    let (dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(job("api", "w", "work", "running")
            + &ended_attempt("w", "10:00:00", "10:01:00", "0.25")
            + &explore_job("repro", "running")
            + &ended_attempt("repro", "11:00:00", "11:10:00", "0.5")),
    );
    (dir, store)
}

#[test]
fn a_plan_with_an_explore_job_that_has_an_attempt_renders_its_plan_view() {
    let (_dir, mut store) = fixture();
    let got = read::plan_boxes_at(&mut store, &plan(), NOW_MS);
    assert!(got.is_ok(), "{:?}", got.err());
}

#[test]
fn an_explore_attempt_is_in_no_workflows_cost_or_active_time() {
    let (_dir, mut store) = fixture();
    let boxes = read::plan_boxes_at(&mut store, &plan(), NOW_MS).unwrap();
    assert_eq!(boxes.prs.len(), 1);
    near(boxes.prs[0].cost_usd, 0.25);
    assert_eq!(boxes.prs[0].elapsed_seconds, Some(60));
}

#[test]
fn an_explore_job_has_no_node_in_a_prs_flow() {
    let (_dir, mut store) = fixture();
    run(&mut store, &explore_job("queued-one", "queued"));
    let boxes = read::plan_boxes_at(&mut store, &plan(), NOW_MS).unwrap();
    let keys: Vec<&str> = boxes.jobs.iter().map(|j| j.key.as_str()).collect();
    assert_eq!(keys, vec!["w"]);
    let flow = read::flow(&mut store, &plan()).unwrap();
    let keys: Vec<&str> = flow.jobs.iter().map(|j| j.key.as_str()).collect();
    assert_eq!(keys, vec!["w"]);
    near(boxes.prs[0].cost_usd, 0.25);
}
