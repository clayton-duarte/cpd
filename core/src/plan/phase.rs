//! `plan phase plan` (commands.md §4.4, contract §3.1): `grounding → plan` or a paired shaping Plan's `shaping → plan`. It
//! writes the phase and `plan-phase` and nothing else: the grounded Lead is
//! retired (hs-d92), so no Session is registered and no window is created.

use serde_json::json;

use super::row::{self, Phase, PlanRow};
use super::{Env, PhaseArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::output::Output;

const COMMAND: &str = "plan phase";

fn illegal(plan: &PlanRow, to: Phase) -> CpdError {
    CpdError::new(
        ErrorCode::IllegalTransition,
        format!(
            "Plan {} cannot move from {} to {} by hand: only grounding or paired shaping → plan is allowed (§3.1).",
            plan.id,
            plan.phase.as_str(),
            to.as_str()
        ),
    )
    .with("plan", plan.id.as_str())
    .with("from", plan.phase.as_str())
    .with("to", to.as_str())
}

pub(super) fn phase(args: PhaseArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let plan = env.store.read(|conn| row::load(conn, &id))?;
    if plan.phase == Phase::Shaping {
        env.store.read(|conn| super::shaping::guard(conn, &id))?;
    }
    if args.phase != Phase::Plan {
        return Err(illegal(&plan, args.phase));
    }
    match plan.phase {
        Phase::Grounding => {}
        Phase::Shaping if plan.lead_session.is_some() => {}
        Phase::Plan => {
            return Ok(Output::success(
                json!({"plan": plan.id, "phase": "plan", "already": true}),
            ));
        }
        _ => return Err(illegal(&plan, Phase::Plan)),
    }
    env.store.write(COMMAND, |tx| {
        let mut stmt = tx.prepare("SELECT key, kind FROM jobs WHERE plan = ?1 AND status IN ('queued', 'running', 'failed') ORDER BY key")?;
        let pending: Vec<String> = stmt
            .query_map([id.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(_, kind)| super::jobs::is_explore(kind))
            .map(|(key, _)| key)
            .collect();
        drop(stmt);
        if !pending.is_empty() {
            return Err(CpdError::new(
                ErrorCode::ExplorePending,
                format!("Plan {id} has explore Jobs not merged yet: {}. Wait for their job-merged Events, or retry or cancel a failed one.", pending.join(", ")),
            )
            .with("plan", id.as_str())
            .with("jobs", pending));
        }
        // The phase, with a compare-and-swap on the phase read.
        let from = plan.phase.as_str();
        let moved = tx.execute("UPDATE plans SET phase = 'plan' WHERE id = ?1 AND phase = ?2", [id.as_str(), from])?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {id} left phase {from} meanwhile: rerun the command."))
                .with("plan", id.as_str()));
        }
        let text = format!("Plan {id} moved from {from} to plan. Next: cpd job add <key> --title <text> --tier <T> --spec <file>, then cpd plan review");
        append(tx, NewEvent::plan_phase(text, from, "plan").plan(id.clone()))?;
        Ok(())
    })?;
    Ok(Output::success(json!({"plan": id, "phase": "plan"})))
}
