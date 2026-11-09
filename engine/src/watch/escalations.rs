//! E1, Escalations (contract §3.5, §3.7): expire every `open` Escalation
//! whose asker, the blocking `cpd ask`, is no longer alive
//! (`escalation-expired` `{escalation, reason: "asker_dead"}`, commands.md §8.1).

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use cpd_core::error::CpdError;
use cpd_core::events::{self, NewEvent};
use cpd_core::host::spawn::Pid;
use cpd_core::ids::{EscalationId, PlanId};

struct Open {
    plan: PlanId,
    id: EscalationId,
    asker_pid: i64,
}

pub(super) fn run(engine: &mut Engine) {
    let open = engine.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT plan, id, asker_pid FROM escalations WHERE status = 'open' ORDER BY plan, id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(plan, id, asker_pid)| {
                Ok(Open {
                    plan: PlanId::parse(&plan)?,
                    id: EscalationId::parse(&id)?,
                    asker_pid,
                })
            })
            .collect::<Result<Vec<_>, CpdError>>()
    });
    let open = match open {
        Ok(open) => {
            engine.clear(Step::E1, &About::Machine);
            open
        }
        Err(e) => return engine.fail(Step::E1, About::Machine, &e),
    };
    let spawn = engine.ports.spawn.clone();
    for escalation in open
        .into_iter()
        .filter(|e| !u32::try_from(e.asker_pid).is_ok_and(|pid| spawn.alive(Pid(pid))))
    {
        let about = About::Attempt(escalation.plan.clone(), escalation.id.attempt().clone());
        match expire(engine, &escalation) {
            Ok(expired) => {
                if expired {
                    engine.log.line(&format!(
                        "escalations: expired {} of Plan {}: its asker is gone",
                        escalation.id, escalation.plan
                    ));
                }
                engine.clear(Step::E1, &about);
            }
            Err(e) => engine.fail(Step::E1, about, &e),
        }
    }
}

/// `open → expired`, compare-and-swap on `open`; false when it was answered
/// or expired meanwhile.
fn expire(engine: &mut Engine, escalation: &Open) -> Result<bool, CpdError> {
    let (plan, id) = (&escalation.plan, &escalation.id);
    let attempt = id.attempt();
    engine.store.write(SOURCE, |tx| {
        let changed = tx.execute(
            "UPDATE escalations SET status = 'expired', expired_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND id = ?2 AND status = 'open'",
            [plan.as_str(), &id.to_string()],
        )?;
        if changed == 0 {
            return Ok(false);
        }
        let text = format!(
            "Escalation {id} of Attempt {attempt} expired: its asker (pid {}) is gone. See: cpd attempt show {attempt} --plan {plan}",
            escalation.asker_pid
        );
        events::append(
            tx,
            NewEvent::escalation_expired(text, id.to_string(), "asker_dead").plan(plan.clone()).job(attempt.job().clone()).attempt(attempt.clone()),
        )?;
        cpd_core::ask::resume_attempt(tx, plan, attempt)?;
        Ok(true)
    })
}
