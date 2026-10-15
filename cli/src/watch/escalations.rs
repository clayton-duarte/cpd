//! E1, Escalations (contract §3.5, §3.7): expire every `open` Escalation
//! whose asker, the blocking `cpd2 ask`, is no longer alive
//! (`escalation-expired` `{escalation, reason: "asker_dead"}`, commands.md §8.1).

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use serde_json::json;

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use crate::error::CpdError;
use crate::events::{self, Kind, NewEvent};
use crate::ids::{EscalationId, PlanId};

struct Open {
    plan: PlanId,
    id: EscalationId,
    asker_pid: i64,
}

pub(super) fn run(engine: &mut Engine) {
    let open = engine.store.read(|conn| {
        let mut stmt = conn.prepare("SELECT plan, id, asker_pid FROM escalations WHERE status = 'open' ORDER BY plan, id")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(plan, id, asker_pid)| Ok(Open { plan: PlanId::parse(&plan)?, id: EscalationId::parse(&id)?, asker_pid }))
            .collect::<Result<Vec<_>, CpdError>>()
    });
    let open = match open {
        Ok(open) => {
            engine.clear(Step::E1, &About::Machine);
            open
        }
        Err(e) => return engine.fail(Step::E1, About::Machine, &e),
    };
    for escalation in open.into_iter().filter(|e| !alive(e.asker_pid)) {
        let about = About::Attempt(escalation.plan.clone(), escalation.id.attempt().clone());
        match expire(engine, &escalation) {
            Ok(expired) => {
                if expired {
                    engine.log.line(&format!("E1: expired {} of Plan {}: its asker is gone", escalation.id, escalation.plan));
                }
                engine.clear(Step::E1, &about);
            }
            Err(e) => engine.fail(Step::E1, about, &e),
        }
    }
}

/// Whether process `pid` exists. `EPERM` means it exists under another user.
fn alive(pid: i64) -> bool {
    let Ok(raw) = i32::try_from(pid) else { return false };
    if raw <= 0 {
        return false;
    }
    !matches!(kill(Pid::from_raw(raw), None), Err(Errno::ESRCH))
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
            "Escalation {id} of Attempt {attempt} expired: its asker (pid {}) is gone. See: cpd2 attempt show {attempt} --plan {plan}",
            escalation.asker_pid
        );
        let data = json!({"escalation": id.to_string(), "reason": "asker_dead"});
        events::append(
            tx,
            NewEvent::new(Kind::EscalationExpired, text, data).plan(plan.clone()).job(attempt.job().clone()).attempt(attempt.clone()),
        )?;
        Ok(true)
    })
}
