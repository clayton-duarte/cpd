//! The engine's loop (contract §3.7; commands.md §12): `locks/watch.lock`,
//! `watch-started`, then E1–E5, E7–E11, E12–E16 (E6 was removed) every
//! `tick_seconds` until SIGTERM, SIGINT or SIGHUP, then `watch-stopped`; or until the store outgrows the binary
//! (its verdict no longer opens, contract §3.7), when the engine stands down: it writes nothing, logs one line and exits 0.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::lock::WatchLock;
use super::log::Log;
use super::poll_run::PollRun;
use super::report::{About, Failure, Report, Step};
use super::{
    SOURCE, archive, archive_leads, closed, distill, escalations, host_alert, hosts, inbox,
    janitor, liveness, models, open, parked, permanent, pr, restart, schedule, sleep, swr, titles,
    wake, workers,
};
use cpd_core::clock::{self, Clock};
use cpd_core::config::{Catalog, Config, load_catalog_with};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, NewEvent};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::host::pi::{self, PiDirs};
use cpd_core::host::spawn::Pid;
use cpd_core::ids::{AttemptId, ContextId, PlanId, SessionId, WorkflowKey};
use cpd_core::output::Output;
use cpd_core::policy::model::{Models, models_of_attempt, models_of_context, models_of_job};
use cpd_core::ports::Ports;
use cpd_core::pr::start::{self as start_workflow, By, Failed};
use cpd_core::store::{Persist, Store};

/// Set by SIGTERM, SIGINT and SIGHUP: the loop ends at its next check.
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: nix::libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// How often the wait between ticks looks at [`STOP`].
const NAP: Duration = Duration::from_millis(100);

pub(super) fn stopping() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// What every step works with.
pub(super) struct Engine {
    pub home: CpdHome,
    pub config: Config,
    pub store: Store,
    pub tmux: Tmux,
    pub log: Log,
    /// The seams the steps read time and start processes through: `Ports::real()` for `cpd watch`.
    pub ports: Ports,
    report: Report,
    /// pi's catalogs, one per distinct `[model]` table a step needed this tick (a repo's policy sets its own).
    catalogs: Vec<(Models, Catalog)>,
    /// The catalog a step test gives the engine for every key.
    fixed_catalog: Option<Catalog>,
    /// When E5's last `pr poll` ended.
    pub last_poll_end: Option<Instant>,
    /// E5's `pr poll` in flight.
    pub poll: Option<PollRun>,
    /// While GitHub's rate limit is nearly spent, E5 runs no poll before this.
    pub paused_until: Option<Instant>,
    /// The reset of GitHub's rate limit as the last poll reported it: where a rate-limited reply pauses to.
    pub rate_reset: Option<OffsetDateTime>,
    /// The `@cpd_workers` texts and width last set on the tmux server.
    pub workers: Option<String>,
    /// The `@cpd_host_alert` value last set on the tmux server (`None` when it was unset).
    pub host_alert: Option<String>,
    /// E9: the (highest record `seq`, uncurated count) of each Context's latest distill run in this engine's life, with the batch size it was given.
    pub distill_started: HashMap<ContextId, ((i64, i64), u8)>,
    /// E9: the `cpd context curate` run in flight of each Context.
    pub curating: HashMap<ContextId, Pid>,
    /// E9: the backoff after failed curate runs of each Context.
    pub curate_backoff: HashMap<ContextId, cpd_core::context::curate_backoff::Backoff>,
    /// E10: the retitle runs this engine started and has not seen end.
    pub titles: titles::Runs,
    /// E15: the helper's run in flight and when the last one ended.
    pub models: models::State,
    /// E24: when `gc::sweep` last ran.
    pub swr: swr::State,
    /// P1.10: the durable host this engine started or adopted, per Session (contract §6.19).
    pub hosts: hosts::State,
    /// E16: when this engine last started a `cpd janitor` pass (`None` before the first, in this engine's life).
    pub janitor_started: Option<std::time::Instant>,
    /// When the engine last started the detached tmux restart (contract §6.1).
    pub restart_started: Option<Instant>,
    /// The detached runs whose pid file the engine removes when it sees them end ([`super::sweep_detached`]).
    pub detached: Vec<(Pid, std::path::PathBuf)>,
    /// The `data_version` at the last verdict read (`None` before the first).
    verdict_seen: Option<i64>,
    /// The refusal once the store outgrew this binary: it stays set, and nothing reads the verdict again.
    outgrown: Option<CpdError>,
}

impl Engine {
    /// An engine on `home` (its config and store) and `tmux`, reading time and starting processes through `ports`.
    #[cfg(test)]
    pub(super) fn with_ports(home: CpdHome, tmux: Tmux, ports: &Ports) -> Engine {
        let config = Config::load(&home).expect("the test home's config");
        let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))
            .expect("the test home's store");
        store.use_clock(ports.clock.clone());
        let log = Log::open(&home).expect("the test home's log");
        Engine::assemble(
            home,
            config,
            store,
            tmux,
            log,
            Report::new(Duration::from_secs(60)),
            ports,
        )
    }

    /// An engine on `home` and `tmux` on the real ports, for the steps' tests.
    #[cfg(test)]
    pub(super) fn for_test(home: CpdHome, tmux: Tmux) -> Engine {
        Engine::with_ports(home, tmux, &Ports::real())
    }

    fn assemble(
        home: CpdHome,
        config: Config,
        store: Store,
        tmux: Tmux,
        log: Log,
        report: Report,
        ports: &Ports,
    ) -> Engine {
        Engine {
            home,
            config,
            store,
            tmux,
            log,
            ports: ports.clone(),
            report,
            catalogs: Vec::new(),
            fixed_catalog: None,
            last_poll_end: None,
            poll: None,
            paused_until: None,
            rate_reset: None,
            workers: None,
            host_alert: None,
            distill_started: HashMap::new(),
            curating: HashMap::new(),
            curate_backoff: HashMap::new(),
            titles: titles::Runs::default(),
            models: models::State::default(),
            swr: swr::State::default(),
            hosts: hosts::State::default(),
            janitor_started: None,
            restart_started: None,
            detached: Vec::new(),
            verdict_seen: None,
            outgrown: None,
        }
    }

    /// The catalog a step test gives the engine, so E3 never runs pi's Node or opens the real home for it.
    #[cfg(test)]
    pub(super) fn set_catalog(&mut self, catalog: Catalog) {
        self.fixed_catalog = Some(catalog);
    }

    /// Whether the store outgrew this binary: its verdict does not open (contract §3.7). Reads the verdict only
    /// when `data_version` changed since the last check; once true it stays true. A failed read is no answer: false.
    pub(super) fn outgrown(&mut self) -> bool {
        if self.outgrown.is_some() {
            return true;
        }
        let Ok(version) = self.store.data_version() else {
            return false;
        };
        if self.verdict_seen == Some(version) {
            return false;
        }
        if let Ok(refusal) = self
            .store
            .read(|conn| Ok(cpd_core::store::verdict(conn)?.refusal().cloned()))
        {
            self.verdict_seen = Some(version);
            self.outgrown = refusal;
        }
        self.outgrown.is_some()
    }

    /// Reports a failure of `step` for `about` (once until it clears). When the store outgrew the binary the
    /// failure is only logged: the engine stands down and writes nothing.
    pub(super) fn fail(&mut self, step: Step, about: About, failure: impl Into<Failure>) {
        if self.outgrown() {
            let failure = failure.into();
            self.log.line(&format!(
                "{step:?}: {} ({}); not reported: the store outgrew this binary",
                failure.message, failure.code
            ));
            return;
        }
        self.report
            .fail(&mut self.store, &mut self.log, step, about, failure);
    }

    /// `step` succeeded for `about`.
    pub(super) fn clear(&mut self, step: Step, about: &About) {
        self.report.clear(step, about);
    }

    /// pi's catalog (§9) of the models of the Job `job` of `plan` (its Workflow's repo key), for the ladder and E3.
    pub(super) fn catalog_of_job(&mut self, plan: &PlanId, job: &str) -> Result<Catalog, CpdError> {
        let home = self.home.clone();
        self.catalog_of(|store| store.read(|conn| models_of_job(conn, &home, plan, job)))
    }

    /// pi's catalog of the models of Workflow `workflow` of `plan` (its repo key), for its open Job's Tier.
    pub(super) fn catalog_of_workflow(
        &mut self,
        plan: &PlanId,
        workflow: &WorkflowKey,
    ) -> Result<Catalog, CpdError> {
        let home = self.home.clone();
        self.catalog_of(|store| {
            store.read(|conn| {
                cpd_core::policy::model::models_of_workflow(conn, &home, plan, workflow)
            })
        })
    }

    /// [`Engine::catalog_of_job`] of the Job of Attempt `id`.
    pub(super) fn catalog_of_attempt(
        &mut self,
        plan: &PlanId,
        id: &AttemptId,
    ) -> Result<Catalog, CpdError> {
        let home = self.home.clone();
        self.catalog_of(|store| {
            store.read(|conn| models_of_attempt(conn, &home, plan, &id.to_string()))
        })
    }

    /// pi's catalog of the models of Context `context`: what a host runs under (contract §6.19). A fixed catalog
    /// (a step test's) is returned without running pi's Node.
    pub(super) fn catalog_of_session(&mut self, context: &ContextId) -> Result<Catalog, CpdError> {
        let home = self.home.clone();
        self.catalog_of(|store| store.read(|conn| models_of_context(conn, &home, context.as_str())))
    }

    fn catalog_of(
        &mut self,
        models: impl FnOnce(&mut Store) -> Result<Models, CpdError>,
    ) -> Result<Catalog, CpdError> {
        if let Some(catalog) = &self.fixed_catalog {
            return Ok(catalog.clone());
        }
        let models = models(&mut self.store)?;
        if let Some((_, catalog)) = self.catalogs.iter().find(|(m, _)| *m == models) {
            return Ok(catalog.clone());
        }
        let catalog = load_catalog_with(&models, &mut self.store)?;
        self.catalogs.push((models, catalog.clone()));
        Ok(catalog)
    }

    /// One tick: the tmux restart's drain, the `@cpd_workers` step, the `@cpd_host_alert` step, then E1–E5, E7–E11, E12–E16 (§3.7; E6 is removed), each reporting its own failures.
    fn tick(&mut self) {
        self.catalogs.clear();
        let steps: [fn(&mut Engine); 24] = [
            restart::drain,
            workers::run,
            host_alert::run,
            permanent::run,
            escalations::run,
            liveness::run,
            schedule::run,
            open::run,
            pr::reap,
            pr::start,
            start_prs,
            parked::run,
            closed::run,
            bug_guard::run,
            inbox::run,
            distill::run,
            titles::run,
            models::run,
            swr::run,
            archive::run,
            sleep::run,
            archive_leads::run,
            hosts::run,
            janitor::run,
        ];
        for step in steps {
            if stopping() || self.outgrown() {
                return;
            }
            step(self);
        }
    }
}

/// Whether incident mode holds `step` machine-wide (contract §3.7): E9 (Context curation), E10 (Session
/// titles) and E11 (team review) do nothing and write nothing while one `incident` row exists; E24 (the SWR
/// outbox drain and the Slack fetch, which took over E17 and E11's reads) holds too; E1, E2, E5, E8, E12 and E13
/// run as usual. E3 starts only a Job of an incident Plan or an urgent one ([`schedule`]);
/// E4 and E7 act per Workflow ([`incident_holds_plan`]).
pub(super) fn incident_holds(engine: &mut Engine, step: Step) -> bool {
    if !matches!(
        step,
        Step::E9 | Step::E10 | Step::E11 | Step::E16 | Step::E24
    ) {
        return false;
    }
    engine
        .store
        .read(|conn| Ok(cpd_core::incident::is_on(conn)))
        .unwrap_or(false)
}

/// Whether incident mode holds `plan` for E4 and E7 (contract §3.7, hs-d643): the incident is on and
/// the Plan is not in `incident_plans` (`cpd_core::incident::is_held`). A held Plan's Workflows get no
/// `open` Job and are not started.
pub(super) fn incident_holds_plan(conn: &rusqlite::Connection, plan: &PlanId) -> bool {
    cpd_core::incident::is_held(conn, plan)
}

/// The Workflows E7 starts: `waiting` in a Plan in `dispatch`, ready (`start_pr::READY`: every need `merged`, a need of
/// another Plan too, and the base Workflow, if any, `open`), and not marked `start_failed_at` (§3.7). Oldest Plan first,
/// then by key.
fn startable() -> String {
    format!(
        "SELECT r.plan, r.key FROM workflows r JOIN plans p ON p.id = r.plan
          WHERE p.phase = 'dispatch' AND r.status = 'waiting' AND r.start_failed_at IS NULL AND {}
          ORDER BY p.created_at, r.plan, r.key",
        start_workflow::READY
    )
}

/// The ready `waiting` Workflows E7 starts, the Plan-scoped incident hold applied (contract §3.7): a Workflow of
/// a held Plan is left alone. Oldest Plan first, then by key (the query's order).
pub(super) fn startable_workflows(
    engine: &mut Engine,
) -> Result<Vec<(PlanId, WorkflowKey)>, CpdError> {
    engine.store.read(|conn| {
        let mut stmt = conn.prepare(&startable())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut waiting = Vec::new();
        for (plan, key) in rows {
            let plan = PlanId::parse(&plan)?;
            let key = WorkflowKey::parse(&key)?;
            // Incident mode on: a held Plan's Workflow is not started (§3.7 E7).
            if !incident_holds_plan(conn, &plan) {
                waiting.push((plan, key));
            }
        }
        Ok(waiting)
    })
}

/// E7, Workflow start (§3.7, §3.8): starts each startable Workflow. A failed setup (worktree,
/// remote check or `pre-start`) marks the Workflow `start_failed_at` and appends one
/// `error` `workflow_start_failed` for the Lead; the engine does not try that Workflow again,
/// the Lead retries with `cpd workflow start`.
fn start_prs(engine: &mut Engine) {
    let waiting = match startable_workflows(engine) {
        Ok(waiting) => {
            engine.clear(Step::E7, &About::Machine);
            waiting
        }
        Err(e) => return engine.fail(Step::E7, About::Machine, &e),
    };
    for (plan, key) in waiting {
        if stopping() {
            return;
        }
        let about = About::Plan(plan.clone());
        let workflow = engine.store.read(|conn| {
            Ok(start_workflow::load(conn, &plan)?
                .into_iter()
                .find(|p| p.key == key))
        });
        let workflow = match workflow {
            Ok(Some(workflow)) => workflow,
            Ok(None) => continue,
            Err(e) => {
                engine.fail(Step::E7, about, &e);
                continue;
            }
        };
        match start_workflow::start(
            &engine.home,
            &engine.config,
            &mut engine.store,
            &plan,
            &workflow,
            By::Engine,
        ) {
            Ok(started) => {
                engine.log.line(&format!(
                    "E7: started Workflow {key} of Plan {plan} ({})",
                    started.to_json()["lead_branch"]
                ));
                engine.clear(Step::E7, &about);
            }
            Err(Failed::Setup { step, error }) => {
                match start_workflow::record_failure(
                    &mut engine.store,
                    SOURCE,
                    &plan,
                    &key,
                    step,
                    &error,
                ) {
                    Ok(true) => {
                        engine.log.line(&format!(
                            "E7: Workflow {key} of Plan {plan} could not be started ({}): {}",
                            step.as_str(),
                            error.message()
                        ));
                        engine.clear(Step::E7, &about);
                    }
                    Ok(false) => {}
                    Err(e) => engine.fail(Step::E7, about, &e),
                }
            }
            // The Workflow changed since it was read (`workflow start` got there first): decided again next tick.
            Err(Failed::Other(e)) if e.code() == ErrorCode::CasLost => {
                engine.log.line(&format!(
                    "E7: skipped Workflow {key} of Plan {plan}: {}",
                    e.message()
                ));
            }
            Err(Failed::Other(e)) => engine.fail(Step::E7, about, &e),
        }
    }
}

/// A9's `metrics` of an Attempt the engine ends, parsed from its Worker's
/// session file (cpd 1 §14); none when there is no Worker or no readable file.
pub(super) fn metrics(session: Option<&SessionId>) -> Option<Value> {
    let dirs = PiDirs::from_env().ok()?;
    let file = pi::session_file(&dirs, session?)?;
    cpd_core::metrics::parse_file(&file)
        .ok()
        .map(|m| m.to_json())
}

fn catch_signals() -> Result<(), CpdError> {
    let action = SigAction::new(
        SigHandler::Handler(on_signal),
        SaFlags::empty(),
        SigSet::empty(),
    );
    for signal in [Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP] {
        // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
        unsafe { sigaction(signal, &action) }.map_err(|e| {
            CpdError::new(
                cpd_core::error::ErrorCode::StoreFailed,
                format!("Cannot handle {signal}: {e}."),
            )
        })?;
    }
    Ok(())
}

/// How the engine meets a transient store error (§3.7): it waits, logging to
/// `run/watch.log`, until the store is clear, or until `give_up` says so.
fn persist(home: &CpdHome, give_up: fn() -> bool) -> Result<Persist, CpdError> {
    let mut log = Log::open(home)?;
    Ok(Persist::new(move |line| log.line(line), give_up))
}

fn watch_event(store: &mut Store, event: NewEvent) -> Result<i64, CpdError> {
    store.write(SOURCE, |tx| events::append(tx, event.clone()))
}

/// A restart that is back within this many seconds is logged only (User, hs-d471).
const PLANNED_RESTART_QUIET_SECS: f64 = 10.0;
/// The oldest an install's marker may be when the new engine starts (contract §3.7 "A planned restart").
const PLANNED_RESTART_MARKER_MAX_AGE_SECS: i64 = 120;

/// The install's marker of a planned restart, `run/install-restart.json`.
fn marker_path(home: &CpdHome) -> std::path::PathBuf {
    home.run_dir().join("install-restart.json")
}

/// `cpd watch --mark-restart`: writes the marker for the latest `watch-started`'s pid, by rename.
pub(super) fn mark_restart() -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let data: Option<String> = store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT data FROM events WHERE kind = 'watch-started' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    let pid = data
        .and_then(|d| serde_json::from_str::<Value>(&d).ok())
        .and_then(|d| d["pid"].as_i64())
        .ok_or_else(|| {
            CpdError::new(
                ErrorCode::WatchNotStarted,
                "No engine has started in this home.",
            )
        })?;
    // The time of the ports in scope (`Ports::current`): the system's, or a test's `ManualClock`.
    let marker = json!({"by": "install-cpd.sh", "at": clock::timestamp(Ports::current().clock.utc()), "engine_pid": pid});
    let io = |e: std::io::Error| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot write {}: {e}.", marker_path(&home).display()),
        )
    };
    std::fs::create_dir_all(home.run_dir()).map_err(io)?;
    let tmp = home
        .run_dir()
        .join(format!("install-restart.json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, marker.to_string()).map_err(io)?;
    std::fs::rename(&tmp, marker_path(&home)).map_err(io)?;
    Ok(Output::success(json!({"marked": true, "engine_pid": pid})))
}

/// Reads and deletes the marker; the pid of the engine it says is being retired when it is well formed and
/// was written at `at <= now` (the time `clock` reads, `Clock::utc`) and at most the max age ago. Nothing else is a
/// marker; an unreadable file is logged once.
pub(super) fn consume_marker_with(clock: &dyn Clock, home: &CpdHome, log: &mut Log) -> Option<i64> {
    let now = clock.utc();
    let path = marker_path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log.line(&format!("cannot read {}: {e}", path.display()));
            let _ = std::fs::remove_file(&path);
            return None;
        }
    };
    let _ = std::fs::remove_file(&path);
    let parsed = serde_json::from_str::<Value>(&text).ok().and_then(|v| {
        v.get("by")?.as_str()?;
        let at = OffsetDateTime::parse(v.get("at")?.as_str()?, &Rfc3339).ok()?;
        Some((at, v.get("engine_pid")?.as_i64()?))
    });
    let Some((at, pid)) = parsed else {
        log.line(&format!(
            "{} is not a marker ({}), ignored and deleted",
            path.display(),
            text.trim().chars().take(80).collect::<String>()
        ));
        return None;
    };
    (at <= now && now - at <= time::Duration::seconds(PLANNED_RESTART_MARKER_MAX_AGE_SECS))
        .then_some(pid)
}

/// How long the engine was down before this start (contract §3.7, hs-d328).
pub(super) struct Downtime {
    /// When the previous engine stopped: its `watch-stopped`, else the last time it was alive.
    pub(super) since: OffsetDateTime,
    /// Whether the previous engine said it stopped (`watch-stopped`): otherwise it crashed or was killed.
    pub(super) said: bool,
    pub(super) down: time::Duration,
    /// The previous engine's pid and the install's marker for it, when the stop was an install's planned restart.
    pub(super) planned: bool,
    /// The marker was valid but the engine stayed down past the quiet bound.
    pub(super) overdue: bool,
}

impl Downtime {
    /// None for the first engine this home has seen. The previous engine's last Event is `watch-stopped`
    /// (its `at` is when it stopped), or `watch-started` with no stop (a crash, SIGKILL): then it was last
    /// alive at the lock file's modification time (the engine marks it every tick), or at its start when
    /// that engine did not mark it. Now is the time `clock` reads (`Clock::utc`).
    pub(super) fn since_previous_with(
        clock: &dyn Clock,
        store: &mut Store,
        lock: &WatchLock,
        lock_existed: bool,
        marker: Option<i64>,
    ) -> Result<Option<Downtime>, CpdError> {
        let last: Option<(String, String)> = store.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT kind, at FROM events WHERE kind IN ('watch-started', 'watch-stopped') ORDER BY id DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?)
        })?;
        // The engine whose stop is reported: the latest `watch-started`.
        let previous_pid: Option<i64> = store.read(|conn| {
            let data: Option<String> = conn
                .query_row(
                    "SELECT data FROM events WHERE kind = 'watch-started' ORDER BY id DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(data
                .and_then(|d| serde_json::from_str::<Value>(&d).ok())
                .and_then(|d| d["pid"].as_i64()))
        })?;
        let marked = marker.is_some() && marker == previous_pid;
        let Some((kind, at)) = last else {
            return Ok(None);
        };
        let at = OffsetDateTime::parse(&at, &Rfc3339).map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("A {kind} Event has an `at` that is not RFC 3339: {e}."),
            )
        })?;
        let said = kind == "watch-stopped";
        // A lock file this engine created says nothing of the previous one: it is dated by its `watch-started`.
        let since = if said || !lock_existed {
            at
        } else {
            at.max(OffsetDateTime::from(lock.last_alive()?))
        };
        let down = (clock.utc() - since).max(time::Duration::ZERO);
        let quiet = down.as_seconds_f64() <= PLANNED_RESTART_QUIET_SECS;
        Ok(Some(Downtime {
            since,
            said,
            down,
            planned: marked && quiet,
            overdue: marked && !quiet,
        }))
    }

    /// `The engine was down for 3.2 s: it stopped at … ` (the one sentence of the log, the Event and the message).
    pub(super) fn sentence(&self, zone: &cpd_core::clock::TimeZone) -> String {
        let how = if self.said {
            "it stopped"
        } else {
            "it was last alive (it left no watch-stopped: a crash or a kill)"
        };
        let at = clock::prose(self.since, zone);
        let secs = self.down.as_seconds_f64();
        if self.overdue {
            format!(
                "The engine was down for {secs:.1} s: {how} at {at}; it was an install's restart that took longer than {PLANNED_RESTART_QUIET_SECS:.0} s."
            )
        } else if self.planned {
            format!(
                "The engine was down for {secs:.1} s: {how} at {at} (an install's planned restart)."
            )
        } else {
            format!("The engine was down for {secs:.1} s: {how} at {at}.")
        }
    }
}

/// [`run`] on `ports` and `home`: the ticks and the naps between them read `ports.clock`, the steps start nothing but
/// through `ports`, and the loop ends when `until` is set (checked between ticks and between naps).
pub(super) fn run_with(
    ports: &Ports,
    home: &CpdHome,
    until: &AtomicBool,
) -> Result<Output, CpdError> {
    serve(ports, home, until, || Ok(()))
}

/// `cpd watch`: the engine in the foreground, until a signal.
pub(super) fn run() -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    serve(&Ports::real(), &home, &STOP, catch_signals)
}

/// The engine's life: lock, `watch-started`, ticks until `until`, `watch-stopped`. `catch` runs once the store is
/// open, where the signal handlers are put in.
fn serve(
    ports: &Ports,
    home: &CpdHome,
    until: &AtomicBool,
    catch: impl FnOnce() -> Result<(), CpdError>,
) -> Result<Output, CpdError> {
    let clock = &*ports.clock;
    let config = Config::load(home)?;
    let lock_existed = home.watch_lock().exists();
    let lock = WatchLock::take(home)?;
    let mut log = Log::open(home)?;
    // A tick's steps give up waiting when the engine is told to stop.
    let mut store = Store::open_persistent_with(
        ports.clock.clone(),
        home,
        u64::from(config.timeouts.store_busy_ms),
        persist(home, stopping)?,
    )?;
    catch()?;
    let pid = std::process::id();
    // What the previous engine left, read before this one's `watch-started` hides it.
    let marker = consume_marker_with(clock, home, &mut log);
    let down = Downtime::since_previous_with(clock, &mut store, &lock, lock_existed, marker)?;
    let how_long = down
        .as_ref()
        .map(|d| format!(" {}", d.sentence(&ports.clock.zone())))
        .unwrap_or_default();
    watch_event(
        &mut store,
        NewEvent::watch_started(
            format!("The engine started (pid {pid}).{how_long} See: cpd doctor"),
            pid,
        ),
    )?;
    log.line(&format!(
        "started (pid {pid}), a tick every {}s",
        config.watch.tick_seconds
    ));
    if let Some(down) = down {
        log.line(&down.sentence(&ports.clock.zone()));
    }

    let tmux = Tmux::new(config.tmux.socket.as_str())
        .with_runner(ports.runner.clone())
        .with_clock(ports.clock.clone());
    let report = Report::new(Duration::from_secs(u64::from(
        config.watch.error_repeat_seconds,
    )));
    let mut engine = Engine::assemble(home.clone(), config, store, tmux, log, report, ports);

    let tick = engine.config.watch.tick_seconds.duration();
    let mut failing = false;
    while !until.load(Ordering::SeqCst) && !engine.outgrown() {
        let began = clock.now();
        // The next engine says when this one stopped from it when it leaves no `watch-stopped`.
        if let Err(e) = lock.touch_with(clock) {
            engine
                .log
                .line(&format!("{} ({})", e.message(), e.code().as_str()));
        }
        engine.tick();
        // Read after the tick's steps: the commits of the children it started count as another's.
        let seen = engine.store.data_version();
        let mut committed = false;
        // The next tick comes at the earliest of a tick from its start, E5's due time and, when another process
        // committed, half a second from its start (§3.7). A finished poll is applied at the next nap.
        while !until.load(Ordering::SeqCst) && !engine.outgrown() {
            pr::reap(&mut engine);
            if !committed {
                match (&seen, engine.store.data_version()) {
                    (Ok(before), Ok(now)) => {
                        failing = false;
                        committed = *before != now;
                    }
                    (_, Err(e)) => {
                        committed = true;
                        if !failing {
                            engine.log.line(&format!(
                                "cannot read the store's data version: {} ({})",
                                e.message(),
                                e.code().as_str()
                            ));
                        }
                        failing = true;
                    }
                    (Err(_), Ok(_)) => committed = true,
                }
            }
            let now = clock.now();
            let next = wake::next_tick(began, now, tick, pr::due(&engine, now), committed);
            if now >= next {
                break;
            }
            clock.sleep(NAP.min(next - now));
        }
    }

    if let Some(refusal) = engine.outgrown.clone() {
        engine.log.line(&format!(
            "standing down: this binary can no longer open the store ({}: {}); the engine of the installed binary takes over",
            refusal.code().as_str(),
            refusal.message()
        ));
        pr::discard(&mut engine);
        return Ok(Output::success(json!({"stopped": true})));
    }
    engine.log.line("stopping on a signal");
    pr::finish(&mut engine);
    // `watch-stopped` is not lost to a busy store: it waits for as long as that lasts.
    engine.store.persist(persist(&engine.home, || false)?);
    if let Err(e) = watch_event(
        &mut engine.store,
        NewEvent::watch_stopped(
            format!("The engine stopped (pid {pid}). Start it: cpd watch --ensure"),
            pid,
        ),
    ) {
        engine.log.line(&format!(
            "cannot append watch-stopped: {} ({})",
            e.message(),
            e.code().as_str()
        ));
    }
    Ok(Output::success(json!({"stopped": true})))
}
