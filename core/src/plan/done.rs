//! The one rule that ends a Plan (§3.1): `done` only when every Workflow that is not `cancelled` is `merged` (one at least) and no
//! Job of the Plan is non-final. A Job is final when `merged` or `cancelled`; `draft`, `queued`, `running` and `failed` Jobs, explore
//! Jobs and `open`/`update` Jobs included, hold the Plan in `deploy`.
//!
//! Writers: `pr poll`'s merge, `pr open`'s record and `workflow cancel`'s finish. `pr review`'s `finish_plan` ends codereview Plans,
//! which have no Workflows, and is not governed by this rule.

use rusqlite::OptionalExtension;

use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, WorkflowKey};
use crate::pr::open::merged_workflows;
use crate::store::WriteTxn;

/// Verifies that all outline `done_when` workflows are registered and merged, and no drafts remain.
pub(crate) fn check_complete(conn: &rusqlite::Connection, plan: &PlanId) -> Result<(), CpdError> {
    if let Ok(Some(outline)) = crate::plan::outline::load(conn, plan) {
        for key in outline.done_when.keys() {
            let status: Option<String> = conn
                .query_row(
                    "SELECT status FROM workflows WHERE plan = ?1 AND key = ?2",
                    [plan.as_str(), key.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            if status.as_deref() != Some("merged") {
                return Err(CpdError::new(
                    ErrorCode::PlanIncomplete,
                    format!(
                        "Plan {plan} cannot reach done: workflow {key} is unregistered or unmerged."
                    ),
                )
                .with("plan", plan.as_str())
                .with("workflow", key.as_str()));
            }
        }
    }
    let draft_exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND status = 'draft')",
        [plan.as_str()],
        |r| r.get(0),
    )?;
    if draft_exists {
        return Err(CpdError::new(
            ErrorCode::PlanIncomplete,
            format!("Plan {plan} cannot reach done: unresolved draft jobs remain."),
        )
        .with("plan", plan.as_str()));
    }
    Ok(())
}

/// In the caller's transaction: when the Plan is in `dispatch` or `deploy` (never `plan`), every non-cancelled Workflow is `merged` (one at least) and
/// no Job is non-final, the Plan steps dispatch -> deploy (no Event) and is `done` with `done_at`, one `plan-done` Event, and its pending plan reviews superseded. When the
/// Workflows are merged but a Job is non-final, the Plan is (moved to) `deploy`, `done_at` NULL, no Event. Idempotent: a Plan already
/// `done` is left alone. `just` names the Workflow this call merged and the time `gh` reported for it: that time, not the
/// transaction time, is its `merged_at` in the Event. Whether the Plan became `done` in this call.
///
/// A `shaping` Plan with no Workflow settles once it has at least one Job and every Job is final (`merged` or
/// `cancelled`): it moves `shaping -> done` with `done_at` set and one quiet `plan-done` `{workflows: [], turn: false}`.
/// Both paired and historical Lead-less shaping Plans keep `locked_at` null. A paired shaping-origin Plan reopened to
/// `plan` settles the same way while still unlocked and without any Workflow. Idempotent.
pub(crate) fn settle_done(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    just: Option<(&WorkflowKey, &str)>,
) -> Result<bool, CpdError> {
    let exploration_phase: Option<String> = tx
        .query_row(
            "SELECT phase FROM plans WHERE id = ?1 AND (phase = 'shaping' OR
              (phase = 'plan' AND shaping_origin = 1 AND lead_session IS NOT NULL AND locked_at IS NULL))",
            [plan.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(phase) = exploration_phase {
        let settle: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1)
               AND NOT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled'))
               AND NOT EXISTS (SELECT 1 FROM workflows WHERE plan = ?1)",
            [plan.as_str()],
            |r| r.get(0),
        )?;
        if !settle {
            return Ok(false);
        }
        crate::plan::close_reviews(tx, plan, &crate::plan::Closing::Done)?;
        let moved = tx.execute(
            "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE id = ?1 AND phase = ?2",
            [plan.as_str(), &phase],
        )?;
        if moved == 0 {
            return Ok(false);
        }
        let text = format!(
            "Every Job of Plan {plan} is final: the Plan is done. See cpd plan show --plan {plan}"
        );
        append(tx, NewEvent::plan_done_quiet(text).plan(plan.clone()))?;
        return Ok(true);
    }
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
    tx.execute(
        "UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'",
        [plan.as_str()],
    )?;
    if blocked {
        return Ok(false);
    }
    if let Err(e) = check_complete(tx, plan) {
        let context: Option<String> = tx
            .query_row(
                "SELECT context FROM plans WHERE id = ?1",
                [plan.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(ctx) = context {
            let _ = crate::session::tell_manager_of(
                tx,
                Some(&ctx),
                &format!("Plan {plan} cannot reach done: {e}"),
            );
        }
        return Ok(false);
    }
    crate::plan::close_reviews(tx, plan, &crate::plan::Closing::Done)?;
    tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE id = ?1 AND phase = 'deploy'",
        [plan.as_str()],
    )?;
    let text = format!(
        "Every Workflow of Plan {plan} is merged: the Plan is done. See cpd plan show --plan {plan}"
    );
    let mut workflows = merged_workflows(tx, plan)?;
    if let Some((key, reported)) = just {
        for w in workflows.iter_mut().filter(|w| w["key"] == key.as_str()) {
            w["merged_at"] = reported.into();
        }
    }
    append(tx, NewEvent::plan_done(text, workflows).plan(plan.clone()))?;
    Ok(true)
}
