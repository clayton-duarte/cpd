//! E4, completion (contract §3.7): a Plan in `dispatch` with no Job `queued`
//! or `running`, at least one `merged`, and its one PR's `all_merged_at` null
//! gets `all-merged` `{merged, cancelled}` and that `all_merged_at`, once.

use serde_json::json;

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use crate::error::CpdError;
use crate::events::{self, Kind, NewEvent};
use crate::ids::PlanId;
use crate::store::WriteTxn;

/// The Plans E4 is for; `?1` is one Plan's id, or NULL for every Plan.
const DONE: &str = "SELECT p.id FROM plans p
  WHERE (?1 IS NULL OR p.id = ?1) AND p.phase = 'dispatch'
    AND NOT EXISTS (SELECT 1 FROM prs r WHERE r.plan = p.id AND r.all_merged_at IS NOT NULL)
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = p.id AND j.status IN ('queued', 'running'))
    AND EXISTS (SELECT 1 FROM jobs j WHERE j.plan = p.id AND j.status = 'merged')
  ORDER BY p.id";

pub(super) fn run(engine: &mut Engine) {
    let plans = engine.store.read(|conn| {
        let mut stmt = conn.prepare(DONE)?;
        let ids = stmt.query_map([None::<&str>], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter().map(|id| PlanId::parse(id)).collect::<Result<Vec<_>, CpdError>>()
    });
    let plans = match plans {
        Ok(plans) => {
            engine.clear(Step::E4, &About::Machine);
            plans
        }
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    for plan in plans {
        let about = About::Plan(plan.clone());
        match engine.store.write(SOURCE, |tx| complete(tx, &plan)) {
            Ok(done) => {
                if done {
                    engine.log.line(&format!("E4: every Job of Plan {plan} is done: all-merged"));
                }
                engine.clear(Step::E4, &about);
            }
            Err(e) => engine.fail(Step::E4, about, &e),
        }
    }
}

/// The keys of `plan`'s Jobs in `status`, in key order.
fn keys(tx: &WriteTxn<'_>, plan: &PlanId, status: &str) -> Result<Vec<String>, CpdError> {
    let mut stmt = tx.prepare("SELECT key FROM jobs WHERE plan = ?1 AND status = ?2 ORDER BY key")?;
    let keys = stmt.query_map([plan.as_str(), status], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(keys)
}

/// Checks the condition again inside the transaction, then sets
/// `all_merged_at` and appends `all-merged`. False when it no longer holds.
fn complete(tx: &WriteTxn<'_>, plan: &PlanId) -> Result<bool, CpdError> {
    let still: bool = tx.query_row(&format!("SELECT EXISTS ({DONE})"), [plan.as_str()], |r| r.get(0))?;
    if !still {
        return Ok(false);
    }
    tx.execute(
        "UPDATE prs SET all_merged_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND all_merged_at IS NULL",
        [plan.as_str()],
    )?;
    let merged = keys(tx, plan, "merged")?;
    let cancelled = keys(tx, plan, "cancelled")?;
    let failed = keys(tx, plan, "failed")?;
    let mut text = format!("No Job of Plan {plan} is queued or running. Merged: {}.", merged.join(", "));
    if !cancelled.is_empty() {
        text += &format!(" Cancelled: {}.", cancelled.join(", "));
    }
    if failed.is_empty() {
        text += &format!(" Next: cpd pr open --plan {plan}");
    } else {
        text += &format!(
            " Failed: {}; decide each with cpd job retry <job> --tier <T> --plan {plan} or cpd job cancel <job> --reason <text> --plan {plan}, then: cpd pr open --plan {plan}",
            failed.join(", ")
        );
    }
    events::append(tx, NewEvent::new(Kind::AllMerged, text, json!({"merged": merged, "cancelled": cancelled})).plan(plan.clone()))?;
    Ok(true)
}
