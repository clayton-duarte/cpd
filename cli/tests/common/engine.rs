//! The engine, `cpd watch`, in-process on an [`Fx`] (docs/implementing.md, "Ports, Spawn and run_with", "Virtual-time
//! loops"): `watch::run_with(ports, until)` on the `Fx`'s `ManualClock`, `FakeRunner` and `FakeSpawn`, no process and no
//! wait. The engine's lock, store and log are real files of the temp home.
//!
//! The loop ends when `until` is set. [`Fx::watch`] sets it once the loop has napped `stop_after` times: the first nap
//! comes after the first tick, so `stop_after = 1` is "until the first tick". A `hook` runs before each nap passes, with
//! the nap's number (1-based): what a test does between two ticks, on the one thread, with no second reader of the time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpd_core::clock::{Clock, ManualClock, timestamp};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use serde_json::json;
use time::OffsetDateTime;

use super::fx::Fx;

/// The `ManualClock` of an `Fx` that also runs `hook` on each nap and sets `until` after the `stop_after`th. It reads and
/// advances time only through the `ManualClock` it holds, so `Fx::advance` and `fakes.clock` see the same time.
struct Stepped {
    inner: ManualClock,
    until: Arc<AtomicBool>,
    stop_after: usize,
    hook: Mutex<Box<dyn FnMut(usize) + Send>>,
}

impl Clock for Stepped {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        let nap = self.inner.naps().len() + 1;
        (self.hook.lock().unwrap())(nap);
        self.inner.sleep(d);
        if nap >= self.stop_after {
            self.until.store(true, Ordering::SeqCst);
        }
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

impl Fx {
    /// `config.toml` of machine `m1` for the engine on virtual time: a tick a second, no PR poll in the way (3600 s).
    /// `extra` is more TOML before the tables (top-level keys, or tables), as `TestEnv::write_tmux_config`'s.
    pub fn engine_config(&self, extra: &str) {
        self.env.write_config(&format!(
            "version = 1\nmachine = \"m1\"\n{extra}\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [watch]\ntick_seconds = 1\npr_poll_seconds = 3600\n"
        ));
    }

    /// What the engine's own calls get when a test does not care: tmux has no server, and `cpd pr poll` (E5, through the
    /// runner) finds no pull request.
    pub fn engine_routes(&self) {
        self.fake("tmux", Reply::exit(1, "", "no server running on /tmp/tmux-1/testkit\n"));
        let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
        self.fake("cpd", Reply::ok(&poll.to_string()));
    }

    /// `cpd watch` in `CPD_HOME` on this `Fx`'s ports until it has napped `stop_after` times (see the module), `hook`
    /// running before each nap. It returns what `cpd watch` returns when it is stopped.
    pub fn watch(&self, stop_after: usize, hook: impl FnMut(usize) + Send + 'static) -> Result<Output, CpdError> {
        let until = Arc::new(AtomicBool::new(false));
        let stepped = Stepped { inner: (*self.fakes.clock).clone(), until: until.clone(), stop_after, hook: Mutex::new(Box::new(hook)) };
        let ports = Ports { clock: Arc::new(stepped), runner: self.fakes.ports.runner.clone(), spawn: self.fakes.ports.spawn.clone() };
        cpd_engine::watch::run_with(&ports, &until)
    }

    /// [`Fx::watch`] until the first tick has run.
    pub fn watch_first_tick(&self) -> Result<Output, CpdError> {
        self.watch(1, |_| {})
    }

    /// The `ManualClock`'s time `ms` milliseconds before now (negative: after it), as the store writes times.
    pub fn stamp_ago(&self, ms: i64) -> String {
        timestamp(self.utc_ago(ms))
    }

    /// The `ManualClock`'s UTC time `ms` milliseconds before now (negative: after it).
    pub fn utc_ago(&self, ms: i64) -> OffsetDateTime {
        self.fakes.clock.utc() - time::Duration::milliseconds(ms)
    }
}
