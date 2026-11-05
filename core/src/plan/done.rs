//! The one rule that ends a Plan (§3.1): `done` only when every Workflow that is not `cancelled` is `merged` (one at least) and no
//! Job of the Plan is non-final. A Job is final when `merged` or `cancelled`; `draft`, `queued`, `running` and `failed` Jobs, explore
//! Jobs and `open`/`update` Jobs included, hold the Plan in `deploy`.
//!
//! Writers: `pr poll`'s merge, `pr open`'s record and `workflow cancel`'s finish. `pr review`'s `finish_plan` ends codereview Plans,
//! which have no Workflows, and is not governed by this rule.

use crate::error::CpdError;
use crate::events::{NewEvent, append};
use crate::ids::PlanId;
use crate::pr::open::merged_workflows;
use crate::store::WriteTxn;

/// In the caller's transaction: when the Plan is in `dispatch` or `deploy`, every non-cancelled Workflow is `merged` (one at least) and
/// no Job is non-final, the Plan is `done` with `done_at`, one `plan-done` Event, and its pending plan reviews superseded. When the
/// Workflows are merged but a Job is non-final, the Plan is (moved to) `deploy`, `done_at` NULL, no Event. Idempotent: a Plan already
/// `done` is left alone. Whether the Plan became `done` in this call.
pub(crate) fn settle_done(tx: &WriteTxn<'_>, plan: &PlanId) -> Result<bool, CpdError> {
    let (all_merged, blocked): (bool, bool) = tx.query_row(
        "SELECT
           NOT EXISTS (SELECT 1 FROM workflows WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled'))
             AND EXISTS (SELECT 1 FROM workflows WHERE plan = ?1 AND status = 'merged'),
           EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled'))
         FROM plans WHERE id = ?1 AND phase IN ('dispatch', 'deploy')",
        [plan.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok((false, false)),
        e => Err(e),
    })?;
    if !all_merged {
        return Ok(false);
    }
    if blocked {
        tx.execute(
            "UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'",
            [plan.as_str()],
        )?;
        return Ok(false);
    }
    crate::plan::close_reviews(tx, plan, &crate::plan::Closing::Done)?;
    tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE id = ?1 AND phase IN ('dispatch', 'deploy')",
        [plan.as_str()],
    )?;
    let text = format!(
        "Every Workflow of Plan {plan} is merged: the Plan is done. See cpd plan show --plan {plan}"
    );
    append(
        tx,
        NewEvent::plan_done(text, merged_workflows(tx, plan)?).plan(plan.clone()),
    )?;
    Ok(true)
}
