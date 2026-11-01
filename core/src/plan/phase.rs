//! `plan phase plan` (commands.md §4.4, contract §3.1): `grounding → plan`. It
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
            "Plan {} cannot move from {} to {} by hand: only grounding → plan is (§3.1).",
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
    if args.phase != Phase::Plan {
        return Err(illegal(&plan, args.phase));
    }
    match plan.phase {
        Phase::Grounding => {}
        Phase::Plan => {
            return Ok(Output::success(
                json!({"plan": plan.id, "phase": "plan", "already": true}),
            ));
        }
        _ => return Err(illegal(&plan, Phase::Plan)),
    }
    env.store.write(COMMAND, |tx| {
        // The phase, with a compare-and-swap on the phase read.
        let moved = tx.execute("UPDATE plans SET phase = 'plan' WHERE id = ?1 AND phase = 'grounding'", [id.as_str()])?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {id} left phase grounding meanwhile: rerun the command."))
                .with("plan", id.as_str()));
        }
        let text = format!("Plan {id} moved from grounding to plan. Next: cpd job add <key> --title <text> --tier <T> --spec <file>, then cpd plan review");
        append(tx, NewEvent::plan_phase(text, "grounding", "plan").plan(id.clone()))?;
        Ok(())
    })?;
    Ok(Output::success(json!({"plan": id, "phase": "plan"})))
}
