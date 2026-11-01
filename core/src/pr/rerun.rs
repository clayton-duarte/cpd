//! `pr rerun` (decision pro-d155; commands.md §9.8c): re-runs the failed GitHub Actions workflow runs of the
//! head commit of the PR's pull request. It writes nothing in the store and appends no Event.

use serde_json::{Value, json};

use super::{Args, Env, gh};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;

const FAILED: [&str; 3] = ["failure", "timed_out", "startup_failure"];

pub(super) fn rerun(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let pr = plan.select(&args.pr)?;
    if pr.status != "open" {
        return Err(CpdError::new(
            ErrorCode::PrNotOpen,
            format!(
                "PR {} of Plan {id} is {}: pr rerun needs an open PR.",
                pr.key, pr.status
            ),
        )
        .with("pr", pr.key.as_str())
        .with("status", pr.status.as_str()));
    }
    let number = pr
        .pull
        .as_ref()
        .expect("an open PR has its pull request (prs CHECK)")
        .number;
    let lead = pr.lead(&id)?;
    let runner = &*env.ports.runner;

    let head_sha = gh::head_commit(runner, lead, number)?;
    let runs = gh::workflow_runs(runner, lead, &head_sha)?;
    let failed: Vec<&gh::WorkflowRun> = runs
        .iter()
        .filter(|r| FAILED.contains(&r.conclusion.to_lowercase().as_str()))
        .collect();
    if failed.is_empty() {
        let listed: Vec<Value> = runs
            .iter()
            .map(|r| json!({"id": r.id, "name": r.name, "status": r.status, "conclusion": r.conclusion}))
            .collect();
        return Err(CpdError::new(
            ErrorCode::NoFailedRun,
            format!("No GitHub Actions workflow run of commit {head_sha} of PR {} has failed: nothing was re-run.", pr.key),
        )
        .with("pr", pr.key.as_str())
        .with("head_sha", head_sha.as_str())
        .with("runs", json!(listed)));
    }

    let mut rerun: Vec<Value> = Vec::new();
    for run in failed {
        if let Err(e) = gh::rerun_failed(runner, lead, run.id) {
            return Err(e.with("rerun", json!(rerun)));
        }
        rerun.push(json!({"id": run.id, "name": run.name}));
    }
    Ok(Output::success(
        json!({"pr": pr.key, "head_sha": head_sha, "rerun": rerun}),
    ))
}
