//! The plan view of a Plan whose Attempt is an explore Job (Decision pro-d237, Plan `explore-roles`): an explore Job
//! (kind `scout` or `research`) has `jobs.workflow` NULL; the Workflow cost must not read it, and every cockpit read takes it.
//! Fixed `now`: no wall clock, no sleep, no git, no tmux.

use cpd_core::error::CpdError;
use cpd_core::ids::{ContextId, PlanId};
use cpd_core::store::Store;

use super::cost_tests::near;
use super::elapsed_tests::{job, run, store_with};
use crate::read;

const NOW_MS: i64 = 1_800_000_000_000;
const NOW: &str = "2026-10-18T15:07:30.000Z";
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn pay() -> PlanId {
    PlanId::parse("pay").unwrap()
}

/// An explore Job `key` (kind `scout`, no Workflow) of Job status `status`, with no Attempt.
fn explore_job(key: &str, status: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind)
         VALUES ('pay', NULL, '{key}', 'Job', 'S1', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', 'scout');"
    )
}

/// Attempt `<key>-a1` of explore Job `key` (`metrics` JSON stored: ended; `None`: running).
fn explore_attempt(key: &str, metrics: Option<&str>) -> String {
    let (status, outcome, cause, ended, metrics) = match metrics {
        Some(m) => (
            "ended",
            "'failed'",
            "'crashed'",
            "'2026-01-02T02:00:00.000Z'",
            format!("'{m}'"),
        ),
        None => ("running", "NULL", "NULL", "NULL", "NULL".to_owned()),
    };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
         VALUES ('pay', '{key}-a1', '{key}', 1, 'S1', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{key}-a1', '/run/pay/explore',
                 '2026-01-02T01:00:00.000Z', {ended}, {metrics});"
    )
}

/// An explore Job `key` (running) with Attempt `<key>-a1` (`metrics` JSON stored: ended; `None`: running).
fn explore(key: &str, metrics: Option<&str>) -> String {
    explore_job(key, "running") + &explore_attempt(key, metrics)
}

/// The Job `work` of Workflow `api` with one Attempt of cost 0.25.
fn work() -> String {
    job("api", "work", "work", "running")
        + "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
           VALUES ('pay', 'work-a1', 'work', 1, 'S3', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--work-a1', '/w',
                   '2026-01-02T01:00:00.000Z', '2026-01-02T02:00:00.000Z', '{\"cost_usd\":0.25}');"
}

/// Plan `pay` with Workflow `api` (one work Job, cost 0.25) and scout Job `look` (cost 0.40).
fn mixed() -> (tempfile::TempDir, Store) {
    let (dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(work() + &explore("look", Some(r#"{"cost_usd":0.40}"#))),
    );
    (dir, store)
}

#[test]
fn plan_boxes_reads_a_plan_with_an_explore_attempt() {
    let (_dir, mut store) = mixed();
    let result = read::plan_boxes_at(&mut store, &pay(), NOW_MS);
    assert!(result.is_ok(), "plan_boxes_at failed: {result:?}");
    let boxes = result.unwrap();
    assert_eq!(boxes.workflows.len(), 1);
    assert_eq!(boxes.workflows[0].key.as_str(), "api");
}

#[test]
fn plan_boxes_reads_a_plan_with_only_explore_attempts() {
    let (_dir, mut store) = store_with(&[]);
    run(&mut store, &explore("look", None));
    let result = read::plan_boxes_at(&mut store, &pay(), NOW_MS);
    assert!(result.is_ok(), "plan_boxes_at failed: {result:?}");
    let boxes = result.unwrap();
    assert!(boxes.workflows.is_empty());
    assert!(boxes.jobs.iter().any(|j| j.key.as_str() == "look"), "the explore Job is a JobNode of the Plan");
}

#[test]
fn an_explore_attempts_cost_is_in_no_workflow() {
    let (_dir, mut store) = mixed();
    let result = read::plan_boxes_at(&mut store, &pay(), NOW_MS);
    assert!(result.is_ok(), "plan_boxes_at failed: {result:?}");
    let boxes = result.unwrap();
    assert_eq!(boxes.workflows.len(), 1);
    near(boxes.workflows[0].cost_usd, 0.25);
}

#[test]
fn an_explore_attempts_cost_is_in_the_plans_cost() {
    let (_dir, mut store) = mixed();
    let tree = read::tree_at(&mut store, Some(NOW)).unwrap();
    let plan = tree
        .contexts
        .iter()
        .flat_map(|c| &c.plans)
        .find(|p| p.head.id.as_str() == "pay")
        .unwrap();
    near(plan.cost_usd, 0.65);
}

#[test]
fn every_cockpit_read_takes_a_plan_with_an_explore_attempt() {
    let (_dir, mut store) = mixed();
    assert_every_cockpit_read_is_ok(&mut store);
}

/// Every cockpit read of Plan `pay` is Ok.
fn assert_every_cockpit_read_is_ok(store: &mut Store) {
    let context = ContextId::parse("ctx").unwrap();
    let reads: Vec<(&str, Result<(), CpdError>)> = vec![
        ("read::flow", read::flow(store, &pay()).map(drop)),
        (
            "read::plan_boxes_at",
            read::plan_boxes_at(store, &pay(), NOW_MS).map(drop),
        ),
        ("read::tree_at", read::tree_at(store, Some(NOW)).map(drop)),
        ("read::widgets", read::widgets(store, 3, 5).map(drop)),
        ("read::slots", read::slots(store, 3).map(drop)),
        (
            "read::pulse",
            read::pulse(store, &context, NOW_MS, 12).map(drop),
        ),
    ];
    for (name, result) in reads {
        assert!(result.is_ok(), "{name} failed: {result:?}");
    }
}

#[test]
fn an_explore_job_is_a_node_beside_the_pr_job_in_the_flow() {
    let (_dir, mut store) = mixed();
    let flow = read::flow(&mut store, &pay()).unwrap();
    let keys: Vec<&str> = flow.jobs.iter().map(|j| j.key.as_str()).collect();
    assert!(keys.contains(&"work") && keys.contains(&"look"), "{keys:?}");
    assert!(flow.jobs.iter().find(|j| j.key.as_str() == "look").unwrap().workflow.is_none(), "the explore Job has no Workflow");
}

#[test]
fn an_explore_attempts_time_is_in_no_workflows_active_time() {
    let (_dir, mut store) = store_with(&["api"]);
    let explore_run = format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind)
         VALUES ('pay', NULL, 'look', 'Job', 'S1', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', 'scout');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
         VALUES ('pay', 'look-a1', 'look', 1, 'S1', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--look-a1', '/run/pay/explore',
                 '2026-01-02T11:00:00.000Z', '2026-01-02T11:10:00.000Z', '{{\"cost_usd\":0.40}}');"
    );
    run(
        &mut store,
        &(job("api", "work", "work", "running")
            + "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
               VALUES ('pay', 'work-a1', 'work', 1, 'S3', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--work-a1', '/w',
                       '2026-01-02T10:00:00.000Z', '2026-01-02T10:01:00.000Z', '{\"cost_usd\":0.25}');"
            + &explore_run),
    );
    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    assert_eq!(boxes.workflows.len(), 1);
    assert_eq!(boxes.workflows[0].elapsed_seconds, Some(60));
}

#[test]
fn a_queued_explore_job_with_no_attempt_is_a_node_beside_the_pr_job() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &(work() + &explore_job("look", "queued")));
    let flow = read::flow(&mut store, &pay()).unwrap();
    let keys: Vec<&str> = flow.jobs.iter().map(|j| j.key.as_str()).collect();
    assert!(keys.contains(&"work") && keys.contains(&"look"), "{keys:?}");
    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    let keys: Vec<&str> = boxes.jobs.iter().map(|j| j.key.as_str()).collect();
    assert!(keys.contains(&"work") && keys.contains(&"look"), "{keys:?}");
    near(boxes.workflows[0].cost_usd, 0.25);
}

#[test]
fn a_queued_explore_job_with_an_ended_attempt_is_read_by_every_cockpit_read() {
    let (_dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(work()
            + &explore_job("look", "queued")
            + &explore_attempt("look", Some(r#"{"cost_usd":0.40}"#))),
    );
    assert_every_cockpit_read_is_ok(&mut store);
    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    near(boxes.workflows[0].cost_usd, 0.25);
    let tree = read::tree_at(&mut store, Some(NOW)).unwrap();
    let plan = tree
        .contexts
        .iter()
        .flat_map(|c| &c.plans)
        .find(|p| p.head.id.as_str() == "pay")
        .unwrap();
    near(plan.cost_usd, 0.65);
}
