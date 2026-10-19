//! E2, liveness (contract §3.4, §3.6, §3.7, §7.4):
//! - an Attempt not `ended` whose driver lock is free (its driver died): in
//!   `merging`, the merge recovery R under the per-repo lock; in any other
//!   status, ended `cancelled` if a cancel was requested, else `crashed`;
//! - an Attempt with a live driver: `stop_request = cancelled` when its Job
//!   has a cancel request, `timed_out` when its active time is over
//!   `attempt_minutes`; the driver acts on it (A5);
//! - a Session whose pane is dead or missing: `stopped`, with no pane
//!   (`session::pane_gone`); a Session not `stopped` with no pane: `stopped`;
//!   and a Lead's dead pane is cleaned up (G2): its park or ctx window is
//!   killed, or, in the chat slot, the pane becomes the placeholder (§3.6).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::SOURCE;
use super::engine::{Engine, metrics};
use super::report::{About, Step};
use crate::attempt::ledger::{self, Cause, End, Ladder, Stage};
use crate::driver::lock::{self, Free};
use crate::driver::worker;
use crate::error::{CpdError, ErrorCode};
use crate::host::layout::{MAIN_WINDOW, View};
use crate::host::tmux::{OPT_SESSION, Pane, Spawn};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::merge::{self, RepoLock};

/// An Attempt not `ended`, as E2 reads it.
struct Active {
    plan: PlanId,
    id: AttemptId,
    status: String,
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
            "SELECT a.plan, a.id, a.status, a.stop_request, j.cancel_requested_at IS NOT NULL, r.repo
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job JOIN prs r ON r.plan = j.plan AND r.key = j.pr
              WHERE a.status <> 'ended' ORDER BY a.plan, a.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(plan, id, status, stop_request, cancel_requested, repo)| {
                Ok(Active {
                    plan: PlanId::parse(&plan)?,
                    id: AttemptId::parse(&id)?,
                    status,
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
        let probe = lock::probe(&engine.home, &attempt.plan, &attempt.id);
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
fn end_dead(engine: &mut Engine, active: &Active, lock: Free) -> Result<(), CpdError> {
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
    engine.store.write(SOURCE, |tx| ledger::end(tx, &target, &end, metrics.as_ref(), &ladder, Stage::Driver))?;
    // A9: after the end is committed, while the lock is still held.
    lock::remove(&engine.home, plan, id)?;
    drop(lock);
    engine.log.line(&format!("E2: the driver of {id} (Plan {plan}) is dead: ended {} / {}", end.outcome(), end.cause()));
    worker::kill_attempt_window(&engine.config, &engine.tmux, plan, id)
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

/// Wall time since the Attempt turned `running` (the time of its `starting →
/// running` `attempt-status` Event: the wait in `starting`, a review Job's wait
/// for its Lead, is not counted), minus the time an Escalation of it was
/// `open` (§3.3, §3.4 A4). Overlapping Escalations count once.
fn active_time(engine: &mut Engine, active: &Active) -> Result<Duration, CpdError> {
    let (plan, id) = (active.plan.as_str(), active.id.to_string());
    let (running, open): (Option<String>, Vec<(String, Option<String>)>) = engine.store.read(|conn| {
        let running = conn
            .query_row(
                "SELECT at FROM events WHERE plan = ?1 AND attempt = ?2 AND kind = 'attempt-status' AND json_extract(data, '$.to') = 'running'
                  ORDER BY id LIMIT 1",
                [plan, &id],
                |r| r.get(0),
            )
            .optional()?;
        let mut stmt = conn.prepare(
            "SELECT asked_at, coalesce(answered_at, expired_at) FROM escalations WHERE plan = ?1 AND attempt = ?2 ORDER BY asked_at",
        )?;
        let rows = stmt.query_map([plan, &id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((running, rows))
    })?;
    let running = running.ok_or_else(|| {
        CpdError::new(ErrorCode::StoreFailed, format!("Attempt {id} is running but the Ledger has no `running` attempt-status Event for it."))
            .with("attempt", id.clone())
    })?;
    let mut escalations = Vec::with_capacity(open.len());
    for (asked, closed) in &open {
        escalations.push((parse_time(asked)?, closed.as_deref().map(parse_time).transpose()?));
    }
    Ok(active_between(parse_time(&running)?, OffsetDateTime::now_utc(), &escalations))
}

/// The time from `running` to `now`, minus the union of the Escalation
/// intervals (asked, closed; none while still `open`) clipped to that span.
fn active_between(running: OffsetDateTime, now: OffsetDateTime, escalations: &[(OffsetDateTime, Option<OffsetDateTime>)]) -> Duration {
    let mut intervals = Vec::with_capacity(escalations.len());
    for &(asked, closed) in escalations {
        let from = asked.max(running);
        let to = closed.unwrap_or(now).min(now);
        if to > from {
            intervals.push((from, to));
        }
    }
    let active = (now - running) - union_length(intervals);
    Duration::try_from(active).unwrap_or(Duration::ZERO)
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

/// A Session not `stopped`, or with a pane, as read before the panes are listed.
struct Live {
    id: SessionId,
    role: String,
    state: String,
    pane: Option<String>,
    name: String,
}

/// Every Session whose pane is dead or missing becomes `stopped` with no pane,
/// and every Session not `stopped` with no pane becomes `stopped` (§3.6, §5.2;
/// Escalation `e2e-a1-q1`): `session::pane_gone` does both. A `stopped`
/// Session keeps a live pane (it was registered before pi starts, or its
/// `session run` waits to resume pi). The Sessions are read before the panes
/// are listed, and each write is a compare-and-swap on the state and pane
/// read, so a Session that got a new pane meanwhile is left alone.
fn sessions(engine: &mut Engine) {
    let live = engine.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, role, state, tmux_pane, name FROM sessions WHERE state <> 'stopped' OR tmux_pane IS NOT NULL ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, role, state, pane, name)| Ok(Live { id: SessionId::parse(&id)?, role, state, pane, name }))
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
        let dead_lead = found.filter(|pane| pane.dead && session.role == "lead");
        match stop_and_clean(engine, &session, &why, dead_lead) {
            Ok(()) => engine.clear(Step::E2, &about),
            Err(e) => engine.fail(Step::E2, about, &e),
        }
    }
}

/// [`stop`], and for a Lead's dead pane (G2, §3.6) its cleanup: a pane outside
/// the chat slot is killed with its window before the write, so a failure is
/// tried again at the next tick; the chat slot's pane is replaced by the
/// placeholder after the write that records it gone.
fn stop_and_clean(engine: &mut Engine, session: &Live, why: &str, dead_lead: Option<&Pane>) -> Result<(), CpdError> {
    let in_slot = dead_lead.is_some_and(|pane| pane.session_name == engine.config.tmux.cockpit_session && pane.window_name == MAIN_WINDOW);
    if let Some(pane) = dead_lead.filter(|_| !in_slot) {
        engine.tmux.kill_window(&pane.window)?;
        engine.log.line(&format!("E2: killed the window {} of Lead {}'s dead pane {}", pane.window, session.name, pane.pane));
    }
    let changed = stop(engine, session, why)?;
    if let Some(pane) = dead_lead.filter(|_| in_slot && changed) {
        replace_with_placeholder(engine, pane)?;
        engine.log.line(&format!("E2: replaced Lead {}'s dead pane {} in the chat slot by the placeholder", session.name, pane.pane));
    }
    Ok(())
}

/// `respawn-pane -k` running `cpd tui view chat`, with `@cpd_view chat` set
/// and `@cpd_session` unset (§3.6): the placeholder `session park` builds.
fn replace_with_placeholder(engine: &mut Engine, pane: &Pane) -> Result<(), CpdError> {
    let spawn = Spawn {
        argv: [crate::session::PROGRAM, "tui", "view", "chat"].map(str::to_owned).into(),
        ..Spawn::default()
    };
    engine.tmux.respawn_pane_killing(&pane.pane, &spawn)?;
    engine.tmux.set_pane_view(&pane.pane, View::Chat)?;
    let unset = ["set", "-p", "-u", "-t", pane.pane.as_str(), OPT_SESSION].map(str::to_owned);
    engine.tmux.run(&unset).map(drop)
}

/// Whether the write changed the Session.
fn stop(engine: &mut Engine, session: &Live, why: &str) -> Result<bool, CpdError> {
    let changed = engine
        .store
        .write(SOURCE, |tx| crate::session::pane_gone(tx, &session.id, &session.state, session.pane.as_deref(), why))?;
    if changed {
        engine.log.line(&format!("E2: Session {} ({}) is stopped with no pane: {why}", session.id, session.name));
    }
    Ok(changed)
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

    const RUNNING: OffsetDateTime = datetime!(2030-01-01 10:00 UTC);

    fn minutes(n: i64) -> OffsetDateTime {
        RUNNING + time::Duration::minutes(n)
    }

    #[test]
    fn active_time_counts_from_running_not_from_starting() {
        // The Attempt started 40 minutes before `running`: those minutes are not active time.
        assert_eq!(active_between(RUNNING, minutes(5), &[]), Duration::from_secs(5 * 60));
    }

    #[test]
    fn active_time_is_zero_before_running() {
        assert_eq!(active_between(minutes(5), RUNNING, &[]), Duration::ZERO);
    }

    #[test]
    fn open_escalation_time_is_not_active_time_and_overlaps_count_once() {
        let escalations = [(minutes(10), Some(minutes(25))), (minutes(20), Some(minutes(30))), (minutes(35), None)];
        // 40 minutes, minus 10–30 and 35–40 waiting for answers.
        assert_eq!(active_between(RUNNING, minutes(40), &escalations), Duration::from_secs(15 * 60));
    }

    #[test]
    fn an_escalation_asked_before_running_is_clipped_to_running() {
        assert_eq!(active_between(RUNNING, minutes(10), &[(minutes(-5), Some(minutes(4)))]), Duration::from_secs(6 * 60));
    }
}
