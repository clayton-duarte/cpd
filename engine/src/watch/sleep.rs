//! E13, Sleep (contract §3.7, §2.2 "Asleep"; hs-d515, hs-d520): puts to sleep each Lead and Manager Session that is
//! finished and out of the User's view. Its pane is closed as `session close` does (pi stops, files kept), then one
//! transaction sets `sessions.asleep_at` and appends `session-asleep`. A tick with nothing ready writes nothing.

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use cpd_core::clock::timestamp;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, NewEvent};
use cpd_core::host::chat_slot;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::session::close_lead;

/// What must hold of a Plan besides its phase and its Lead's Inbox: nothing of the Plan is live or open. Aliases the Plan
/// `p` and binds no parameter. [`PLAN_FINISHED`] is shared with E14's Lead-less selection ([`super::archive_leads`]), so
/// the two cannot drift.
macro_rules! plan_nothing_open {
    () => {
        "
       AND NOT EXISTS (SELECT 1 FROM workflows r WHERE r.plan = p.id AND (r.status NOT IN ('merged', 'cancelled')
                                                  OR r.status = 'cancelled' AND EXISTS (SELECT 1 FROM pull_requests q WHERE q.id = r.pull_request AND q.state = 'open')))
       AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = p.id AND j.status IN ('draft', 'queued', 'running', 'failed'))
       AND NOT EXISTS (SELECT 1 FROM escalations e WHERE e.plan = p.id AND e.status = 'open')
       AND NOT EXISTS (SELECT 1 FROM plan_reviews v WHERE v.plan = p.id AND v.status = 'pending')"
    };
}

/// The plan part of "finished": a Plan (alias `p`) with nothing live or open. A Lead-less `shaping` Plan uses this alone,
/// having no Session; a Lead's query adds its own Inbox rule. Aliases `p` only.
pub(super) const PLAN_FINISHED: &str = plan_nothing_open!();

/// What must hold of a Lead's Plan besides its phase: nothing of it is live or open, and its Lead's Session has no open,
/// undismissed Inbox item (snoozed or not). Aliases the Session `s` and its Plan `p`.
macro_rules! nothing_open {
    () => {
        concat!(plan_nothing_open!(), "
       AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.session = s.id AND i.closed_at IS NULL AND i.dismissed_at IS NULL)")
    };
}

/// A Lead's Plan is finished when the Plan is `done` and nothing of it is live or open, and its Lead has no open,
/// undismissed Inbox item (snoozed or not), and no Job of the Plan is `failed`. E13 ([`ready_sql`]) and both selections of E14 ([`super::archive_leads`]) share it, so the two cannot drift.
/// Both queries alias the Session `s` and its Plan `p`, and bind no parameter.
pub(super) const LEAD_FINISHED: &str = concat!("\n       p.phase = 'done'", nothing_open!());

/// pro-d416 (A2): E13 also sleeps the Lead of an open `codereview` Plan that awaits changes (E14 does not archive it).
const LEAD_AWAITING_CHANGES: &str = concat!(
    "\n       (p.phase = 'done' OR p.context = 'codereview' AND p.phase = 'grounding' AND p.changes_requested_at IS NOT NULL)",
    nothing_open!()
);

/// A Lead is finished when its Plan is `done` and nothing of the Plan is live or open; a Manager when every Plan of
/// its Context is `done` or archived (and one is done). `?1` is `now`, `?2` the cut-off: what happened after it is
/// recent. Times are §1.3's, so they compare as text.
///
/// **A reload is not activity** (contract §3.7 E13; air-n103, air-n104): a `cpd install` reloads pi, and the
/// adapter's `stopped` → `idle` round trip (§10.6), and any later refresh, move `state_at` to now. They are no turn,
/// so the Session's clock is not `state_at` but the `at` of its latest `session-state` Event that ends a turn or a
/// wait (`working`, `blocked` or `waiting` → `idle` or `stopped`; A37), as E10 does for a turn. A finished Lead
/// still sleeps after `timeouts.sleep_seconds`.
fn ready_sql() -> String {
    format!(
        "SELECT s.id, s.context, s.plan FROM sessions s LEFT JOIN plans p ON p.id = s.plan
 WHERE s.role IN ('lead', 'manager') AND s.archived_at IS NULL AND s.asleep_at IS NULL
   AND CASE s.role
     WHEN 'lead' THEN {LEAD_AWAITING_CHANGES}
     ELSE EXISTS (SELECT 1 FROM plans q WHERE q.context = s.context AND q.phase = 'done')
       AND NOT EXISTS (SELECT 1 FROM plans q WHERE q.context = s.context AND q.phase <> 'done' AND q.archived_at IS NULL)
       AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.session = s.id AND i.closed_at IS NULL AND i.dismissed_at IS NULL
                          AND (i.snoozed_until IS NULL OR i.snoozed_until <= ?1))
     END
   AND max(
         CASE s.role WHEN 'lead' THEN coalesce(p.done_at, p.changes_requested_at, '')
                     ELSE coalesce((SELECT max(q.done_at) FROM plans q WHERE q.context = s.context AND q.phase = 'done'), '') END,
         coalesce((SELECT max(e.at) FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
                     AND json_extract(e.data, '$.from') IN ('working', 'blocked', 'waiting')
                     AND json_extract(e.data, '$.to') IN ('idle', 'stopped')), ''),
         coalesce((SELECT max(i.closed_at) FROM inbox_items i WHERE i.session = s.id), '')) <= ?2
 ORDER BY s.created_at, s.id"
    )
}

struct Ready {
    session: SessionId,
    context: ContextId,
    plan: Option<PlanId>,
}

pub(super) fn run(engine: &mut Engine) {
    match ready(engine) {
        Ok(ready) => {
            engine.clear(Step::E13, &About::Machine);
            for r in ready {
                let about = About::Session(r.session.clone());
                match sleep(engine, &r) {
                    Ok(true) => {
                        engine
                            .log
                            .line(&format!("sleep: put Session {} to sleep", r.session));
                        engine.clear(Step::E13, &about);
                    }
                    Ok(false) => {}
                    Err(e) => engine.fail(Step::E13, about, &e),
                }
            }
        }
        Err(e) => engine.fail(Step::E13, About::Machine, &e),
    }
}

fn ready(engine: &mut Engine) -> Result<Vec<Ready>, CpdError> {
    let now = engine.ports.clock.utc();
    let cutoff = now - time::Duration::seconds(i64::from(engine.config.timeouts.sleep_seconds));
    let (now, cutoff) = (timestamp(now), timestamp(cutoff));
    engine.store.read(|conn| {
        let mut stmt = conn.prepare(&ready_sql())?;
        let rows = stmt
            .query_map([&now, &cutoff], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(session, context, plan)| {
                Ok(Ready {
                    session: SessionId::parse(&session)?,
                    context: ContextId::parse(&context)?,
                    plan: plan.map(|p| PlanId::parse(&p)).transpose()?,
                })
            })
            .collect()
    })
}

/// Puts one Session to sleep. False when the User is looking at it (the chat slot) or it changed meanwhile.
fn sleep(engine: &mut Engine, r: &Ready) -> Result<bool, CpdError> {
    let cockpit = engine.config.tmux.cockpit_session.clone();
    // A durable Session has no pane (contract §3.6): no tmux call, only the two writes, and its host is stopped
    // first (P1.10, §6.19).
    let durable = engine.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT runner = 'durable' FROM sessions WHERE id = ?1",
            [r.session.as_str()],
            |row| row.get::<_, bool>(0),
        )?)
    })?;
    if durable {
        super::hosts::stop(engine, &r.session);
    } else {
        let panes = engine.tmux.list_panes()?;
        if chat_slot(&panes, &cockpit).is_some_and(|p| p.cpd_session.as_ref() == Some(&r.session)) {
            return Ok(false);
        }
        match close_lead(
            &mut engine.store,
            &engine.tmux,
            &cockpit,
            &r.session,
            SOURCE,
        ) {
            Ok(_) => {}
            Err(e) if e.code() == ErrorCode::InChatSlot => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    engine.store.write(SOURCE, |tx| {
        let changed = tx.execute(
            "UPDATE sessions SET asleep_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND asleep_at IS NULL AND archived_at IS NULL",
            [r.session.as_str()],
        )?;
        if changed == 0 {
            return Ok(false);
        }
        let mut event = NewEvent::session_asleep(format!("Session {} is asleep and out of view.", r.session)).session(r.session.clone()).context(r.context.clone());
        if let Some(plan) = &r.plan {
            event = event.plan(plan.clone());
        }
        events::append(tx, event)?;
        Ok(true)
    })
}
