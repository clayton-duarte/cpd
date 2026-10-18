//! The engine's loop (contract §3.7; commands.md §12): `locks/watch.lock`,
//! `watch-started`, the import of the Context stores, then E1–E7 every
//! `tick_seconds` until SIGTERM or SIGINT, then `watch-stopped`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
use serde_json::{Value, json};

use super::lock::WatchLock;
use super::log::Log;
use super::report::{About, Failure, Report, Step};
use super::{SOURCE, completion, contexts, escalations, liveness, pr, schedule};
use crate::config::{Catalog, Config, load_catalog};
use crate::context::Contexts;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::host::pi::{self, PiDirs};
use crate::ids::{PlanId, PrKey, SessionId};
use crate::output::Output;
use crate::pr::start::{self as start_pr, By, Failed};
use crate::store::Store;

/// Set by SIGTERM and SIGINT: the loop ends at its next check.
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
    report: Report,
    /// pi's catalog, read at most once per tick and only when a step needs it.
    catalog: Option<Catalog>,
    /// When E5's last `pr poll` ended.
    pub last_poll_end: Option<Instant>,
    /// While GitHub's rate limit is nearly spent, E5 runs no poll before this.
    pub paused_until: Option<Instant>,
    /// When E6 last ran.
    pub synced: Option<Instant>,
}

impl Engine {
    /// Reports a failure of `step` for `about` (once until it clears).
    pub(super) fn fail(&mut self, step: Step, about: About, failure: impl Into<Failure>) {
        self.report.fail(&mut self.store, &mut self.log, step, about, failure);
    }

    /// `step` succeeded for `about`.
    pub(super) fn clear(&mut self, step: Step, about: &About) {
        self.report.clear(step, about);
    }

    /// pi's catalog (§9), for the ladder and E3.
    pub(super) fn catalog(&mut self) -> Result<Catalog, CpdError> {
        if let Some(catalog) = &self.catalog {
            return Ok(catalog.clone());
        }
        let catalog = load_catalog(&self.config)?;
        self.catalog = Some(catalog.clone());
        Ok(catalog)
    }

    /// One tick: E1–E7 (§3.7), each reporting its own failures.
    fn tick(&mut self) {
        self.catalog = None;
        let steps: [fn(&mut Engine); 7] =
            [escalations::run, liveness::run, schedule::run, completion::run, pr::run, start_prs, contexts::run];
        for step in steps {
            if stopping() {
                return;
            }
            step(self);
        }
    }
}

/// The PRs E7 starts: `waiting` in a Plan in `dispatch`, every need `merged`, and
/// not marked `start_failed_at` (§3.7). Oldest Plan first, then by key.
const STARTABLE: &str = "SELECT r.plan, r.key FROM prs r JOIN plans p ON p.id = r.plan
  WHERE p.phase = 'dispatch' AND r.status = 'waiting' AND r.start_failed_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM pr_needs n JOIN prs q ON q.plan = n.plan AND q.key = n.needs
                     WHERE n.plan = r.plan AND n.pr = r.key AND q.status <> 'merged')
  ORDER BY p.created_at, r.plan, r.key";

/// E7, PR start (§3.7, §3.8): starts each startable PR. A failed setup (worktree,
/// remote check or `pre-start`) marks the PR `start_failed_at` and appends one
/// `error` `pr_start_failed` for the Lead; the engine does not try that PR again,
/// the Lead retries with `cpd pr start`.
fn start_prs(engine: &mut Engine) {
    let waiting = engine.store.read(|conn| {
        let mut stmt = conn.prepare(STARTABLE)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|(plan, key)| Ok((PlanId::parse(&plan)?, PrKey::parse(&key)?))).collect::<Result<Vec<_>, CpdError>>()
    });
    let waiting = match waiting {
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
        let pr = engine.store.read(|conn| Ok(start_pr::load(conn, &plan)?.into_iter().find(|p| p.key == key)));
        let pr = match pr {
            Ok(Some(pr)) => pr,
            Ok(None) => continue,
            Err(e) => {
                engine.fail(Step::E7, about, &e);
                continue;
            }
        };
        match start_pr::start(&engine.home, &engine.config, &mut engine.store, &plan, &pr, By::Engine) {
            Ok(started) => {
                engine.log.line(&format!("E7: started PR {key} of Plan {plan} ({})", started.to_json()["lead_branch"]));
                engine.clear(Step::E7, &about);
            }
            Err(Failed::Setup { step, error }) => {
                match start_pr::record_failure(&mut engine.store, SOURCE, &plan, &key, step, &error) {
                    Ok(true) => {
                        engine.log.line(&format!("E7: PR {key} of Plan {plan} could not be started ({}): {}", step.as_str(), error.message()));
                        engine.clear(Step::E7, &about);
                    }
                    Ok(false) => {}
                    Err(e) => engine.fail(Step::E7, about, &e),
                }
            }
            // The PR changed since it was read (`pr start` got there first): decided again next tick.
            Err(Failed::Other(e)) if e.code() == ErrorCode::CasLost => {
                engine.log.line(&format!("E7: skipped PR {key} of Plan {plan}: {}", e.message()));
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
    crate::metrics::parse_file(&file).ok().map(|m| m.to_json())
}

fn catch_signals() -> Result<(), CpdError> {
    let action = SigAction::new(SigHandler::Handler(on_signal), SaFlags::empty(), SigSet::empty());
    for signal in [Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP] {
        // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
        unsafe { sigaction(signal, &action) }.map_err(|e| {
            CpdError::new(crate::error::ErrorCode::StoreFailed, format!("Cannot handle {signal}: {e}."))
        })?;
    }
    Ok(())
}

fn watch_event(store: &mut Store, kind: Kind, text: String) -> Result<i64, CpdError> {
    store.write(SOURCE, |tx| events::append(tx, NewEvent::new(kind, text, json!({"pid": std::process::id()}))))
}

/// `cpd watch`: the engine in the foreground.
pub(super) fn run() -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let _lock = WatchLock::take(&home)?;
    let mut log = Log::open(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    catch_signals()?;
    let pid = std::process::id();
    watch_event(&mut store, Kind::WatchStarted, format!("The engine started (pid {pid}). See: cpd doctor"))?;
    log.line(&format!("started (pid {pid}), a tick every {}s", config.watch.tick_seconds));

    let tmux = Tmux::new(config.tmux.socket.as_str());
    let report = Report::new(Duration::from_secs(u64::from(config.watch.error_repeat_seconds)));
    let mut engine = Engine {
        home,
        config,
        store,
        tmux,
        log,
        report,
        catalog: None,
        last_poll_end: None,
        paused_until: None,
        synced: None,
    };
    import(&mut engine);

    let tick = Duration::from_secs(u64::from(engine.config.watch.tick_seconds));
    while !stopping() {
        let began = Instant::now();
        engine.tick();
        while !stopping() && began.elapsed() < tick {
            std::thread::sleep(NAP.min(tick.saturating_sub(began.elapsed())));
        }
    }

    engine.log.line("stopping on a signal");
    if let Err(e) = watch_event(&mut engine.store, Kind::WatchStopped, format!("The engine stopped (pid {pid}). Start it: cpd watch --ensure")) {
        engine.log.line(&format!("cannot append watch-stopped: {} ({})", e.message(), e.code().as_str()));
    }
    Ok(Output::success(json!({"stopped": true})))
}

/// At start, the import of every Context store (§4.6, commands.md §12).
fn import(engine: &mut Engine) {
    let result = Contexts::new(&engine.home, &engine.config, &mut engine.store).import_all(SOURCE);
    match result {
        Ok(0) => engine.clear(Step::Start, &About::Machine),
        Ok(n) => {
            engine.log.line(&format!("imported {n} Context record(s)"));
            engine.clear(Step::Start, &About::Machine);
        }
        Err(e) => engine.fail(Step::Start, About::Machine, &e),
    }
}
