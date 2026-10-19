//! `pr cancel` (commands.md §9.6, contract §3.8): `waiting → cancelled` before
//! the lock; after it, a `waiting` or `dispatch` PR whose Jobs are all
//! `cancelled` (or that has none) too. The PR keeps its key; before the lock
//! the plan fingerprint changes, after it the Plan may move on.

use rusqlite::Connection;
use serde_json::json;

use super::open::{merged_prs, opened_prs};
use super::{CancelArgs, Env, find_pr, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs;
use crate::store::WriteTxn;

const COMMAND: &str = "pr cancel";

enum Decision {
    /// Cancel it; `started` when it is `dispatch`, so its worktree is cleared from the row.
    Cancel,
    /// It was already `cancelled`.
    Already,
}

pub(super) fn cancel(args: CancelArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;
    let mut moved_to = None;
    let already = match env.store.read(|conn| decide(conn, &plan, &key))? {
        Decision::Already => true,
        Decision::Cancel => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key)? {
            Decision::Already => Ok(true),
            Decision::Cancel => {
                // `prs` allows no Lead worktree or start time on a `cancelled` PR (the worktree on disk stays, §3.8).
                let moved = tx.execute(
                    "UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL
                      WHERE plan = ?1 AND key = ?2 AND status IN ('waiting', 'dispatch')",
                    [plan.as_str(), key.as_str()],
                )?;
                if moved == 0 {
                    return Err(CpdError::new(ErrorCode::CasLost, format!("PR {key} changed while it was cancelled: rerun the command."))
                        .with("plan", plan.as_str())
                        .with("pr", key.as_str()));
                }
                let text = format!("PR {key} of Plan {plan} was cancelled. Next: cpd plan show --plan {plan}");
                append(tx, NewEvent::new(Kind::PrCancelled, text, json!({"pr": key})).plan(plan.clone()))?;
                moved_to = advance(tx, &plan, &key)?;
                Ok(false)
            }
        })?,
    };
    let mut out = json!({"pr": {"key": key, "status": "cancelled"}});
    if already {
        out["already"] = json!(true);
    }
    if let Some(phase) = moved_to {
        out["phase"] = json!(phase.as_str());
    }
    Ok(Output::success(out))
}

/// After the lock, the cancelled PR may have been the last one `waiting` or `dispatch`: the Plan then moves on as
/// `pr open` and `pr poll` move it (§3.1): `dispatch → deploy`, and `deploy → done` when every PR left is `merged`.
/// Never when no PR is left that is not `cancelled`. The Plan's new phase, if it moved.
fn advance(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey) -> Result<Option<Phase>, CpdError> {
    if plan_phase(tx, plan)? != Phase::Dispatch {
        return Ok(None);
    }
    let count = |statuses: &str| -> Result<i64, CpdError> {
        Ok(tx.query_row(&format!("SELECT count(*) FROM prs WHERE plan = ?1 AND status {statuses}"), [plan.as_str()], |r| r.get(0))?)
    };
    if count("IN ('waiting', 'dispatch')")? > 0 || count("<> 'cancelled'")? == 0 {
        return Ok(None);
    }
    let lost = || {
        CpdError::new(ErrorCode::CasLost, format!("Plan {plan} left phase dispatch while PR {key} was cancelled: rerun the command."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
    };
    if tx.execute("UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'", [plan.as_str()])? == 0 {
        return Err(lost());
    }
    let text = format!("Every PR of Plan {plan} that is not cancelled is open or merged. The engine polls them; to follow them by hand: cpd pr poll --plan {plan}");
    append(tx, NewEvent::new(Kind::PlanPhase, text, json!({"from": "dispatch", "to": "deploy", "prs": opened_prs(tx, plan)?})).plan(plan.clone()))?;
    if count("NOT IN ('merged', 'cancelled')")? > 0 {
        return Ok(Some(Phase::Deploy));
    }
    let done = tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND phase = 'deploy'",
        [plan.as_str()],
    )?;
    if done == 0 {
        return Err(lost());
    }
    let text = format!("Every PR of Plan {plan} is merged: the Plan is done. See cpd plan show --plan {plan}");
    append(tx, NewEvent::new(Kind::PlanDone, text, json!({"prs": merged_prs(tx, plan)?})).plan(plan.clone()))?;
    Ok(Some(Phase::Done))
}

fn decide(conn: &Connection, plan: &PlanId, key: &PrKey) -> Result<Decision, CpdError> {
    let prs = load_prs(conn, plan)?;
    let pr = find_pr(&prs, plan, key)?;
    let locked = !matches!(plan_phase(conn, plan)?, Phase::Grounding | Phase::Plan);
    match (pr.status.as_str(), locked) {
        ("cancelled", _) => return Ok(Decision::Already),
        ("waiting", _) | ("dispatch", true) => {}
        (status, _) => {
            let only = if locked { "only a waiting or dispatch PR is cancelled" } else { "only a waiting PR is cancelled" };
            return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {key} is {status}: {only}."))
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
