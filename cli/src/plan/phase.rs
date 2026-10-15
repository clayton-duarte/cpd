//! `plan phase plan` (commands.md §4.4, contract §3.1): `grounding → plan`,
//! with the compare-and-swap that gives the Context its grounded Lead.

use std::path::Path;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::lead::{self, Lead};
use super::row::{self, Phase, PlanRow};
use super::{Env, PhaseArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::host::layout::context_window;
use crate::ids::SessionId;
use crate::output::Output;

const COMMAND: &str = "plan phase";

fn illegal(plan: &PlanRow, to: Phase) -> CpdError {
    CpdError::new(
        ErrorCode::IllegalTransition,
        format!("Plan {} cannot move from {} to {} by hand: only grounding → plan is (§3.1).", plan.id, plan.phase.as_str(), to.as_str()),
    )
    .with("plan", plan.id.as_str())
    .with("from", plan.phase.as_str())
    .with("to", to.as_str())
}

/// The grounded Lead this transaction registered.
struct Grounded {
    session: SessionId,
    name: String,
    cwd: String,
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
        Phase::Plan => return already(&mut env, &plan),
        _ => return Err(illegal(&plan, Phase::Plan)),
    }

    let tier = env.config.lead_tier.to_string();
    let grounded = env.store.write(COMMAND, |tx| {
        // 1. The phase, with a compare-and-swap on the phase read.
        let moved = tx.execute("UPDATE plans SET phase = 'plan' WHERE id = ?1 AND phase = 'grounding'", [id.as_str()])?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {id} left phase grounding meanwhile: rerun the command."))
                .with("plan", id.as_str()));
        }
        // 2. The Context's grounded Lead, if it has none on this machine.
        let new = SessionId::new_v4();
        let grounded = tx.execute(
            "UPDATE contexts SET lead_session = ?1,
                    grounded_seq = (SELECT coalesce(max(seq), 0) FROM context_records WHERE context = ?2)
              WHERE id = ?2 AND lead_session IS NULL",
            [new.as_str(), plan.context.as_str()],
        )?;
        let text = format!("Plan {id} moved from grounding to plan. Next: cpd2 job add <key> --title <text> --tier <T> --spec <file>, then cpd2 plan review");
        append(tx, NewEvent::new(Kind::PlanPhase, text, json!({"from": "grounding", "to": "plan"})).plan(id.clone()))?;
        if grounded == 0 {
            return Ok(None);
        }
        // 3. Its Session row; 4. its Events.
        let cwd: String = tx.query_row("SELECT cwd FROM sessions WHERE id = ?1", [plan.lead_session.as_str()], |r| r.get(0))?;
        let name = format!("{} (grounded)", plan.context);
        tx.execute(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                   state, state_label, state_at, created_at)
             SELECT ?1, 'lead', ?2, NULL, NULL, ?3, NULL, ?4, ?5, ?6, NULL, 'stopped', NULL, at, at
               FROM txns WHERE id = cpd_txn()",
            rusqlite::params![new.as_str(), plan.context.as_str(), name, plan.lead_session.as_str(), cwd, tier],
        )?;
        let seq: i64 = tx.query_row("SELECT grounded_seq FROM contexts WHERE id = ?1", [plan.context.as_str()], |r| r.get(0))?;
        let text = format!("Grounded Lead {name} of Context {} registered, a fork of Plan {id}'s Lead.", plan.context);
        let data = json!({"role": "lead", "name": name, "forked_from": plan.lead_session, "tier": tier});
        append(tx, NewEvent::new(Kind::SessionRegistered, text, data).context(plan.context.clone()).session(new.clone()))?;
        let text = format!(
            "Context {} is grounded: new Plans fork {name}, with the records after index position {seq}.",
            plan.context
        );
        let data = json!({"seq": seq, "forked_from": plan.lead_session});
        append(tx, NewEvent::new(Kind::ContextGrounded, text, data).context(plan.context.clone()).session(new.clone()))?;
        Ok(Some(Grounded { session: new, name, cwd }))
    })?;

    // After the commit: the grounded Lead's window; its `session run` forks
    // this Lead once it is idle (§6.7).
    let grounded_lead = match grounded {
        None => Value::Null,
        Some(g) => start(&mut env, &plan, &g)?,
    };
    Ok(Output::success(json!({"plan": id, "phase": "plan", "grounded_lead": grounded_lead})))
}

fn start(env: &mut Env, plan: &PlanRow, g: &Grounded) -> Result<Value, CpdError> {
    let lead = Lead { session: &g.session, context: &plan.context, plan: None, name: &g.name, cwd: Path::new(&g.cwd) };
    let started = lead::start(env, &lead, &context_window(&plan.context), COMMAND)?;
    Ok(json!({"session": g.session, "pane": started.pane.as_str(), "window": started.window.as_str()}))
}

/// Already in phase `plan`: `"already": true`; when the Context's grounded
/// Lead was forked from this Plan's Lead and has no pane (a crash between the
/// two transactions), its window is created now.
fn already(env: &mut Env, plan: &PlanRow) -> Result<Output, CpdError> {
    let found: Option<(String, String, String, Option<String>, String)> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT s.id, s.name, s.cwd, s.tmux_pane, s.state FROM contexts c JOIN sessions s ON s.id = c.lead_session
                  WHERE c.id = ?1 AND s.forked_from = ?2",
                [plan.context.as_str(), plan.lead_session.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?)
    })?;
    let grounded_lead = match found {
        None => Value::Null,
        Some((session, name, cwd, pane, state)) => {
            let session = SessionId::parse(&session)?;
            match pane {
                None if state == "stopped" => start(env, plan, &Grounded { session, name, cwd })?,
                pane => {
                    let window = env.store.read(|conn| lead::last_window(conn, &session))?;
                    json!({"session": session, "pane": pane, "window": window})
                }
            }
        }
    };
    Ok(Output::success(json!({"plan": plan.id, "phase": "plan", "grounded_lead": grounded_lead, "already": true})))
}
