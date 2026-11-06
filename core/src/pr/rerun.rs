//! `pr rerun` (decision pro-d155; commands.md §9.8c): re-runs the failed GitHub Actions workflow runs of the
//! head commit of the Workflow's pull request. It queues a mutation (swr-github.md §9).

use serde_json::{Value, json};

use super::{Args, Env};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;
use crate::swr::github::effective_in;
use crate::swr::source::github_write::{enqueue, queued, target_of};

const FAILED: [&str; 3] = ["failure", "timed_out", "startup_failure"];

/// The workflow runs whose head-commit checks failed, in the cached detail: the `databaseId`s, each once, sorted.
fn failed_runs(detail: &Value) -> Vec<(i64, String)> {
    let mut runs: Vec<(i64, String)> = detail["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|n| {
            n["conclusion"]
                .as_str()
                .is_some_and(|c| FAILED.contains(&c.to_lowercase().as_str()))
        })
        .filter_map(|n| {
            let run = &n["checkSuite"]["workflowRun"];
            Some((
                run["databaseId"].as_i64()?,
                run["workflow"]["name"].as_str().unwrap_or_default().to_owned(),
            ))
        })
        .collect();
    runs.sort();
    runs.dedup_by_key(|r| r.0);
    runs
}

/// `pr rerun` queues the re-run of the failed workflow runs of the cached head (swr-github.md §9); the overlay shows the checks
/// `pending` at once. The engine drains it with `gh run rerun <id> --failed`, in order.
pub(super) fn rerun(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let workflow = plan.select(&args.workflow)?;
    if workflow.status != "open" {
        return Err(CpdError::new(
            ErrorCode::WorkflowNotOpen,
            format!(
                "Workflow {} of Plan {id} is {}: pr rerun needs an open Workflow.",
                workflow.key, workflow.status
            ),
        )
        .with("pr", workflow.key.as_str())
        .with("status", workflow.status.as_str()));
    }
    let row = workflow
        .pull_request
        .expect("an open Workflow has its pull_request row (prs CHECK)");
    let raw = env.store.read(|conn| crate::pr::pulls::load(conn, row))?;
    let effective = env
        .store
        .read(|conn| effective_in(conn, &raw.repo, raw.number))?
        .ok_or_else(|| {
            CpdError::new(
                ErrorCode::ItemNotFound,
                format!("{}#{} is not in the cache.", raw.repo, raw.number),
            )
        })?;
    let runs = effective
        .detail
        .as_ref()
        .map(failed_runs)
        .unwrap_or_default();
    if runs.is_empty() {
        return Err(CpdError::new(
            ErrorCode::NoFailedRun,
            format!(
                "No GitHub Actions workflow run of the head of Workflow {} has failed in the cache: nothing was re-run.",
                workflow.key
            ),
        )
        .with("pr", workflow.key.as_str())
        .with("head_oid", json!(effective.head_oid)));
    }
    let ids: Vec<i64> = runs.iter().map(|r| r.0).collect();
    let mutation = enqueue(
        &mut env.store,
        ports.clock.utc(),
        &target_of(&raw.repo, raw.number),
        "rerun",
        json!({"run_ids": ids}),
        Some(json!({"checks": "pending"})),
    )?;
    let rerun: Vec<Value> = runs.iter().map(|(i, n)| json!({"id": i, "name": n})).collect();
    queued(
        &mut env.store,
        &raw.repo,
        raw.number,
        mutation,
        json!({"pr": workflow.key, "head_sha": effective.head_oid, "rerun": rerun}),
    )
}
