//! E20, Parked drafts (contract §3.7 E20, §2.2 "A parked draft"; hs-d731, hs-d733): after E7 and before E8, the Lead of a Plan
//! with parked drafts is told once, by the message of a stuck Job (`from` null, `wake` true), and the drafts of an archived Plan
//! are cancelled. The message is the record that it was sent: nothing more goes until an Event changes a draft's legality.
//! A quiet tick writes nothing.

use rusqlite::Connection;

use super::SOURCE;
use super::engine::{Engine, incident_holds_plan};
use super::report::{About, Step};
use cpd_core::error::CpdError;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::plan::parked_wake::{prefix, wake_state};
use cpd_core::pr::{archived_with_drafts, cancel_archived_drafts};
use cpd_core::session::tell;

pub(super) fn run(engine: &mut Engine) {
    match archived_plans(engine) {
        Ok(plans) => {
            for plan in plans {
                match cancel(engine, &plan) {
                    Ok(()) => engine.log.line(&format!("E20: cancelled the drafts of archived Plan {plan}")),
                    Err(e) => engine.fail(Step::E20, About::Plan(plan), &e),
                }
            }
        }
        Err(e) => engine.fail(Step::E20, About::Machine, &e),
    }
    match leads(engine) {
        Ok(leads) => {
            for (plan, lead) in leads {
                match wake(engine, &plan, &lead) {
                    Ok(true) => {
                        engine.log.line(&format!("E20: woke the Lead of Plan {plan}: its drafts are parked"));
                        engine.clear(Step::E20, &About::Plan(plan));
                    }
                    Ok(false) => engine.clear(Step::E20, &About::Plan(plan)),
                    Err(e) => engine.fail(Step::E20, About::Plan(plan), &e),
                }
            }
            engine.clear(Step::E20, &About::Machine);
        }
        Err(e) => engine.fail(Step::E20, About::Machine, &e),
    }
}

fn archived_plans(engine: &mut Engine) -> Result<Vec<PlanId>, CpdError> {
    engine.store.read(|conn| archived_with_drafts(conn))
}

/// W2: one transaction per Plan, which writes nothing when the Plan has no draft left.
fn cancel(engine: &mut Engine, plan: &PlanId) -> Result<(), CpdError> {
    engine.store.write(SOURCE, |tx| cancel_archived_drafts(tx, plan))
}

/// The live Plans not held by an incident whose Lead is a live Lead in state `idle`.
fn leads(engine: &mut Engine) -> Result<Vec<(PlanId, SessionId)>, CpdError> {
    engine.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.lead_session FROM plans p JOIN sessions s ON s.id = p.lead_session
              WHERE p.archived_at IS NULL AND s.role = 'lead' AND s.archived_at IS NULL AND s.state = 'idle' ORDER BY p.created_at, p.id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut leads = Vec::new();
        for (plan, lead) in rows {
            let plan = PlanId::parse(&plan)?;
            if !incident_holds_plan(conn, &plan) {
                leads.push((plan, SessionId::parse(&lead)?));
            }
        }
        Ok(leads)
    })
}

/// The message of W1 for the parked `keys`, sorted.
pub(super) fn text(plan: &PlanId, keys: &[String]) -> String {
    let mut keys = keys.to_vec();
    keys.sort();
    format!(
        "{}{} (no pending Plan review, no unfinished need). Resolve each now: edit it to a full Spec and Tier and call cpd_plan_review; \
         or cancel it and add it queued when it only carries out a recorded decision; or cancel it (cpd job cancel <k> --reason \"<why>\"). \
         Nothing runs it otherwise.",
        prefix(plan),
        keys.join(", ")
    )
}

/// Whether the Lead is to be told now, and the message when so: parked drafts and no message newer than the last change.
fn due(conn: &Connection, plan: &PlanId, lead: &SessionId) -> Result<Option<String>, CpdError> {
    let state = wake_state(conn, plan, lead.as_str())?;
    let sent = state.wake.is_some_and(|w| w >= state.t);
    Ok((!state.parked.is_empty() && !sent).then(|| text(plan, &state.parked)))
}

/// W1: whether it sent the message. The condition is read again inside the transaction that sends.
fn wake(engine: &mut Engine, plan: &PlanId, lead: &SessionId) -> Result<bool, CpdError> {
    if engine.store.read(|conn| due(conn, plan, lead))?.is_none() {
        return Ok(false);
    }
    engine.store.write(SOURCE, |tx| match due(tx, plan, lead)? {
        Some(text) => tell(tx, lead, &text).map(|_| true),
        None => Ok(false),
    })
}
