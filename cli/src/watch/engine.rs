//! The engine's loop (contract §3.7; commands.md §12): `locks/watch.lock`,
//! `watch-started`, the import of the Context stores, then E1–E6 every
//! `tick_seconds` until SIGTERM or SIGINT, then `watch-stopped`.

use std::collections::HashMap;
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
use crate::error::CpdError;
use crate::events::{self, Kind, NewEvent};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::host::pi::{self, PiDirs};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;
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
    /// When E5 last ran `pr poll` for each Plan.
    pub polled: HashMap<PlanId, Instant>,
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

    /// One tick: E1–E6 (§3.7), each reporting its own failures.
    fn tick(&mut self) {
        self.catalog = None;
        let steps: [fn(&mut Engine); 6] =
            [escalations::run, liveness::run, schedule::run, completion::run, pr::run, contexts::run];
        for step in steps {
            if stopping() {
                return;
            }
            step(self);
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
    for signal in [Signal::SIGTERM, Signal::SIGINT] {
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

/// `cpd2 watch`: the engine in the foreground.
pub(super) fn run() -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let _lock = WatchLock::take(&home)?;
    let mut log = Log::open(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    catch_signals()?;
    let pid = std::process::id();
    watch_event(&mut store, Kind::WatchStarted, format!("The engine started (pid {pid}). See: cpd2 doctor"))?;
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
        polled: HashMap::new(),
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
    if let Err(e) = watch_event(&mut engine.store, Kind::WatchStopped, format!("The engine stopped (pid {pid}). Start it: cpd2 watch --ensure")) {
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
