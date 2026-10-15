//! E2, liveness (contract §3.4, §3.6, §3.7, §7.4):
//! - an Attempt not `ended` whose driver lock is free (its driver died): in
//!   `merging`, the merge recovery R under the per-repo lock; in any other
//!   status, ended `cancelled` if a cancel was requested, else `crashed`;
//! - an Attempt with a live driver: `stop_request = cancelled` when its Job
//!   has a cancel request, `timed_out` when its active time is over
//!   `attempt_minutes`; the driver acts on it (A5);
//! - a Session not `stopped` whose pane is dead or missing: `stopped`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::params;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::SOURCE;
use super::engine::{Engine, metrics};
use super::report::{About, Step};
use crate::driver::ledger::{self, Cause, End, Ladder};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::host::layout::attempt_window;
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::merge::{self, DriverLock, RepoLock, take_free_driver_lock};

/// An Attempt not `ended`, as E2 reads it.
struct Active {
    plan: PlanId,
    id: AttemptId,
    status: String,
    started_at: String,
    stop_request: Option<String>,
    cancel_requested: bool,
    repo: PathBuf,
}

impl Active {
    fn about(&self) -> About {
        About::Attempt(self.plan.clone(), self.id.clone())
    }
}

pub(super) fn run(engine: &mut Engine) {
    attempts(engine);
    sessions(engine);
}

fn attempts(engine: &mut Engine) {
    let active = engine.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.plan, a.id, a.status, a.started_at, a.stop_request, j.cancel_requested_at IS NOT NULL, p.repo
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job JOIN plans p ON p.id = a.plan
              WHERE a.status <> 'ended' ORDER BY a.plan, a.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(plan, id, status, started_at, stop_request, cancel_requested, repo)| {
                Ok(Active {
                    plan: PlanId::parse(&plan)?,
                    id: AttemptId::parse(&id)?,
                    status,
                    started_at,
                    stop_request,
                    cancel_requested,
                    repo: PathBuf::from(repo),
                })
            })
            .collect::<Result<Vec<_>, CpdError>>()
    });
    let active = match active {
        Ok(active) => {
            engine.clear(Step::E2, &About::Machine);
            active
        }
        Err(e) => return engine.fail(Step::E2, About::Machine, &e),
    };

    // The repos with a `merging` Attempt whose driver died, and a Plan to report against.
    let mut orphaned: BTreeMap<PathBuf, PlanId> = BTreeMap::new();
    for attempt in &active {
        let probe = take_free_driver_lock(&engine.home, &attempt.plan, &attempt.id);
        let result = match probe {
            // Released at once: R takes it again itself.
            Ok(Some(_free)) if attempt.status == "merging" => {
                orphaned.entry(attempt.repo.clone()).or_insert_with(|| attempt.plan.clone());
                Ok(())
            }
            Ok(Some(lock)) => end_dead(engine, attempt, lock),
            Ok(None) => stop_request(engine, attempt),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => engine.clear(Step::E2, &attempt.about()),
            Err(e) => engine.fail(Step::E2, attempt.about(), &e),
        }
    }
    for (repo, plan) in orphaned {
        let about = About::Plan(plan);
        match recover(engine, &repo) {
            Ok(()) => engine.clear(Step::E2, &about),
            Err(e) => engine.fail(Step::E2, about, &e),
        }
    }
}

/// The driver of `active` (`starting` or `running`) is dead: end the Attempt
/// `cancelled` if its Job has a cancel request, else `failed` / `crashed` (a
/// ladder cause), as the driver would (§3.4), then kill its window (A9).
/// Holds the driver lock meanwhile, so no one else ends it at the same time.
fn end_dead(engine: &mut Engine, active: &Active, lock: DriverLock) -> Result<(), CpdError> {
    let (plan, id) = (&active.plan, &active.id);
    let target = engine.store.read(|conn| ledger::load(conn, plan, id))?;
    if !matches!(target.status.as_str(), "starting" | "running") {
        return Ok(());
    }
    let cancel = engine.store.read(|conn| ledger::cancel_request(conn, plan, &target.job))?;
    let end = match cancel {
        Some(_) => End::Cancelled,
        None => End::Failed { cause: Cause::Crashed, detail: format!("Its driver died while it was {}.", target.status) },
    };
    let catalog = engine.catalog()?;
    let ladder = Ladder { catalog: &catalog, retry_cap: engine.config.retry_cap };
    let metrics = metrics(target.worker_session.as_ref());
    engine.store.write(SOURCE, |tx| ledger::end(tx, &target, &end, metrics.as_ref(), &ladder))?;
    drop(lock);
    engine.log.line(&format!("E2: the driver of {id} (Plan {plan}) is dead: ended {} / {}", end.outcome(), end.cause()));
    let name = attempt_window(plan, id);
    let work = engine.config.tmux.work_session.clone();
    for pane in engine.tmux.list_panes()? {
        if pane.session_name == work && pane.window_name == name {
            engine.tmux.kill_window(&pane.window)?;
        }
    }
    Ok(())
}

/// R (§7.4) for the orphaned merges of `repo`, when its per-repo lock is free
/// now; otherwise at a later tick, or by the next holder of the lock.
fn recover(engine: &mut Engine, repo: &std::path::Path) -> Result<(), CpdError> {
    let Some(lock) = RepoLock::try_take(&engine.home, repo)? else { return Ok(()) };
    let catalog = engine.catalog()?;
    let metrics = |a: &merge::Attempt| metrics(a.worker_session.as_ref());
    let ctx = merge::Ctx { home: &engine.home, config: &engine.config, catalog: &catalog, source: SOURCE, metrics: &metrics };
    let recovered = merge::recover(&mut engine.store, &ctx, &lock)?;
    drop(lock);
    for r in recovered {
        let how = r.end.map_or_else(|| "nothing changed: git failed".to_owned(), |e| format!("ended {} / {}", e.outcome(), e.cause()));
        engine.log.line(&format!("E2: recovered the merge of {} (Plan {}): {how}", r.attempt, r.plan));
    }
    Ok(())
}

/// A live driver: `stop_request = cancelled` when the Job has a cancel
/// request, else `timed_out` when a `running` Attempt's active time is over
/// `attempt_minutes`. A data write for the driver, no Event (§5.1): the
/// driver's end appends it. A request already set is left alone; a `merging`
/// Attempt is the merge's, which checks for a cancel itself (§7.4).
fn stop_request(engine: &mut Engine, active: &Active) -> Result<(), CpdError> {
    if active.stop_request.is_some() || !matches!(active.status.as_str(), "starting" | "running") {
        return Ok(());
    }
    let request = if active.cancel_requested {
        "cancelled"
    } else {
        let limit = Duration::from_secs(u64::from(engine.config.timeouts.attempt_minutes) * 60);
        if active.status != "running" || active_time(engine, active)? <= limit {
            return Ok(());
        }
        "timed_out"
    };
    let (plan, id) = (active.plan.as_str(), active.id.to_string());
    let changed = engine.store.write(SOURCE, |tx| {
        Ok(tx.execute(
            "UPDATE attempts SET stop_request = ?3 WHERE plan = ?1 AND id = ?2 AND status = ?4 AND stop_request IS NULL",
            params![plan, id, request, active.status],
        )?)
    })?;
    if changed == 1 {
        engine.log.line(&format!("E2: asked the driver of {id} (Plan {plan}) to stop: {request}"));
    }
    Ok(())
}

/// Wall time since the Attempt started, minus the time an Escalation of it
/// was `open` (§3.3, §3.4 A4). Overlapping Escalations count once.
fn active_time(engine: &mut Engine, active: &Active) -> Result<Duration, CpdError> {
    let (plan, id) = (active.plan.as_str(), active.id.to_string());
    let open: Vec<(String, Option<String>)> = engine.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT asked_at, coalesce(answered_at, expired_at) FROM escalations WHERE plan = ?1 AND attempt = ?2 ORDER BY asked_at",
        )?;
        let rows = stmt.query_map([plan, &id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let now = OffsetDateTime::now_utc();
    let started = parse_time(&active.started_at)?;
    let mut intervals = Vec::with_capacity(open.len());
    for (asked, closed) in &open {
        let from = parse_time(asked)?.max(started);
        let to = closed.as_deref().map(parse_time).transpose()?.unwrap_or(now).min(now);
        if to > from {
            intervals.push((from, to));
        }
    }
    let paused = union_length(intervals);
    let active = (now - started) - paused;
    Ok(Duration::try_from(active).unwrap_or(Duration::ZERO))
}

/// The total length of the union of `intervals`.
fn union_length(mut intervals: Vec<(OffsetDateTime, OffsetDateTime)>) -> time::Duration {
    intervals.sort();
    let mut total = time::Duration::ZERO;
    let mut current: Option<(OffsetDateTime, OffsetDateTime)> = None;
    for (from, to) in intervals {
        current = match current {
            Some((a, b)) if from <= b => Some((a, b.max(to))),
            Some((a, b)) => {
                total += b - a;
                Some((from, to))
            }
            None => Some((from, to)),
        };
    }
    if let Some((a, b)) = current {
        total += b - a;
    }
    total
}

fn parse_time(text: &str) -> Result<OffsetDateTime, CpdError> {
    OffsetDateTime::parse(text, &Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The Ledger holds a time that is not RFC 3339: {text:?} ({e}).")).with("time", text)
    })
}

/// A Session not `stopped`, as read before the panes are listed.
struct Live {
    id: SessionId,
    state: String,
    pane: Option<String>,
    name: String,
    plan: Option<PlanId>,
}

/// Every Session not `stopped` whose pane is dead or missing becomes
/// `stopped` (§3.6). The Sessions are read before the panes are listed, and
/// each write is a compare-and-swap on the state and pane read, so a Session
/// that got a new pane meanwhile is left alone.
fn sessions(engine: &mut Engine) {
    let live = engine.store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id, state, tmux_pane, name, plan FROM sessions WHERE state <> 'stopped' ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, state, pane, name, plan)| {
                Ok(Live { id: SessionId::parse(&id)?, state, pane, name, plan: plan.map(|p| PlanId::parse(&p)).transpose()? })
            })
            .collect::<Result<Vec<_>, CpdError>>()
    });
    let panes = live.and_then(|live| Ok((live, engine.tmux.list_panes()?)));
    let (live, panes) = match panes {
        Ok(found) => {
            engine.clear(Step::E2, &About::Machine);
            found
        }
        Err(e) => return engine.fail(Step::E2, About::Machine, &e),
    };
    for session in live {
        let found = session.pane.as_deref().and_then(|p| panes.iter().find(|pane| pane.pane.as_str() == p));
        let why = match (&session.pane, found) {
            (_, Some(pane)) if !pane.dead => continue,
            (Some(pane), Some(_)) => format!("its pane {pane} is dead"),
            (Some(pane), None) => format!("its pane {pane} is gone"),
            (None, _) => "it has no pane".to_owned(),
        };
        let about = About::Session(session.id.clone());
        match stop(engine, &session, &why) {
            Ok(()) => engine.clear(Step::E2, &about),
            Err(e) => engine.fail(Step::E2, about, &e),
        }
    }
}

fn stop(engine: &mut Engine, session: &Live, why: &str) -> Result<(), CpdError> {
    let changed = engine.store.write(SOURCE, |tx| {
        let changed = tx.execute(
            "UPDATE sessions SET state = 'stopped', state_label = NULL, state_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE id = ?1 AND state = ?2 AND tmux_pane IS ?3",
            params![session.id.as_str(), session.state, session.pane],
        )?;
        if changed == 0 {
            return Ok(false);
        }
        let text = format!("Session {} is stopped: {why}.", session.name);
        let event = NewEvent::new(Kind::SessionState, text, json!({"from": session.state, "to": "stopped", "label": null}))
            .session(session.id.clone());
        events::append(tx, match &session.plan {
            Some(plan) => event.plan(plan.clone()),
            None => event,
        })?;
        Ok(true)
    })?;
    if changed {
        engine.log.line(&format!("E2: Session {} ({}) is stopped: {why}", session.id, session.name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn overlapping_escalations_pause_the_clock_once() {
        let t = |m: i64| datetime!(2026-10-14 10:00 UTC) + time::Duration::minutes(m);
        assert_eq!(union_length(vec![]), time::Duration::ZERO);
        assert_eq!(union_length(vec![(t(0), t(10)), (t(5), t(15)), (t(20), t(25))]), time::Duration::minutes(20));
        assert_eq!(union_length(vec![(t(20), t(25)), (t(0), t(10))]), time::Duration::minutes(15));
    }

    #[test]
    fn times_are_rfc_3339() {
        assert!(parse_time("2026-10-14T21:10:00.000Z").is_ok());
        assert_eq!(parse_time("t").unwrap_err().code(), ErrorCode::StoreFailed);
    }
}
