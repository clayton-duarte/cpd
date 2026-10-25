//! The engine, `cpd watch`, in-process on an [`Fx`] (docs/implementing.md, "Ports, Spawn and run_with", "Virtual-time
//! loops"): `watch::run_with(ports, until)` on the `Fx`'s `ManualClock`, `FakeRunner` and `FakeSpawn`, no process and no
//! wait. The engine's lock, store and log are real files of the temp home.
//!
//! The loop ends when `until` is set. [`Fx::watch`] sets it once the loop has napped `stop_after` times: the first nap
//! comes after the first tick, so `stop_after = 1` is "until the first tick". A `hook` runs before each nap passes, with
//! the nap's number (1-based): what a test does between two ticks, on the one thread, with no second reader of the time.
//! [`Fx::watch_steps`] is the general form: its `step` runs like the hook and says whether this is the last nap, so a test
//! runs the engine until a state it can read (a file, a row) holds. A step that never says so ends the test after
//! [`MAX_NAPS`] naps, with a panic: virtual time, no wall clock.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use cpd_core::clock::{Clock, ManualClock, timestamp};
use cpd_core::error::CpdError;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::output::Output;
use cpd_core::ids::PlanId;
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use serde_json::json;
use time::OffsetDateTime;

use super::fx::{Fx, exit_code};

/// The Plan of [`Fx::engine_plan`].
pub const PLAN: &str = "pay";

/// The most naps [`Fx::watch_steps`] lets a step ask for: 600 virtual seconds of the engine's tenth-of-a-second nap.
pub const MAX_NAPS: usize = 6000;

type Step = Box<dyn FnMut(usize) -> bool + Send>;

/// The executable the fake engine "started from" (`Spawn::self_exe`): E5 runs it with `pr poll`.
pub const ENGINE_EXE: &str = "/opt/cpd/bin/cpd";

/// The `ManualClock` of an `Fx` that also runs `step` on each nap and sets `until` after the nap it says is the last. It
/// reads and advances time only through the `ManualClock` it holds, so `Fx::advance` and `fakes.clock` see the same time.
struct Stepped {
    inner: ManualClock,
    until: Arc<AtomicBool>,
    step: Mutex<Step>,
    /// The thread that runs the engine, and what to do when another thread reads the clock ([`Fx::watch_gated`]).
    engine_thread: ThreadId,
    on_other_thread: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Clock for Stepped {
    fn now(&self) -> Instant {
        let now = self.inner.now();
        if let Some(hook) = &self.on_other_thread
            && std::thread::current().id() != self.engine_thread
        {
            hook();
        }
        now
    }
    fn sleep(&self, d: Duration) {
        let nap = self.inner.naps().len() + 1;
        assert!(nap <= MAX_NAPS, "the engine napped {MAX_NAPS} times (virtual time) and the step never said it was done");
        let last = (self.step.lock().unwrap())(nap);
        self.inner.sleep(d);
        if last {
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
    /// runner, from the executable the engine recorded: [`ENGINE_EXE`]) finds no pull request.
    pub fn engine_routes(&self) {
        self.engine_routes_without_poll();
        let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
        self.fake("cpd", Reply::ok(&poll.to_string()));
    }

    /// [`Fx::engine_routes`] with the poll run for real, in this process: `cpd pr poll` is [`Fx::cpd_runs_in_process`], so the
    /// poll's own `gh` calls go to the `gh` of the `FakeRunner` (script it with [`Fx::fake_gh`] or [`Fx::route_fn`]).
    pub fn engine_routes_polling(&self) {
        self.engine_routes_without_poll();
        self.cpd_runs_in_process();
    }

    fn engine_routes_without_poll(&self) {
        self.fakes.spawn.set_self_exe(ENGINE_EXE);
        self.fake("tmux", Reply::exit(1, "", "no server running on /tmp/tmux-1/testkit\n"));
    }

    /// Makes every call of `cpd` through the `FakeRunner` a `run_with` of this `Fx`'s ports, on the calling thread: the
    /// reply is what the command wrote to stdout and its exit code. The call's environment and directory are not applied
    /// (the process's are the `Fx`'s, set once; `CPD_HOME` is the same for the engine and the command it runs).
    pub fn cpd_runs_in_process(&self) {
        // The runner is held weakly: the route lives in the runner's own script and calls it back.
        let ports = Ports { clock: self.fakes.ports.clock.clone(), runner: self.fakes.runner.weak(), spawn: self.fakes.ports.spawn.clone() };
        let _ = (*self.fakes.runner).clone().route("cpd", Arc::new(CpdRoute { ports }));
    }

    /// Answers every call of `program` with `reply(call)`, in the thread that makes it: a script that depends on the call or
    /// on what the test changed since the last one.
    pub fn route_fn(&self, program: &str, reply: impl Fn(&Call) -> Reply + Send + Sync + 'static) {
        let _ = (*self.fakes.runner).clone().route(program, Arc::new(FnRoute(Box::new(reply))));
    }

    /// Context `ctx` and Plan `pay` in `dispatch` (PR `repo` in `dispatch`, its Lead Session `lead1` stopped, the Tier
    /// catalog of pi 1.0.0 seeded): the Plan of the engine tests. Its repo and Lead worktree are paths that are not on disk:
    /// a step that runs git needs a repo of its own.
    pub fn engine_plan(&self) {
        let catalog: String = [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")]
            .iter()
            .map(|(letter, model, efforts)| {
                format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
            })
            .collect();
        let repo = self.env.root().join("pay");
        let lead = self.env.root().join("pay.pay");
        self.env.sql(&format!(
            "{catalog}
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', '{PLAN}', 'Lead', '/', 'O3', 'stopped', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('{PLAN}', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
               VALUES ('{PLAN}', 'repo', '{}', 'main', 'dispatch', 'cpd/{PLAN}', '{}', 'Charge the card at checkout', 'config', cpd_txn(), 't', 't');",
            repo.display(),
            lead.display()
        ));
    }

    /// Job `key` of Plan `pay` in PR `repo`, Tier `S2`, `queued` with its Locked-Job fingerprint and its `needs`, then moved
    /// by `then` (SQL run after the fingerprint is set).
    pub fn add_job(&self, key: &str, needs: &[&str], then: &str) {
        self.add_job_in("repo", key, needs, then);
    }

    /// [`Fx::add_job`] in PR `pr` of Plan `pay`.
    pub fn add_job_in(&self, pr: &str, key: &str, needs: &[&str], then: &str) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('{PLAN}', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        );
        for need in needs {
            sql += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{PLAN}', '{key}', '{need}');");
        }
        self.env.sql(&sql);
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.env.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.env.sql(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{PLAN}' AND key = '{key}'; {then}",
            locked_fingerprint(job)
        ));
    }

    /// `cpd watch` in `CPD_HOME` on this `Fx`'s ports until it has napped `stop_after` times (see the module), `hook`
    /// running before each nap. It returns what `cpd watch` returns when it is stopped.
    pub fn watch(&self, stop_after: usize, mut hook: impl FnMut(usize) + Send + 'static) -> Result<Output, CpdError> {
        self.watch_steps(move |nap| {
            hook(nap);
            nap >= stop_after
        })
    }

    /// `cpd watch` in `CPD_HOME` on this `Fx`'s ports, `step` running before each nap passes (with its 1-based number): it
    /// returns whether that nap is the last. What `step` reads is the state after the tick that came before the nap.
    pub fn watch_steps(&self, step: impl FnMut(usize) -> bool + Send + 'static) -> Result<Output, CpdError> {
        self.watch_steps_on(step, None)
    }

    /// [`Fx::watch_steps`] where `poll_ended` runs each time a thread other than the test's reads the engine's clock. The
    /// engine's only other thread is E5's poll, and it reads the clock once, to stamp the end of the poll, as the last thing
    /// it does before it hands the result over: a `step` that waits for what `poll_ended` signals stops the engine's
    /// virtual time while a poll runs, so the poll ends at a time the test knows (the engine's own thread would otherwise
    /// nap through virtual time while the poll thread is starting).
    pub fn watch_gated(&self, step: impl FnMut(usize) -> bool + Send + 'static, poll_ended: impl Fn() + Send + Sync + 'static) -> Result<Output, CpdError> {
        self.watch_steps_on(step, Some(Box::new(poll_ended)))
    }

    fn watch_steps_on(&self, step: impl FnMut(usize) -> bool + Send + 'static, on_other_thread: Option<Box<dyn Fn() + Send + Sync>>) -> Result<Output, CpdError> {
        let until = Arc::new(AtomicBool::new(false));
        let stepped = Stepped {
            inner: (*self.fakes.clock).clone(),
            until: until.clone(),
            step: Mutex::new(Box::new(step)),
            engine_thread: std::thread::current().id(),
            on_other_thread,
        };
        let ports = Ports { clock: Arc::new(stepped), runner: self.fakes.ports.runner.clone(), spawn: self.fakes.ports.spawn.clone() };
        cpd_engine::watch::run_with(&ports, &until)
    }

    /// `cpd watch` on this `Fx`'s ports until the engine has napped `n` more times than before this call: `n` ticks, whatever
    /// engines ran earlier on the same clock (a test that looks at the home between two runs).
    pub fn watch_ticks(&self, n: usize) -> Result<Output, CpdError> {
        let from = self.fakes.clock.naps().len();
        self.watch(from + n, |_| {})
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

impl Fx {
    /// PR `key` of Plan `pay` in `dispatch`, on the repo and Lead worktree of [`Fx::engine_plan`].
    pub fn add_pr(&self, key: &str, title: &str) {
        let repo = self.env.root().join("pay");
        let lead = self.env.root().join("pay.pay");
        self.env.sql(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
               VALUES ('{PLAN}', '{key}', '{}', 'main', 'dispatch', 'cpd/{PLAN}/{key}', '{}', '{title}', 'config', cpd_txn(), 't', 't');",
            repo.display(),
            lead.display()
        ));
    }

    /// Job `key` of PR `pr` a `draft`: the PR's open Job waits for it, and E3 never starts it.
    pub fn add_draft_in(&self, pr: &str, key: &str) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('{PLAN}', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, cpd_txn(), 't');"
        ));
    }

    /// Job `api` of PR `repo` merged: the PR's open Job can be Ready once nothing else of the PR is left.
    pub fn add_merged_api(&self) {
        self.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    }

    /// Job `key` of PR `pr` as cpd adds an `open` or `update` Job (`kind`): `appended`, no Dependencies, `queued` with its
    /// fingerprint, then moved by `then` (SQL).
    pub fn add_cpd_job_in(&self, pr: &str, key: &str, kind: &str, then: &str) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
             VALUES ('{PLAN}', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
        ));
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.env.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.env.sql(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{PLAN}' AND key = '{key}'; {then}",
            locked_fingerprint(job)
        ));
    }

    /// `(key, status)` of every Job of `kind`, by key.
    pub fn jobs_of_kind(&self, kind: &str) -> Vec<(String, String)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT key, status FROM jobs WHERE kind = ?1 ORDER BY key")?;
            Ok(stmt.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}

/// `cpd` run by `run_with` on the ports it holds.
struct CpdRoute {
    ports: Ports,
}

impl Handler for CpdRoute {
    fn handle(&self, call: &Call) -> Reply {
        let argv = std::iter::once(call.program.clone()).chain(call.args.iter().cloned()).map(OsString::from);
        let mut stdout = Vec::new();
        let code = cpd::run_with(&self.ports, argv, &mut stdout);
        Reply::Exit { code: exit_code(code), stdout, stderr: Vec::new(), takes: Duration::ZERO }
    }
}

type Reply1 = Box<dyn Fn(&Call) -> Reply + Send + Sync>;

/// A [`Handler`] that is a closure.
struct FnRoute(Reply1);

impl Handler for FnRoute {
    fn handle(&self, call: &Call) -> Reply {
        (self.0)(call)
    }
}
