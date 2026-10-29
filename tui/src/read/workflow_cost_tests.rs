//! The cost of a Workflow (`PrNode.cost_usd`, Context record pro-d179): the total of the costs of every Attempt of every
//! Job of the PR, by the rule of the Plan row. Read by `plan_boxes_at` at a fixed `now`: no wall clock, no sleeps; the
//! session files are written into the test's own temporary directory.

use std::fs;

use cpd_core::ids::PlanId;
use cpd_core::store::Store;

use super::cost_tests::{costing, near, session};
use super::elapsed_tests::{job, run, store_with};
use crate::read;

const NOW_MS: i64 = 1_800_000_000_000;
const NOW: &str = "2026-10-18T15:07:30.000Z";

/// Job `key` of PR `pr` (kind `kind`) with Attempt `<key>-a<number>` and its Worker Session `w-<key>-a<number>`; `metrics`
/// is the stored JSON of an ended Attempt, `None` for a live one (NULL `metrics`, running); `file` the Session's file.
/// `with_job` adds the Job's own row (once per Job).
fn attempt(pr: &str, key: &str, kind: &str, number: u32, metrics: Option<&str>, file: Option<&str>, with_job: bool) -> String {
    let id = format!("{key}-a{number}");
    let (status, outcome, cause, ended, metrics) = match metrics {
        Some(m) => ("ended", "'failed'", "'crashed'", "'2026-01-02T02:00:00.000Z'", format!("'{m}'")),
        None => ("running", "NULL", "NULL", "NULL", "NULL".to_owned()),
    };
    format!(
        "{}
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at,
                               worker_session, metrics)
         VALUES ('pay', '{id}', '{key}', {number}, 'S3', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{id}', '/w',
                 '2026-01-02T01:00:00.000Z', {ended}, 'w-{id}', {metrics});
         {}",
        if with_job { job(pr, key, kind, "running") } else { String::new() },
        session(&format!("w-{id}"), "worker", "ctx", Some("pay"), Some(&id), file, "")
    )
}

fn write(dir: &tempfile::TempDir, name: &str, costs: &[f64]) -> String {
    let path = dir.path().join(name).display().to_string();
    fs::write(&path, costing(costs).iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    path
}

fn costs(store: &mut Store) -> Vec<Option<f64>> {
    let boxes = read::plan_boxes_at(store, &PlanId::parse("pay").unwrap(), NOW_MS).unwrap();
    boxes.prs.iter().map(|p| p.cost_usd).collect()
}

fn stored(c: &str) -> String {
    format!(r#"{{"cost_usd":{c}}}"#)
}

#[test]
fn a_workflow_costs_the_stored_costs_of_all_its_attempts() {
    let (_dir, mut store) = store_with(&["api", "docs", "web"]);
    run(
        &mut store,
        &(attempt("api", "w", "work", 1, Some(&stored("0.25")), None, true)
            + &attempt("api", "o", "open", 1, Some(&stored("0.20")), None, true)
            + &attempt("api", "u", "update", 1, Some(&stored("0.05")), None, true)
            + &attempt("api", "r", "work", 1, Some(&stored("0.10")), None, true)
            + &attempt("api", "r", "work", 2, Some(&stored("0.15")), None, false)
            + &attempt("web", "x", "work", 1, Some(&stored("1.0")), None, true)
            + &attempt("docs", "d", "docs", 1, Some(&stored("2")), None, true)),
    );
    let got = costs(&mut store); // api, docs, web
    assert_eq!(got.len(), 3);
    near(got[0], 0.8);
    near(got[1], 2.0);
    near(got[2], 1.0);
}

#[test]
fn a_workflow_with_no_attempt_or_no_cost_data_has_no_cost() {
    let (dir, mut store) = store_with(&["a", "b", "c", "d"]);
    let says_one = write(&dir, "b.jsonl", &[1.0]);
    let missing = dir.path().join("never-written.jsonl").display().to_string();
    run(
        &mut store,
        &(job("a", "ja", "work", "queued")
            + &attempt("b", "jb", "work", 1, Some(r#"{"turns":3}"#), Some(&says_one), true)
            + &attempt("c", "jc", "work", 1, None, Some(&missing), true)
            + &attempt("d", "jd", "work", 1, Some(&stored("0.10")), None, true)
            + &attempt("d", "je", "work", 1, Some(r#"{"turns":3}"#), None, true)),
    );
    let got = costs(&mut store);
    assert_eq!(got.len(), 4);
    assert_eq!(&got[..3], &[None, None, None]);
    near(got[3], 0.1);
}

#[test]
fn a_workflow_whose_cost_totals_zero_has_some_zero() {
    let (_dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(attempt("api", "a", "work", 1, Some(&stored("0")), None, true) + &attempt("api", "b", "work", 1, Some(&stored("0.0")), None, true)),
    );
    assert_eq!(costs(&mut store), vec![Some(0.0)]);
}

#[test]
fn a_live_attempt_costs_its_worker_sessions_file() {
    let (dir, mut store) = store_with(&["api"]);
    let file = write(&dir, "live.jsonl", &[0.25, 0.25]);
    run(&mut store, &attempt("api", "a", "work", 1, None, Some(&file), true));
    near(costs(&mut store)[0], 0.5);
    run(&mut store, &attempt("api", "b", "work", 1, Some(&stored("0.5")), None, true));
    near(costs(&mut store)[0], 1.0);
}

#[test]
fn a_written_metrics_is_the_only_source_of_an_attempts_cost() {
    let (dir, mut store) = store_with(&["api"]);
    let file = write(&dir, "nine.jsonl", &[9.0]);
    run(&mut store, &attempt("api", "a", "work", 1, Some(&stored("0.25")), Some(&file), true));
    near(costs(&mut store)[0], 0.25);
}

#[test]
fn a_workflow_costs_what_the_plan_row_counts_for_its_attempts() {
    let (dir, mut store) = store_with(&["api"]);
    let file = write(&dir, "live.jsonl", &[0.25]);
    run(
        &mut store,
        &(attempt("api", "a", "work", 1, Some(&stored("0.5")), None, true) + &attempt("api", "b", "work", 1, None, Some(&file), true)),
    );
    let tree = read::tree_at(&mut store, Some(NOW)).unwrap();
    let plan = tree.contexts.iter().flat_map(|c| &c.plans).find(|p| p.head.id.as_str() == "pay").unwrap();
    near(plan.cost_usd, 0.75);
    near(costs(&mut store)[0], 0.75);
}
