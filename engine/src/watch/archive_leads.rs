//! E14, the archive of finished Plans (contract §3.7, §2.2 "Archived"; hs-d630, hs-d633): archives the done Plan of
//! each Lead whose `asleep_at` is older than `timeouts.archive_sleep_seconds`, taking the whole Plan through the
//! `plan archive` cascade. A finished codereview Plan follows E13 then E14 like every Plan. A refusal (`archive_blocked`, `in_chat_slot`) is not an error: the Plan is skipped this
//! tick and its Lead stays asleep. Idempotent: after the archive the Lead is not selected again.

use time::OffsetDateTime;

use super::engine::Engine;
use super::report::{About, Step};
use super::sleep::LEAD_FINISHED;
use cpd_core::clock::timestamp;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::plan::archive::{self, Objects};

/// Every Plan with a Lead asleep past the threshold whose Plan is still finished (contract §3.7; hs-d630). `?1` is
/// `now`, `?2` the cut-off `now - timeouts.archive_sleep_seconds`; E13's [`LEAD_FINISHED`] is shared so "finished"
/// cannot drift. A Plan with `deleted_at` set (a tombstone, §2.2 "Deleted") is never a candidate (hs-d716).
/// A finished codereview Plan is no special case: it sleeps via E13 and is archived here after the cut-off, like every Plan.
fn archivable() -> String {
    format!(
        "SELECT p.id FROM sessions s LEFT JOIN plans p ON p.id = s.plan
          WHERE s.role = 'lead' AND s.archived_at IS NULL AND s.asleep_at IS NOT NULL AND s.asleep_at <= ?2
            AND p.deleted_at IS NULL
            AND {LEAD_FINISHED}
         ORDER BY 1"
    )
}

pub(super) fn run(engine: &mut Engine) {
    let plans = match ready(engine) {
        Ok(plans) => {
            engine.clear(Step::E14, &About::Machine);
            plans
        }
        Err(e) => return engine.fail(Step::E14, About::Machine, &e),
    };
    let mut archived = 0usize;
    for plan in plans {
        match archive_plan(engine, &plan) {
            Ok(true) => {
                stop_hosts(engine, &plan);
                archived += 1;
            }
            Ok(false) => {}
            Err(e) => engine.fail(Step::E14, About::Plan(plan), &e),
        }
    }
    if archived > 0 {
        engine
            .log
            .line(&format!("E14: archived {archived} Plan(s)"));
    }
}

/// Stops the hosts of the Plan's durable Lead Sessions (contract §6.19; E14 archives their Plan).
fn stop_hosts(engine: &mut Engine, plan: &PlanId) {
    let sessions: Vec<SessionId> = match engine.store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM sessions WHERE plan = ?1 AND role = 'lead' AND runner = 'durable' ORDER BY id")?;
        Ok(stmt.query_map([plan.as_str()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }) {
        Ok(ids) => ids.iter().filter_map(|id| SessionId::parse(id).ok()).collect(),
        Err(e) => {
            engine.log.line(&format!("E14: cannot read the durable Leads of Plan {plan}: {}", e.message()));
            return;
        }
    };
    for session in sessions {
        super::hosts::stop(engine, &session);
    }
}

fn ready(engine: &mut Engine) -> Result<Vec<PlanId>, CpdError> {
    let now = OffsetDateTime::now_utc();
    let cutoff =
        now - time::Duration::seconds(i64::from(engine.config.timeouts.archive_sleep_seconds));
    let (now, cutoff) = (timestamp(now), timestamp(cutoff));
    engine.store.read(|conn| {
        let mut stmt = conn.prepare(&archivable())?;
        let rows = stmt
            .query_map([&now, &cutoff], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|id| PlanId::parse(id)).collect()
    })
}

/// Archives one Plan through `plan archive`'s cascade (contract §3.7; hs-d630). False when it refused
/// (`archive_blocked`, `in_chat_slot`): the Lead stays asleep and E14 looks again next tick.
fn archive_plan(engine: &mut Engine, plan: &PlanId) -> Result<bool, CpdError> {
    let objects = engine.store.read(|conn| Objects::of_plan(conn, plan))?;
    match archive::archive(&mut engine.store, &engine.config, "engine E14", &objects) {
        Ok(_) => Ok(true),
        Err(e) if matches!(e.code(), ErrorCode::ArchiveBlocked | ErrorCode::InChatSlot) => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}
