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

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use cpd_core::clock::{Clock, ManualClock, timestamp};
use cpd_core::error::CpdError;
use cpd_core::events::{NewEvent, append};
use cpd_core::fakes::tmux::{Invocation, TmuxRoute};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::output::Output;
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
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

/// The text of the engine's `config.toml`: machine `m1`, `extra`, a tick a second, no PR poll, then `tail` (more tables).
fn engine_config_text(extra: &str, tail: &str) -> String {
    format!(
        "version = 1\nmachine = \"m1\"\n{extra}\n\
         [watch]\ntick_seconds = 1\npr_poll_seconds = 3600\n{tail}"
    )
}

/// The `ManualClock` of an `Fx` that also runs `step` on each nap and sets `until` after the nap it says is the last. It
/// reads and advances time only through the `ManualClock` it holds, so `Fx::advance` and `fakes.clock` see the same time.
struct Stepped {
    inner: ManualClock,
    until: Arc<AtomicBool>,
    step: Mutex<Step>,
    /// The thread that runs the engine, and what to run once per other thread, at that thread's exit, after its last
    /// statement (so after the poll's send): registered on the thread's first clock read ([`Fx::watch_gated`]).
    engine_thread: ThreadId,
    on_other_thread: Option<Hook>,
}

type Hook = Arc<dyn Fn() + Send + Sync>;

/// Runs its hook when dropped: a thread-local, so at the exit of the thread that read the clock. The hook must not panic
/// (a panic in a thread-local destructor aborts the process).
struct Guard(Hook);

impl Drop for Guard {
    fn drop(&mut self) {
        (self.0)()
    }
}

thread_local! {
    static ENDED: RefCell<Option<Guard>> = const { RefCell::new(None) };
}

impl Clock for Stepped {
    fn now(&self) -> Instant {
        let now = self.inner.now();
        if let Some(hook) = &self.on_other_thread
            && std::thread::current().id() != self.engine_thread
        {
            ENDED.with(|ended| {
                ended.borrow_mut().get_or_insert_with(|| Guard(hook.clone()));
            });
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
        self.env.write_models();
        self.env.quiet_models(self.fakes.clock.utc());
        self.env.write_config(&engine_config_text(extra, ""));
    }

    /// [`Fx::engine_config`] on the tmux server of [`Fx::tmux`] (the env's socket): the engine and the test's own tmux calls
    /// reach the same server of the tmux route.
    pub fn engine_config_on_tmux(&self, extra: &str) {
        self.env.write_models();
        self.env.quiet_models(self.fakes.clock.utc());
        self.env.write_config(&engine_config_text(extra, &format!("[tmux]\nsocket = \"{}\"\n", self.env.tmux_socket())));
    }

    /// What the engine's own calls get when a test does not care: tmux has no server, and `cpd pr poll` (E5, through the
    /// runner, from the executable the engine recorded: [`ENGINE_EXE`]) finds no pull request.
    pub fn engine_routes(&self) {
        self.engine_base_routes();
        let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
        self.fake("cpd", Reply::ok(&poll.to_string()));
    }

    /// [`Fx::engine_routes`] with the poll run for real, in this process: `cpd pr poll` is [`Fx::cpd_runs_in_process`], so the
    /// poll's own `gh` calls go to the `gh` of the `FakeRunner` (script it with [`Fx::fake_gh`] or [`Fx::route_fn`]).
    pub fn engine_routes_polling(&self) {
        self.engine_base_routes();
        self.cpd_runs_in_process();
    }

    /// What every engine test needs of the runner, with no answer for `cpd pr poll` yet: the engine's executable
    /// ([`ENGINE_EXE`]) and a tmux that has no server.
    pub fn engine_base_routes(&self) {
        self.fakes.spawn.set_self_exe(ENGINE_EXE);
        self.fake("tmux", Reply::exit(1, "", "no server running on /tmp/tmux-1/testkit\n"));
    }

    /// [`Fx::engine_routes`] with the tmux route left to answer, not a tmux with no server: the fake server of [`Fx::tmux`] is the
    /// engine's (give it the config of [`Fx::engine_config_on_tmux`]). Faults: `fx.fakes.runner` `once`/`on` (the tmux route docs).
    pub fn engine_routes_on_tmux(&self) {
        self.fakes.spawn.set_self_exe(ENGINE_EXE);
        let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
        self.fake("cpd", Reply::ok(&poll.to_string()));
    }

    /// Makes every call of `cpd` through the `FakeRunner` a `run_with` of this `Fx`'s ports, on the calling thread: the
    /// reply is what the command wrote to stdout and its exit code. The call's environment and directory are not applied
    /// (the process's are the `Fx`'s, set once; `CPD_HOME` is the same for the engine and the command it runs).
    pub fn cpd_runs_in_process(&self) {
        // The runner is held weakly: the route lives in the runner's own script and calls it back.
        let ports = Ports { clock: self.fakes.ports.clock.clone(), runner: self.fakes.runner.weak(), spawn: self.fakes.ports.spawn.clone(), ..Ports::real() };
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
        self.engine_plan_at(&self.env.root().join("pay"), &self.env.root().join("pay.pay"));
    }

    /// [`Fx::engine_plan`] with PR `repo` on the repo `repo` and the Lead worktree `lead` (a repo and a worktree of the fake git).
    /// The pi release the catalog is of is the installed one (`install/current-version` and the release's entry points, which
    /// E2's ladder looks for and nothing runs).
    pub fn engine_plan_at(&self, repo: &Path, lead: &Path) {
        self.engine_plan_on(repo, lead, &format!("cpd/{PLAN}/repo"));
    }

    /// [`Fx::engine_plan_at`] with the Lead Branch `lead_branch` the Lead worktree `lead` really has (a worktree made by
    /// `create_lead_worktree` is on `cpd/<plan>`): E2's merge recovery tests the Attempt against the stored name.
    pub fn engine_plan_on(&self, repo: &Path, lead: &Path, lead_branch: &str) {
        let install = self.env.root().join("install");
        std::fs::create_dir_all(&install).expect("create install/");
        std::fs::write(install.join("current-version"), "1.0.0\n").expect("write current-version");
        let scope = install.join("releases/1.0.0/node_modules/@earendil-works");
        for package in ["pi-coding-agent", "pi-ai"] {
            std::fs::create_dir_all(scope.join(package).join("dist")).expect("create dist");
            std::fs::write(scope.join(package).join("dist/index.js"), "").expect("write index.js");
        }
        let catalog: String = [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")]
            .iter()
            .map(|(letter, model, efforts)| {
                format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
            })
            .collect();
        self.env.sql(&format!(
            "{catalog}
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', '{PLAN}', 'Lead', '/', 'O3', 'stopped', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('{PLAN}', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, description, shape, created_txn, created_at, started_at)
               VALUES ('{PLAN}', 'repo', '{}', 'main', 'dispatch', '{lead_branch}', '{}', 'Charge the card at checkout', 'A description', 'config', cpd_txn(), 't', 't');",
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
        self.add_job_on("S2", pr, key, needs, then);
    }

    /// [`Fx::add_job_in`] with the Job's Tier `tier`.
    pub fn add_job_on(&self, tier: &str, pr: &str, key: &str, needs: &[&str], then: &str) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('{PLAN}', '{pr}', '{key}', 'Title of {key}', '{tier}', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
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

    /// [`Fx::watch_steps`] where `poll_ended` runs once per thread other than the test's that reads the engine's clock, at
    /// that thread's exit, after its last statement. The engine's only other thread is E5's poll: it reads the clock to
    /// stamp the end of the poll, then sends its result, so `poll_ended` runs at the poll thread's exit, after its send,
    /// and the result is already on the channel. A `step` that waits for what `poll_ended` signals stops the engine's
    /// virtual time while a poll runs, so the poll ends at a time the test knows. `poll_ended` must not panic.
    pub fn watch_gated(&self, step: impl FnMut(usize) -> bool + Send + 'static, poll_ended: impl Fn() + Send + Sync + 'static) -> Result<Output, CpdError> {
        self.watch_steps_on(step, Some(Arc::new(poll_ended)))
    }

    fn watch_steps_on(&self, step: impl FnMut(usize) -> bool + Send + 'static, on_other_thread: Option<Hook>) -> Result<Output, CpdError> {
        let until = Arc::new(AtomicBool::new(false));
        let stepped = Stepped {
            inner: (*self.fakes.clock).clone(),
            until: until.clone(),
            step: Mutex::new(Box::new(step)),
            engine_thread: std::thread::current().id(),
            on_other_thread,
        };
        let ports = Ports { clock: Arc::new(stepped), runner: self.fakes.ports.runner.clone(), spawn: self.fakes.ports.spawn.clone(), ..Ports::real() };
        // The steps that reach git or tmux through the core's wrappers (E2's merge recovery, a Worker's window) read `Ports::current()`.
        Ports::scope(&ports, || cpd_engine::watch::run_with(&ports, &until))
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
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, description, shape, created_txn, created_at, started_at)
               VALUES ('{PLAN}', '{key}', '{}', 'main', 'dispatch', 'cpd/{PLAN}/{key}', '{}', '{title}', 'A description', 'config', cpd_txn(), 't', 't');",
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

impl Fx {
    /// Job `key` of Plan `pay`, `running` with Attempt `<key>-a1` (`status`: `starting` or `running`) started at `started_at`,
    /// its driver pid 4242 and its worktree beside the repo's. The `starting → running` Event is at `started_at` when the
    /// Attempt is `running`.
    pub fn add_running(&self, key: &str, status: &str, started_at: &str) {
        self.add_running_since(key, status, started_at, (status == "running").then_some(started_at));
    }

    /// [`Fx::add_running`] with the `starting → running` Event at `running_at` (none: the Attempt is still `starting`): an
    /// Attempt that waited in `starting`.
    pub fn add_running_since(&self, key: &str, status: &str, started_at: &str, running_at: Option<&str>) {
        let event = running_at
            .map(|at| {
                format!(
                    "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
                       VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', '{PLAN}', '{key}', '{key}-a1', 'running',
                               '{{\"from\":\"starting\",\"to\":\"running\"}}');"
                )
            })
            .unwrap_or_default();
        let worktree = self.env.root().join(format!("pay.pay--{key}-a1"));
        self.add_job(
            key,
            &[],
            &format!(
                "UPDATE jobs SET status = 'running' WHERE plan = '{PLAN}' AND key = '{key}';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
                   VALUES ('{PLAN}', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', 'cpd/{PLAN}--{key}-a1', '{}', '{started_at}', 4242);
                 {event}",
                worktree.display()
            ),
        );
    }

    /// Job `key` has a cancel request, as `cpd job cancel` of a running Job leaves it, made at the clock's now.
    pub fn request_cancel(&self, key: &str) {
        self.env.sql(&format!(
            "UPDATE jobs SET cancel_reason = 'not needed', cancel_requested_at = '{}' WHERE plan = '{PLAN}' AND key = '{key}';",
            self.stamp_ago(0)
        ));
    }

    /// Holds Attempt `attempt`'s driver lock, as a live driver does: a `flock` on our own lock file of the temp home (kept real,
    /// it is not external software). The lock goes with the value the test holds.
    pub fn live_driver(&self, attempt: &str) -> Flock<File> {
        let home = CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap();
        let path = home.attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(attempt).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
        Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap()
    }

    /// A tmux server with one session, `keepalive`, on the tmux route ([`Fx::engine_config_on_tmux`]).
    pub fn tmux_server(&self) {
        assert!(self.tmux(&["new-session", "-d", "-s", "keepalive", "sleep", "600"]).status.success());
    }

    /// The server option `name` (`show -gv`), without its trailing newline: empty when it is not set.
    pub fn tmux_option(&self, name: &str) -> String {
        String::from_utf8_lossy(&self.tmux(&["show", "-gv", name]).stdout).trim_end().to_owned()
    }
}

/// What becomes of the window `cpd-work:watch` that `cpd watch --ensure` makes ([`Fx::engine_window`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineWindow {
    /// The engine starts: its pane has a process that holds `locks/watch.lock` and says `watch-started` with its pid.
    Runs,
    /// The command of the window exits with this status.
    Exits(i32),
    /// A signal killed the command of the window: tmux has no exit status for it.
    IsKilled,
}

/// The engine that `cpd watch --ensure` waits for, simulated: the fake tmux runs no program, so the test says what the window's
/// process does. Call [`Engines::at_nap`] from [`Fx::on_nap`]: `--ensure` naps while it waits for the engine, and the process
/// of the window is "alive" (and has done what [`EngineWindow`] says) when it looks again. A real `flock` on our own lock file.
#[derive(Clone)]
pub struct Engines {
    route: Arc<TmuxRoute>,
    socket: String,
    tmpdir: PathBuf,
    cwd: PathBuf,
    home: cpd_core::home::CpdHome,
    how: EngineWindow,
    /// `locks/watch.lock` while a simulated engine runs.
    lock: Arc<Mutex<Option<Flock<File>>>>,
}

impl Engines {
    /// `tmux <args>` on the fake server of the env's socket, not through the runner (it is no call of the product's).
    fn tmux(&self, args: &[&str]) -> Option<String> {
        let argv = ["-L", &self.socket].into_iter().chain(args.iter().copied()).map(str::to_owned).collect();
        let out = self.route.fake().run(&Invocation { argv, tmux_tmpdir: self.tmpdir.clone(), cwd: self.cwd.clone(), env: Vec::new() });
        (out.code == 0).then_some(out.stdout)
    }

    /// What the process of the engine window does by the time `--ensure` has napped once more.
    pub fn at_nap(&self) {
        // No server yet: the call has not made the window.
        let Some(rows) = self.tmux(&["list-panes", "-a", "-F", "#{pane_id} #{session_name}:#{window_name} #{pane_dead} #{pane_pid}"]) else {
            return;
        };
        for row in rows.lines() {
            let fields: Vec<&str> = row.split(' ').collect();
            let [pane, window, dead, pid] = fields[..] else { panic!("a pane row: {row:?}") };
            if window != "cpd-work:watch" || dead == "1" {
                continue;
            }
            match self.how {
                EngineWindow::Runs => self.runs(pid.parse().expect("a pane pid")),
                EngineWindow::Exits(status) => {
                    self.tmux(&["__ctl", "pane-exit", pane, "--status", &status.to_string()]).expect("pane-exit");
                }
                EngineWindow::IsKilled => {
                    self.tmux(&["__ctl", "pane-exit", pane, "--signal"]).expect("pane-exit");
                }
            }
        }
    }

    /// The process `pid` is the engine: it takes the lock and says it started, once.
    fn runs(&self, pid: u32) {
        let mut store = Store::open(&self.home, 5000).expect("open the store");
        let known = store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM events WHERE kind = 'watch-started' AND json_extract(data, '$.pid') = ?1)",
                    [pid],
                    |r| r.get::<_, bool>(0),
                )?)
            })
            .expect("read the events");
        if known {
            return;
        }
        let path = self.home.watch_lock();
        std::fs::create_dir_all(path.parent().expect("the lock's directory")).expect("create locks/");
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).expect("open the engine's lock");
        let lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).expect("the engine's lock is free when an engine starts");
        *self.lock.lock().unwrap() = Some(lock);
        store
            .write("engine", |tx| append(tx, NewEvent::watch_started(format!("The engine started (pid {pid})."), pid)).map(|_| ()))
            .expect("append watch-started");
    }

    /// The simulated engine dies: the kernel lets go of its lock.
    pub fn stop(&self) {
        self.lock.lock().unwrap().take();
    }
}

impl Fx {
    /// The engine of the window `watch --ensure` makes, simulated as `how` says ([`Engines`]).
    pub fn engine_window(&self, how: EngineWindow) -> Engines {
        Engines {
            route: self.tmux_route.clone(),
            socket: self.env.tmux_socket().to_owned(),
            tmpdir: self.env.tmux_tmpdir().to_path_buf(),
            cwd: self.env.root().to_path_buf(),
            home: CpdHome::resolve(Some(self.env.cpd_home().into()), None).expect("the home"),
            how,
            lock: Arc::default(),
        }
    }
}

/// What a `pi --mode json` one-shot run prints (shapes recorded from pi 1.3.0, as [`super::PI_REPLY`]): a stream whose final
/// assistant message holds `answer`.
pub fn pi_stream(answer: &str) -> String {
    let message = json!({"type": "message_end", "message": {
        "role": "assistant",
        "content": [{"type": "thinking", "thinking": "hm"}, {"type": "text", "text": answer}],
        "usage": {"input": 2, "output": 4, "cacheRead": 100, "cacheWrite": 631, "reasoning": 1, "totalTokens": 737,
                  "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0.25}},
        "stopReason": "stop",
    }});
    format!(
        "{}\n{}\n{message}\n{}\n",
        json!({"type": "session", "version": 3, "id": "x", "timestamp": "2026-10-19T01:59:25.851Z", "cwd": "/"}),
        json!({"type": "agent_start"}),
        json!({"type": "agent_settled", "aborted": false})
    )
}

/// The distiller's answer that curates every record of `batch_md` (the Context's `batch.md`): one valid JSON line per block,
/// with no retirement and no doc candidate ([`super::CURATES_ALL`] in Rust).
pub fn curates_all(batch_md: &str) -> String {
    batch_md
        .lines()
        .filter_map(|line| line.strip_prefix('[')?.split_once(']'))
        .map(|(id, _)| json!({"id": id, "topic": "t", "short": format!("s {id}"), "retire": null, "doc": false}).to_string() + "\n")
        .collect()
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
