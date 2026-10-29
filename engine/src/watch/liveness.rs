//! E2, liveness (contract §3.4, §3.6, §3.7, §7.4):
//! - an Attempt not `ended` whose driver lock is free (its driver died): in
//!   `merging`, the merge recovery R for that Attempt, with no repo lock
//!   (hs-d243); in any other
//!   status, ended `cancelled` if a cancel was requested, else `crashed`; but a driver that an install or a
//!   store migration ended (it left `run/<plan>/attempts/<attempt>.infra`, hs-n278) is ended as one whose driver
//!   never started (hs-d160): the same Tier, not counted by `retry_cap`, E3's back-off applies;
//! - an Attempt with a live driver: `stop_request = cancelled` when its Job
//!   has a cancel request, `timed_out` when its active time is over
//!   `attempt_minutes`; the driver acts on it (A5);
//! - a Session whose pane is dead or missing: `stopped`, with no pane
//!   (`session::pane_gone`); a Session not `stopped` with no pane: `stopped`;
//!   and a Lead's dead pane is cleaned up (G2): its park or ctx window is
//!   killed, or, in the chat slot, the pane becomes the placeholder (§3.6).

use std::collections::BTreeMap;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::SOURCE;
use super::engine::{Engine, metrics};
use super::report::{About, Step};
use cpd_core::attempt::ledger::{self, Cause, End, Ladder, Stage};
use cpd_core::driver::lock::{self, Free};
use cpd_core::driver::runner;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::layout::{MAIN_WINDOW, View};
use cpd_core::host::tmux::{OPT_SESSION, Pane, Spawn};
use cpd_core::ids::{AttemptId, PlanId, SessionId};
use cpd_core::merge;

/// An Attempt not `ended`, as E2 reads it.
struct Active {
    plan: PlanId,
    id: AttemptId,
    status: String,
    stop_request: Option<String>,
    cancel_requested: bool,
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
            "SELECT a.plan, a.id, a.status, a.stop_request, j.cancel_requested_at IS NOT NULL
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
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
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(plan, id, status, stop_request, cancel_requested)| {
                Ok(Active {
                    plan: PlanId::parse(&plan)?,
                    id: AttemptId::parse(&id)?,
                    status,
                    stop_request,
                    cancel_requested,
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

    for attempt in &active {
        // A submit that died inside its check span left the span open: close it (§7.7), before the Attempt's end or its timeout reads it.
        if attempt.status == "running" {
            if let Err(e) = close_dead_span(engine, attempt) {
                engine.fail(Step::E2, attempt.about(), &e);
                continue;
            }
        }
        let probe = lock::probe(&engine.home, &attempt.plan, &attempt.id);
        let result = match probe {
            // Released at once: R takes it again itself.
            Ok(Some(free)) if attempt.status == "merging" => {
                drop(free);
                recover(engine, attempt)
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
}

/// A check span (contract §7.7) of a `running` Attempt whose `attempt submit` is gone: its latest check Event is a `start` and nothing
/// holds `locks/check-<plan>--<attempt>.lock`. Appends the missing `end` with `died: true` (§3.7 E2), once. The lock is probed first and
/// the Events read after, so a submit that has just ended its span (it writes the `end` before it lets the lock go) is not closed twice.
fn close_dead_span(engine: &mut Engine, active: &Active) -> Result<(), CpdError> {
    if lock::check_span_open(&engine.home, &active.plan, &active.id)? {
        return Ok(());
    }
    let (plan, id) = (&active.plan, &active.id);
    engine.store.write(SOURCE, |tx| {
        let latest: Option<(String, String)> = tx
            .query_row(
                "SELECT json_extract(data, '$.check'), json_extract(data, '$.what') FROM events
                  WHERE plan = ?1 AND attempt = ?2 AND kind = 'attempt-status' AND json_extract(data, '$.check') IS NOT NULL
                  ORDER BY id DESC LIMIT 1",
                [plan.as_str(), id.to_string().as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((_, what)) = latest.filter(|(check, _)| check == "start") else { return Ok(()) };
        let job = ledger::load(tx, plan, id)?.job;
        let text = format!("Attempt {id} of Job {job} ended its {what} check: its submit died inside it.");
        cpd_core::events::append(tx, cpd_core::events::NewEvent::attempt_check_end(text, &what, true).plan(plan.clone()).job(job).attempt(id.clone()))?;
        Ok(())
    })?;
    Ok(())
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
    // Incident mode asked the Attempt to stop but the Job carries no cancel request: it ends `Interrupted`,
    // which requeues the Job at the same Tier (contract §3.7).
    let incident_stop = cancel.is_none() && active.stop_request.as_deref() == Some("cancelled");
    // An install or a store migration ended the driver, not the Worker (hs-n278): no `driver_pid`, so the
    // ladder keeps the Tier and `retry_cap` does not count the Attempt, as for a driver that never started.
    let infra = match (cancel.is_some(), incident_stop) {
        (true, _) | (false, true) => None,
        (false, false) => engine.home.infra(plan, id)?,
    };
    let end = match (&cancel, incident_stop, &infra) {
        (Some(_), _, _) => End::Cancelled,
        (None, true, _) => End::Interrupted,
        (None, false, Some((why, message))) => End::Failed {
            cause: Cause::Crashed,
            detail: format!("Its driver ended on {} ({message}), not through its Worker.", why.code()),
        },
        (None, false, None) => End::Failed { cause: Cause::Crashed, detail: format!("Its driver died while it was {}.", target.status) },
    };
    let catalog = engine.catalog_of_job(plan, target.job.as_str())?;
    let ladder = Ladder { catalog: &catalog, retry_cap: engine.config.retry_cap };
    let metrics = metrics(target.worker_session.as_ref());
    engine.store.write(SOURCE, |tx| {
        if infra.is_some() {
            tx.execute(
                "UPDATE attempts SET driver_pid = NULL WHERE plan = ?1 AND id = ?2 AND status = ?3",
                params![plan.as_str(), id.to_string(), target.status],
            )?;
        }
        ledger::end(tx, &target, &end, metrics.as_ref(), &ladder, Stage::Driver)
    })?;
    // A9: after the end is committed, while the lock is still held.
    lock::remove(&engine.home, plan, id)?;
    engine.home.remove_infra(plan, id)?;
    drop(lock);
    engine.log.line(&format!("E2: the driver of {id} (Plan {plan}) is dead: ended {} / {}", end.outcome(), end.cause()));
    runner::kill_orphan(&engine.ports, &engine.config, &engine.home, &engine.tmux, &mut engine.store, plan, id)
}

/// R (§7.4) for the orphaned merge `active`, with no repo lock (hs-d243): R
/// takes the Attempt's driver lock itself; held again meanwhile, it does
/// nothing and a later tick looks again.
fn recover(engine: &mut Engine, active: &Active) -> Result<(), CpdError> {
    let catalog = engine.catalog_of_attempt(&active.plan, &active.id)?;
    let metrics = |a: &merge::Attempt| metrics(a.worker_session.as_ref());
    let ctx = merge::Ctx { home: &engine.home, config: &engine.config, catalog: &catalog, source: SOURCE, metrics: &metrics };
    let recovered = merge::recover_attempt(&mut engine.store, &ctx, &active.plan, &active.id)?;
    if let Some(r) = recovered {
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
/// running` `attempt-status` Event: the wait in `starting` is not counted), minus the time an Escalation of it was
/// `open` (§3.3, §3.4 A4), minus the check spans of its `attempt submit` (a `start` to its `end`, or to now while open; §7.7). Overlaps
/// count once.
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
        let mut rows = stmt.query_map([plan, &id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.extend(check_spans(conn, plan, &id)?);
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
    Ok(active_between(parse_time(&running)?, engine.ports.clock.utc(), &escalations))
}

/// The check spans of the Attempt as `(start, end)`, `end` `None` for a span still open: each `start` Event pairs with the next `end`.
fn check_spans(conn: &rusqlite::Connection, plan: &str, id: &str) -> rusqlite::Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT at, json_extract(data, '$.check') FROM events
          WHERE plan = ?1 AND attempt = ?2 AND kind = 'attempt-status' AND json_extract(data, '$.check') IS NOT NULL ORDER BY id",
    )?;
    let events = stmt.query_map([plan, id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut spans: Vec<(String, Option<String>)> = Vec::new();
    for (at, check) in events {
        match (check.as_str(), spans.last_mut()) {
            ("start", Some((_, end))) if end.is_none() => {}
            ("start", _) => spans.push((at, None)),
            (_, Some((_, end))) if end.is_none() => *end = Some(at),
            _ => {}
        }
    }
    Ok(spans)
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

/// The Sessions E2 can still see as live. The `WHERE` is the `WHERE` of the partial index `sessions_live`
/// (migration `read-indexes`, engine-F2), written the same: SQLite reads the index only for a query whose
/// `WHERE` it can prove implies the index's, so the read costs the live Sessions, not every Session ever made
/// (`sessions` only grows). A test pins the plan.
const LIVE_SESSIONS: &str =
    "SELECT id, role, state, tmux_pane, name FROM sessions WHERE state <> 'stopped' OR tmux_pane IS NOT NULL ORDER BY id";

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
        let mut stmt = conn.prepare(LIVE_SESSIONS)?;
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
        // R9: a live durable Worker has no pane by design.
        match live_durable_worker(engine, &session, found.is_some()) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                engine.fail(Step::E2, about, &e);
                continue;
            }
        }
        let dead_lead = found.filter(|pane| pane.dead && session.role == "lead");
        match stop_and_clean(engine, &session, &why, dead_lead) {
            Ok(()) => engine.clear(Step::E2, &about),
            Err(e) => engine.fail(Step::E2, about, &e),
        }
    }
}

/// R9: whether `session` is a paneless Worker Session whose Attempt is not `ended` and has the stored runner `durable`: it
/// has no pane by design, so it is live. Any other paneless Session is not.
fn live_durable_worker(engine: &mut Engine, session: &Live, has_pane: bool) -> Result<bool, CpdError> {
    if has_pane || session.role != "worker" {
        return Ok(false);
    }
    let found: Option<(String, String)> = engine.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT a.status, a.runner FROM sessions s JOIN attempts a ON a.plan = s.plan AND a.id = s.attempt WHERE s.id = ?1",
                [session.id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    })?;
    Ok(found.is_some_and(|(status, runner)| status != "ended" && runner == "durable"))
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
    if changed && session.state != "stopped" && matches!(session.role.as_str(), "lead" | "manager") {
        restart(engine, session)?;
    }
    Ok(())
}

/// A Lead or Manager that died while not `stopped` is not restarted when the engine started it less than this ago (A4).
const RESTART_GUARD: Duration = Duration::from_secs(10 * 60);

/// `run/session-restarts.json`: a map of Session id to when E2 last restarted it, as the engine's clock reads it (the Ledger's
/// times are wall time, the guard must follow the engine's clock). Safe to delete: the 10 minutes then start again. A file that
/// is not such a map is deleted and written again.
fn restarts_path(engine: &Engine) -> std::path::PathBuf {
    engine.home.run_dir().join("session-restarts.json")
}

fn read_restarts(engine: &mut Engine) -> BTreeMap<String, OffsetDateTime> {
    let path = restarts_path(engine);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(e) => {
            engine.log.line(&format!("E2: cannot read {}: {e}", path.display()));
            return BTreeMap::new();
        }
    };
    let parsed = serde_json::from_str::<BTreeMap<String, String>>(&text)
        .ok()
        .and_then(|m| m.into_iter().map(|(id, at)| Some((id, OffsetDateTime::parse(&at, &Rfc3339).ok()?))).collect());
    parsed.unwrap_or_else(|| {
        engine.log.line(&format!("E2: {} is not a map of Session to time: deleted, written again", path.display()));
        let _ = std::fs::remove_file(&path);
        BTreeMap::new()
    })
}

/// Records the restart of `id` at `now`, and drops the entries older than [`RESTART_GUARD`].
fn write_restart(engine: &mut Engine, id: &str, now: OffsetDateTime) {
    let mut all = read_restarts(engine);
    all.insert(id.to_owned(), now);
    let limit = time::Duration::try_from(RESTART_GUARD).unwrap_or(time::Duration::ZERO);
    let kept: BTreeMap<String, String> =
        all.into_iter().filter(|(_, at)| now - *at < limit).map(|(id, at)| (id, cpd_core::clock::timestamp(at))).collect();
    let path = restarts_path(engine);
    let text = serde_json::to_string(&kept).expect("a map of strings serializes");
    if let Err(e) = cpd_core::atomic::write_atomic(&path, text) {
        engine.log.line(&format!("E2: cannot write {}: {e}", path.display()));
    }
}

/// Restarts `session` in a parked window, unless it is archived, its Lead's Plan is `done` or archived, or the engine restarted it
/// less than [`RESTART_GUARD`] ago: then it stays `stopped` and the step fails `session_restart_loop`.
fn restart(engine: &mut Engine, session: &Live) -> Result<(), CpdError> {
    let id = session.id.as_str();
    let live: bool = engine.store.read(|conn| {
        let (archived, plan): (bool, Option<String>) =
            conn.query_row("SELECT archived_at IS NOT NULL, plan FROM sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(!archived
            && match (&plan, session.role.as_str()) {
                (Some(plan), "lead") => cpd_core::session::plan_inactive(conn, plan).is_ok(),
                _ => true,
            })
    })?;
    if !live {
        return Ok(());
    }
    let now = engine.ports.clock.utc();
    if let Some(last) = read_restarts(engine).get(id) {
        let seconds = (now - *last).whole_seconds();
        if seconds < i64::try_from(RESTART_GUARD.as_secs()).unwrap_or(i64::MAX) {
            return Err(CpdError::new(
                ErrorCode::SessionRestartLoop,
                format!("Session {} ({}) died {seconds}s after the engine last restarted it: left stopped.", session.id, session.name),
            )
            .with("session", id)
            .with("name", session.name.as_str())
            .with("seconds", seconds));
        }
    }
    if let Some(pane) = cpd_core::session::start_parked_as(&engine.ports, engine.home.clone(), &session.id, SOURCE)? {
        write_restart(engine, id, now);
        engine.log.line(&format!("E2: restarted Session {} ({}) in the parked pane {pane}", session.id, session.name));
    }
    Ok(())
}

/// `respawn-pane -k` running `cpd tui view chat`, with `@cpd_view chat` set
/// and `@cpd_session` unset (§3.6): the placeholder `session park` builds.
fn replace_with_placeholder(engine: &mut Engine, pane: &Pane) -> Result<(), CpdError> {
    let spawn = Spawn {
        argv: [cpd_core::session::PROGRAM, "tui", "view", "chat"].map(str::to_owned).into(),
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
        .write(SOURCE, |tx| cpd_core::session::pane_gone(tx, &session.id, &session.state, session.pane.as_deref(), why))?;
    if changed {
        engine.log.line(&format!("E2: Session {} ({}) is stopped with no pane: {why}", session.id, session.name));
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    /// engine-F2: E2's read of the Sessions goes through `sessions_live`, which lists the live Sessions only.
    #[test]
    fn the_live_sessions_are_read_through_the_partial_index_not_by_scanning_every_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = cpd_core::home::CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = cpd_core::store::Store::open(&home, 5000).unwrap();
        let plan: Vec<String> = store
            .read(|conn| {
                let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {LIVE_SESSIONS}"))?;
                Ok(stmt.query_map([], |r| r.get::<_, String>(3))?.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap();
        assert!(plan.iter().any(|step| step.contains("USING INDEX sessions_live")), "{plan:?}");
        assert!(!plan.iter().any(|step| step.contains("sqlite_autoindex_sessions_1")), "{plan:?}");
    }

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
