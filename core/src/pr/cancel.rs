//! `pr cancel` (commands.md §9.6, contract §3.8): `waiting → cancelled`,
//! before the lock. The PR keeps its key; the plan fingerprint changes.

use rusqlite::Connection;
use serde_json::json;

use super::{CancelArgs, Env, find_pr, plan_phase, pr_locked};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs;

const COMMAND: &str = "pr cancel";

enum Decision {
    Cancel,
    /// It was already `cancelled`.
    Already,
}

pub(super) fn cancel(args: CancelArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;
    let already = match env.store.read(|conn| decide(conn, &plan, &key))? {
        Decision::Already => true,
        Decision::Cancel => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key)? {
            Decision::Already => Ok(true),
            Decision::Cancel => {
                let moved = tx.execute(
                    "UPDATE prs SET status = 'cancelled' WHERE plan = ?1 AND key = ?2 AND status = 'waiting'",
                    [plan.as_str(), key.as_str()],
                )?;
                if moved == 0 {
                    return Err(CpdError::new(ErrorCode::CasLost, format!("PR {key} changed while it was cancelled: rerun the command."))
                        .with("plan", plan.as_str())
                        .with("pr", key.as_str()));
                }
                let text = format!("PR {key} of Plan {plan} was cancelled. Next: cpd plan show --plan {plan}");
                append(tx, NewEvent::new(Kind::PrCancelled, text, json!({"pr": key})).plan(plan.clone()))?;
                Ok(false)
            }
        })?,
    };
    let mut out = json!({"pr": {"key": key, "status": "cancelled"}});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

fn decide(conn: &Connection, plan: &PlanId, key: &PrKey) -> Result<Decision, CpdError> {
    let prs = load_prs(conn, plan)?;
    let pr = find_pr(&prs, plan, key)?;
    let phase = plan_phase(conn, plan)?;
    if !matches!(phase, Phase::Grounding | Phase::Plan) {
        return Err(pr_locked(plan, phase, COMMAND));
    }
    match pr.status.as_str() {
        "cancelled" => return Ok(Decision::Already),
        "waiting" => {}
        status => {
            return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {key} is {status}: only a waiting PR is cancelled."))
                .with("plan", plan.as_str())
                .with("pr", key.as_str())
                .with("from", status)
                .with("to", "cancelled"));
        }
    }
    let live_jobs: Vec<String> =
        jobs::load(conn, plan)?.into_iter().filter(|j| j.pr == key.as_str() && j.status != "cancelled").map(|j| j.key).collect();
    let needing: Vec<String> =
        prs.iter().filter(|p| p.status != "cancelled" && p.needs.iter().any(|n| n == key.as_str())).map(|p| p.key.clone()).collect();
    if !live_jobs.is_empty() || !needing.is_empty() {
        return Err(CpdError::new(
            ErrorCode::PrInUse,
            format!("PR {key} is in use: cancel its Jobs (cpd job cancel) and edit away the needs on it (cpd pr edit) first."),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("jobs", live_jobs)
        .with("prs", needing));
    }
    Ok(Decision::Cancel)
}
