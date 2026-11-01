//! The read model of the Plan view carries a Plan's explore Jobs (`jobs.workflow` NULL, kind `scout`/`research`) beside its Workflow
//! Jobs, as nodes: `plan_boxes_at(...).jobs` and `flow(...).jobs` (Plan `explore-view`, D1-A). An explore Job has no Workflow,
//! so its Attempts count in no Workflow's cost (`WorkflowNode.cost_usd`) and no Workflow's active time (`WorkflowNode.elapsed_seconds`)
//! and stay in the Plan's cost (decision pro-d237, unchanged by the visibility).
//! Fixed `now_ms`: no wall clock, no sleep, no git, no tmux.

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
        Some(m) => ("ended", "'failed'", "'crashed'", "'2026-01-02T02:00:00.000Z'", format!("'{m}'")),
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
    run(&mut store, &(work() + &explore("look", Some(r#"{"cost_usd":0.40}"#))));
    (dir, store)
}

fn job_node<'a>(jobs: &'a [read::JobNode], key: &str) -> Option<&'a read::JobNode> {
    jobs.iter().find(|j| j.key.as_str() == key)
}

#[test]
fn the_read_model_lists_a_plans_explore_job_beside_its_pr_jobs() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &(job("api", "work", "work", "queued") + &explore("look", Some(r#"{"cost_usd":0.40}"#))));

    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    assert!(job_node(&boxes.jobs, "work").is_some(), "the Workflow Job is a node: {:?}", keys(&boxes.jobs));
    let look = job_node(&boxes.jobs, "look").expect("the explore Job is a node");
    assert_eq!(look.attempts.len(), 1, "the explore Job carries its Attempts");

    let flow = read::flow(&mut store, &pay()).unwrap();
    assert!(job_node(&flow.jobs, "work").is_some(), "the Workflow Job is in the flow: {:?}", keys(&flow.jobs));
    assert!(job_node(&flow.jobs, "look").is_some(), "the explore Job is in the flow");
}

fn keys(jobs: &[read::JobNode]) -> Vec<String> {
    jobs.iter().map(|j| j.key.to_string()).collect()
}

#[test]
fn a_plan_with_only_explore_jobs_is_not_empty() {
    let (_dir, mut store) = store_with(&[]);
    run(&mut store, &explore("look", None));

    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    assert!(boxes.workflows.is_empty(), "no Workflow box");
    assert!(job_node(&boxes.jobs, "look").is_some(), "the explore Job is the Plan's content");
    assert_every_cockpit_read_is_ok(&mut store);
}

/// Every cockpit read of Plan `pay` is Ok.
fn assert_every_cockpit_read_is_ok(store: &mut Store) {
    let context = ContextId::parse("ctx").unwrap();
    let reads: Vec<(&str, Result<(), CpdError>)> = vec![
        ("read::flow", read::flow(store, &pay()).map(drop)),
        ("read::plan_boxes_at", read::plan_boxes_at(store, &pay(), NOW_MS).map(drop)),
        ("read::tree_at", read::tree_at(store, Some(NOW)).map(drop)),
        ("read::widgets", read::widgets(store, 3, 5).map(drop)),
        ("read::slots", read::slots(store, 3).map(drop)),
        ("read::pulse", read::pulse(store, &context, NOW_MS, 12).map(drop)),
    ];
    for (name, result) in reads {
        assert!(result.is_ok(), "{name} failed: {result:?}");
    }
}

#[test]
fn an_explore_attempts_cost_is_in_no_pr_box_and_in_the_plan_total() {
    let (_dir, mut store) = mixed();
    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    assert!(job_node(&boxes.jobs, "look").is_some(), "the explore Job is a node");
    assert_eq!(boxes.workflows.len(), 1);
    near(boxes.workflows[0].cost_usd, 0.25); // the explore Attempt is in no Workflow box

    let tree = read::tree_at(&mut store, Some(NOW)).unwrap();
    let plan = tree.contexts.iter().flat_map(|c| &c.plans).find(|p| p.head.id.as_str() == "pay").unwrap();
    near(plan.cost_usd, 0.65); // 0.25 + the explore 0.40
}

#[test]
fn an_explore_attempts_active_time_is_in_no_pr_box() {
    let (_dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(job("api", "work", "work", "running")
            + "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
               VALUES ('pay', 'work-a1', 'work', 1, 'S3', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--work-a1', '/w',
                       '2026-01-02T10:00:00.000Z', '2026-01-02T10:01:00.000Z', '{\"cost_usd\":0.25}');"
            + &explore_job("look", "running")
            + "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, metrics)
               VALUES ('pay', 'look-a1', 'look', 1, 'S1', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--look-a1', '/run/pay/explore',
                       '2026-01-02T11:00:00.000Z', '2026-01-02T11:10:00.000Z', '{\"cost_usd\":0.40}');"),
    );
    let boxes = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    assert!(job_node(&boxes.jobs, "look").is_some(), "the explore Job is a node");
    assert_eq!(boxes.workflows.len(), 1);
    assert_eq!(boxes.workflows[0].elapsed_seconds, Some(60), "only the Workflow Job's Attempt counts");
}
