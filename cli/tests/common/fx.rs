//! `Fx`: a test env whose `cpd` commands run in the test process, through `cpd::main_with`, instead of as a child
//! process (an exec costs 10-100x more on a slow machine; docs/implementing.md, Tests).
//!
//! `Fx` wraps a [`TestEnv`] for the temporary `$CPD_HOME`, `$HOME`, config, policy and store, and gives `cpd`, `cpd_in`
//! and `cpd_with` the signatures and the [`Run`] of `TestEnv`'s.
//!
//! The product reads the process's environment and current directory (`CpdHome::from_env`, `locate_pi`, the `txns`
//! row's `cwd`, ...), which are global to the process. So:
//! - [`Fx::new`] takes a process-wide lock held for the `Fx`'s whole life, then sets the variables of
//!   [`TestEnv::isolation`] once, and `Drop` puts the environment and the directory back as they were. Under nextest
//!   (a process per test) the lock is never contended; under a plain `cargo test` (threads of one process) it makes two
//!   `Fx` never overlap. A test that does not use `Fx` and reads the environment meanwhile is not covered: that is why
//!   a file is migrated whole.
//! - `cpd_with` sets the extra variables and `cpd_in` the directory around the one call, and restores them after it,
//!   also when the call panics.
//!
//! What `Fx` does not cover stays on `TestEnv::cpd` (the binary): a command that runs the recorded executable again
//! (`pr poll`, `attempt run`, `session reload`, `watch`, a detached driver), `slot` (it exits the process), a signal, a
//! terminal, `events follow`, and a command that owns its stdout (`print!`: `context pending --markdown`; `Fx::cpd`
//! panics on it, as it reads nothing). stdin is the test process's own. stderr is not captured: `Run::stderr` is empty (what the
//! product writes there goes to the test's own stderr, which nextest shows only for a failed test).

use std::cell::{Cell, OnceCell};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use nix::fcntl::{Flock, FlockArg};
use nix::unistd::{dup, dup2_stderr, dup2_stdin, dup2_stdout};
use rusqlite::Connection;
use time::OffsetDateTime;

use cpd_core::clock::{Clock, ManualClock};
use cpd_core::error::CpdError;
use cpd_core::fakes::git::{GitRoute, Repo, Universe};
use cpd_core::fakes::tmux::{Invocation, TmuxRoute};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::host::spawn::{FakeSpawn, Launch, Pid, Signal, Spawn};
use cpd_core::host::tmux::SERVER_OPTIONS;
use cpd_core::ports::{Fakes, Ports};

use super::{Run, TestEnv};

/// What [`Fx::until`] advances the `ManualClock` by between two looks (the engine tests' tick, `TICK_SECONDS`).
const TICK: Duration = Duration::from_millis(100);

/// The virtual time [`Fx::until`] waits at most.
const LIMIT: Duration = Duration::from_secs(600);

/// The descriptor a driver finds its lock on (`Launch::hand_over`).
const HAND_OVER_FD: std::os::fd::RawFd = 3;

/// What [`Fx::hand_over`] returns: closes fd 3 when it drops, if fd 3 still is the file it handed over (the driver closes it
/// itself when it ends the way it should, and another file may have taken the number since).
pub struct HandOver {
    dev: u64,
    ino: u64,
}

impl HandOver {
    fn of_fd3() -> HandOver {
        // SAFETY: fd 3 is open (the caller just made it so).
        let file = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(HAND_OVER_FD) });
        let meta = file.metadata().expect("stat fd 3");
        HandOver {
            dev: meta.dev(),
            ino: meta.ino(),
        }
    }
}

impl Drop for HandOver {
    fn drop(&mut self) {
        // SAFETY: only reads the descriptor, which `ManuallyDrop` never closes by itself.
        let file = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(HAND_OVER_FD) });
        if file
            .metadata()
            .is_ok_and(|m| (m.dev(), m.ino()) == (self.dev, self.ino))
        {
            // SAFETY: closes the descriptor `hand_over` made.
            unsafe { nix::libc::close(HAND_OVER_FD) };
        }
    }
}

/// What a `Spawn::exec` of an [`Fx::exec_replaces_the_process`] unwinds the driver with: the process is gone, as after `exec(2)`.
pub struct Exec {
    /// What was exec'd: the program and its argv.
    pub launch: Launch,
    /// Whether fd 3 was close-on-exec at the call: false is what keeps the driver lock across a real `exec`. `None`: fd 3 was closed.
    pub fd3_cloexec: Option<bool>,
    /// fd 3 as `exec` keeps it: the same open file description (so the lock), a descriptor of its own. `None`: fd 3 was closed.
    kept: Option<OwnedFd>,
}

impl Exec {
    /// The lock file as the new program finds it, to be handed over again ([`Fx::hand_over`]) on fd 3.
    pub fn fd3(&mut self) -> File {
        File::from(self.kept.take().expect("fd 3 was open at the exec"))
    }
}

/// A `Spawn` over the `FakeSpawn` whose `exec` records the `Launch` there, keeps fd 3 and unwinds with an [`Exec`]: the code after
/// an `exec(2)` never runs, and nor does the rest of the driver that called it. [`Fx::run_launch_until_exec`] catches it.
struct Replacing {
    inner: Arc<FakeSpawn>,
}

impl Spawn for Replacing {
    fn spawn(&self, l: &Launch) -> io::Result<Pid> {
        self.inner.spawn(l)
    }
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait(p)
    }
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()> {
        self.inner.signal(p, s)
    }
    fn alive(&self, p: Pid) -> bool {
        self.inner.alive(p)
    }
    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        self.inner.exe_of(p)
    }
    fn cmdline(&self, p: Pid) -> Option<String> {
        self.inner.cmdline(p)
    }
    fn with_env(&self, key: &str, value: &str) -> Vec<Pid> {
        self.inner.with_env(key, value)
    }
    fn ppid(&self, p: Pid) -> Option<Pid> {
        self.inner.ppid(p)
    }
    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        self.inner.env_of(p, key)
    }
    fn descendants(&self, root: Pid) -> Vec<Pid> {
        self.inner.descendants(root)
    }
    fn exec(&self, l: &Launch) -> io::Error {
        // Records the launch; its answer, the failure of a fake, is not one here.
        let _ = self.inner.exec(l);
        // SAFETY: only probes the descriptor number.
        let fd3 = unsafe { BorrowedFd::borrow_raw(HAND_OVER_FD) };
        let flags = nix::fcntl::fcntl(fd3, nix::fcntl::FcntlArg::F_GETFD).ok();
        let cloexec = flags.map(|f| {
            nix::fcntl::FdFlag::from_bits_truncate(f).contains(nix::fcntl::FdFlag::FD_CLOEXEC)
        });
        let kept = flags.and_then(|_| dup(fd3).ok());
        // Not `panic!`: no message, no hook, and a driver that does not catch unwinding does not see a failure.
        std::panic::resume_unwind(Box::new(Exec {
            launch: l.clone(),
            fd3_cloexec: cloexec,
            kept,
        }))
    }
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)> {
        self.inner.stream(l)
    }
    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        self.inner.self_exe()
    }
}

/// The nap hook of [`Fx::run_launch_with`], with its lifetime erased: only [`NapClock`] calls it, on the thread that installed it,
/// while the call that borrowed it runs (the `Installed` guard takes it out before that call returns or unwinds).
struct Erased(*mut (dyn FnMut(usize) + 'static));

// SAFETY: it is called on the installing thread only (`NapSlot::owner`); the slot merely moves the pointer.
unsafe impl Send for Erased {}

#[derive(Default)]
struct NapSlot {
    hook: Option<Erased>,
    owner: Option<ThreadId>,
    /// The naps counted since the hook was installed.
    naps: usize,
    /// The virtual time when it was installed.
    since: Duration,
}

/// The `Clock` of the fake ports: the `ManualClock`, and the hook of [`Fx::run_launch_with`] called after each nap of the thread
/// that installed it. That is how a test changes the world between two polls of a loop it runs on its own thread (a Worker
/// that submits, a cancel that arrives) with no second thread, no wait and no race: the loop naps, the hook acts, the loop goes on.
struct NapClock {
    inner: ManualClock,
    slot: Mutex<NapSlot>,
}

impl NapClock {
    fn slot(&self) -> MutexGuard<'_, NapSlot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Clock for NapClock {
    fn now(&self) -> std::time::Instant {
        self.inner.now()
    }

    fn utc(&self) -> time::OffsetDateTime {
        self.inner.utc()
    }

    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        let taken = {
            let mut slot = self.slot();
            if slot.owner == Some(std::thread::current().id()) {
                assert!(
                    self.inner.elapsed() - slot.since < LIMIT,
                    "the loop napped for {LIMIT:?} of virtual time: the world it waits for never came"
                );
                slot.naps += 1;
                let n = slot.naps;
                slot.hook.take().map(|hook| (hook, n))
            } else {
                None
            }
        };
        if let Some((hook, n)) = taken {
            // Puts the hook back when it returns or unwinds; while it runs, a nap inside it (a command the hook ran) calls none.
            struct Back<'a>(&'a NapClock, Option<Erased>);
            impl Drop for Back<'_> {
                fn drop(&mut self) {
                    let mut slot = self.0.slot();
                    if slot.owner.is_some() {
                        slot.hook = self.1.take();
                    }
                }
            }
            let back = Back(self, Some(hook));
            let ptr = back.1.as_ref().map(|h| h.0).expect("just set");
            // SAFETY: `ptr` is the `&mut dyn FnMut` that `run_launch_with` borrowed for its call, which has not returned: its
            // `Installed` guard clears the slot first. Only this thread calls it, and the slot gave it up, so it is not called twice.
            unsafe { (*ptr)(n) };
            drop(back);
        }
    }

    fn kind(&self) -> &'static str {
        "NapClock"
    }
}

/// Fd 2 of the process pointing at a log file (appending) for a run, put back on drop: what the driver's `eprintln!` reaches when its
/// `Launch::stdout_to` is that file. (Under `cargo test`'s capture, `eprintln!` never reaches fd 2: nextest runs the tests with it off.)
struct StderrTo {
    path: PathBuf,
    saved: OwnedFd,
}

impl StderrTo {
    fn enter(path: &Path) -> StderrTo {
        std::fs::create_dir_all(path.parent().expect("a log lives in a directory"))
            .expect("create the log's directory");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open the log");
        let saved = dup(std::io::stderr()).expect("dup stderr");
        dup2_stderr(&file).expect("point stderr at the log");
        StderrTo {
            path: path.to_path_buf(),
            saved,
        }
    }

    /// Puts fd 2 back and appends the result the process printed on stdout, the last line of its log.
    fn finish(self, stdout: &str) {
        let path = self.path.clone();
        drop(self);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open the log");
        file.write_all(stdout.as_bytes()).expect("write the log");
    }
}

impl Drop for StderrTo {
    fn drop(&mut self) {
        let _ = dup2_stderr(&self.saved);
    }
}

/// Clears the nap hook when it drops.
struct Installed<'a>(&'a NapClock);

impl Drop for Installed<'_> {
    fn drop(&mut self) {
        *self.0.slot() = NapSlot::default();
    }
}

/// Held by every [`Fx`] from its creation to its drop.
static LOCK: Mutex<()> = Mutex::new(());

/// One pane of the fake server: what `list-panes -a` prints with the `@cpd_*` options.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneRow {
    pub pane: String,
    pub window: String,
    pub window_name: String,
    /// `@cpd_session`, empty when unset.
    pub session: String,
    /// `@cpd_view`, empty when unset.
    pub view: String,
    /// `@cpd_history`, empty when unset.
    pub history: String,
}

pub struct Fx {
    /// The env: the fixtures (`env.sql(..)`, `env.write_config(..)`, `env.git_repo(..)`) are its.
    pub env: TestEnv,
    /// The fake ports of [`Fx::with_ports`]: the `ManualClock`, the `FakeRunner` and the `FakeSpawn`, and the `Ports`
    /// over them. [`Fx::new`] builds them too, but its commands run on the real ports.
    pub fakes: Fakes,
    /// The clock of `fakes.ports`: `fakes.clock`'s time, and the hook of [`Fx::run_launch_with`].
    nap: Arc<NapClock>,
    /// The migration [`Fx::hold_store_migration`] simulates, while it runs; dropped (and the lock released) with the `Fx`.
    migration: Option<Arc<Migration>>,
    /// The clock of [`Fx::on_nap`], once installed.
    ask_nap: Option<Arc<AskNapClock>>,
    /// The `Universe` of the git route of `fakes.runner`: the repos, refs and config of every `git` call.
    pub universe: Arc<Mutex<Universe>>,
    /// The git route of `fakes.runner`.
    pub git_route: Arc<GitRoute>,
    /// The tmux route of `fakes.runner`: `tmux_route.fake()` is the `FakeTmux` (`set_screen`, `on_popup`, `add_client`...).
    pub tmux_route: Arc<TmuxRoute>,
    /// Whether `cpd` runs on `fakes.ports` ([`Fx::with_ports`]) or on the real ports ([`Fx::new`]).
    on_fake_ports: bool,
    allow_unscripted: Cell<bool>,
    /// The route of the `op` program, registered by the first [`Fx::op`].
    op_route: OnceCell<Arc<OpRoute>>,
    /// The process's environment and directory when this `Fx` took the lock.
    saved_vars: Vec<(OsString, OsString)>,
    saved_cwd: PathBuf,
    /// Last: released after `Drop` restored the process and `env` removed its directory.
    _lock: MutexGuard<'static, ()>,
}

impl Fx {
    /// An `Fx` over a [`TestEnv::bare`] env with a store (a copy of the template, as [`TestEnv::new`]): it starts no
    /// process. Waits for the other `Fx` of the process, if any.
    pub fn new() -> Fx {
        Fx::with(bare_with_store())
    }

    /// An `Fx` like [`Fx::new`] whose `cpd`, `cpd_in` and `cpd_with` call `cpd::run_with(&self.fakes.ports, ..)`: no
    /// process is started and nothing external is run. Its drop fails the test when a program was called that no
    /// `fake*` rule answered, unless [`Fx::allow_unscripted`] was called.
    pub fn with_ports() -> Fx {
        Fx::with_ports_over(bare_with_store())
    }

    /// [`Fx::with_ports`] whose `$CPD_HOME` is empty: the first command creates and migrates the store.
    pub fn with_ports_empty() -> Fx {
        Fx::with_ports_over(TestEnv::bare())
    }

    /// [`Fx::with_ports`] over `env`, a [`TestEnv::bare`] one (with a store) that the test seeded.
    pub fn with_ports_over(mut env: TestEnv) -> Fx {
        // The fake tmux has no socket: its directory is the env's own, never the shared `/tmp` a long root would give.
        env.tmux_in_root = true;
        let mut fx = Fx::with(env);
        fx.on_fake_ports = true;
        fx
    }

    /// Sets `key` in the process's environment until this `Fx` drops, which puts the environment back: for a test
    /// that reads what the product does with a variable it inherits (the hooks withhold `TMUX`, `PI_*`, `CPD_*`).
    pub fn export(&self, key: &str, value: &str) {
        // SAFETY: under this `Fx`'s lock, as `enter`.
        unsafe { std::env::set_var(key, value) };
    }

    /// Scripts `reply` for every call of `program` (matched by file name) on the `FakeRunner`.
    pub fn fake(&self, program: &str, reply: Reply) {
        let _ = (*self.fakes.runner).clone().on(program, |_| true, reply);
    }

    /// Scripts `reply` for the next call only of `program` on the `FakeRunner`: a later `fake_once` answers the one
    /// after it, and a call no rule is left for is unscripted (the test fails at drop unless [`Fx::allow_unscripted`]).
    pub fn fake_once(&self, program: &str, reply: Reply) {
        let _ = (*self.fakes.runner).clone().once(program, |_| true, reply);
    }

    /// [`Fx::fake`] for `gh`.
    pub fn fake_gh(&self, reply: Reply) {
        self.fake("gh", reply);
    }

    /// [`Fx::fake`] for `pi`.
    pub fn fake_pi(&self, reply: Reply) {
        self.fake("pi", reply);
    }

    /// [`Fx::fake`] for `op`.
    pub fn fake_op(&self, reply: Reply) {
        self.fake("op", reply);
    }

    /// The route of the `op` program on the `FakeRunner`, registered by the first call of this: it answers by
    /// subcommand (the word after `item`) from [`Fx::op_says`], exit 0 with nothing for a subcommand no one scripted,
    /// and keeps every call it answered, stdin included. A rule of [`Fx::fake_op`] answers before it, so a call that
    /// rule answers is not the route's.
    pub fn op(&self) -> &Arc<OpRoute> {
        self.op_route.get_or_init(|| {
            let route = Arc::new(OpRoute::default());
            let _ = (*self.fakes.runner).clone().route("op", route.clone());
            route
        })
    }

    /// The `item <sub>` of op answers with `stdout`, `stderr` and exit status `code`, until the next call of this for
    /// the same `sub` (`get`, `list`, `create`, `edit`, `delete`).
    pub fn op_says(&self, sub: &str, stdout: &str, stderr: &str, code: i32) {
        self.op().says(sub, Reply::exit(code, stdout, stderr));
    }

    /// The calls the route of `op` answered, one line of argv each, in order.
    pub fn op_log(&self) -> Vec<String> {
        self.op()
            .calls()
            .iter()
            .map(|call| call.args.join(" "))
            .collect()
    }

    /// What the route of `op` read on stdin in its last call of `item <sub>` that had one (`create` or `edit`).
    pub fn op_stdin(&self, sub: &str) -> String {
        let call = self
            .op()
            .calls()
            .into_iter()
            .rev()
            .find(|call| op_sub(call) == sub && call.stdin.is_some());
        let bytes = call
            .unwrap_or_else(|| panic!("op got no stdin for {sub}"))
            .stdin
            .expect("found by its stdin");
        String::from_utf8(bytes).expect("the stdin is UTF-8")
    }

    /// [`Fx::fake`] for `node`.
    pub fn fake_node(&self, reply: Reply) {
        self.fake("node", reply);
    }

    /// Every call of `program` (matched by file name) so far, in order, as the `FakeRunner` saw it.
    pub fn calls(&self, program: &str) -> Vec<Call> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|call| file_name(&call.program) == file_name(program))
            .collect()
    }

    /// Lifts the refusal, at drop, of a call no rule answered.
    pub fn allow_unscripted(&self) {
        self.allow_unscripted.set(true);
    }

    /// Moves the `ManualClock` forward by `d` (no nap is recorded).
    pub fn advance(&self, d: Duration) {
        self.fakes.clock.advance(d);
    }

    /// A second env with a migrated store, made as [`Fx::new`]'s own: starts no process. Its commands run on this `Fx`
    /// with its `$CPD_HOME`: `fx.cpd_with(env.root(), args, &[("CPD_HOME", ..)])`.
    pub fn another_env(&self) -> TestEnv {
        bare_with_store()
    }

    /// Runs `f` with the variables `set` added to the process's environment, put back after it, also when `f` panics:
    /// for code that reads the environment outside a `cpd` command (`TestEnv::bare` reads `TMPDIR`). Under this `Fx`'s lock.
    pub fn with_vars<T>(&self, set: &[(&str, &str)], f: impl FnOnce() -> T) -> T {
        let _scope = Scope::enter(
            &std::env::current_dir().expect("the current directory"),
            set,
            &[],
        );
        f()
    }

    /// Returns as soon as `cond` holds, advancing the `ManualClock` by [`TICK`] between two looks; panics
    /// `waited <limit> virtual time` when it never holds within [`LIMIT`].
    pub fn until(&self, mut cond: impl FnMut() -> bool) {
        let start = self.fakes.clock.elapsed();
        while !cond() {
            assert!(
                self.fakes.clock.elapsed() - start < LIMIT,
                "waited {LIMIT:?} virtual time"
            );
            self.advance(TICK);
        }
    }

    /// Sets the modification time of `path` to `by` before the virtual clock's now: a file that has been quiet for `by`
    /// by what the product reads (`ManualClock::utc`), not by the wall clock the file system stamped it with.
    pub fn backdate(&self, path: &Path, by: Duration) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open the file to backdate");
        file.set_modified(std::time::SystemTime::from(self.fakes.clock.utc()) - by)
            .expect("set the modification time");
    }

    /// The pid of a process the `FakeSpawn` runs: it is alive until [`Fx::ended_pid`] or an `exit` of the test.
    pub fn live_pid(&self) -> u32 {
        let launch = Launch {
            program: "cpd".into(),
            args: vec!["pr".into(), "merge".into()],
            ..Launch::default()
        };
        self.fakes
            .spawn
            .spawn(&launch)
            .expect("the FakeSpawn starts nothing and cannot fail")
            .0
    }

    /// The pid of a process the `FakeSpawn` ran and that has ended.
    pub fn ended_pid(&self) -> u32 {
        let pid = self.live_pid();
        self.fakes.spawn.exit(Pid(pid), 0);
        pid
    }

    /// Runs `f` with this `Fx`'s fake ports as the ambient `Ports::current()` of this thread: for a library call whose
    /// wrapper reads the ambient ports (`git::run`, `tamper`, `sync_lead`, the gate). A thread `f` spawns captures
    /// `fx.fakes.ports.clone()` and scopes it itself.
    pub fn scoped<R>(&self, f: impl FnOnce() -> R) -> R {
        Ports::scope(&self.fakes.ports, f)
    }

    /// Makes a process migrate the store, as `Store::open` of another `cpd` does (contract §1.4): `locks/store.lock` is
    /// held (a real `flock` on a file of the env, which conflicts with the product's own open of it in this process) with
    /// the marker of a live migrating pid (running in the `FakeSpawn`), and the store is held exclusively (a
    /// rollback-journal `BEGIN EXCLUSIVE`, as a migration's WAL switch does) so a follower's read-only schema check cannot
    /// land and it must take the setup lock, until `hold` of the `ManualClock`'s time has passed: the first `sleep` or
    /// `now` after that releases both, so a waiter's retries are what pass the time. Replaces the `Ports`' clock by one
    /// that does it; `fakes.clock` is still the `ManualClock` under it (`elapsed`, `naps`). It all drops with the `Fx`.
    /// Only on [`Fx::with_ports`]; once.
    pub fn hold_store_migration(&mut self, hold: Duration) {
        assert!(
            self.on_fake_ports && self.migration.is_none(),
            "hold_store_migration: on an Fx::with_ports, once"
        );
        let locks = self.env.cpd_home().join("locks");
        std::fs::create_dir_all(&locks).expect("create the locks directory");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(locks.join("store.lock"))
            .expect("open store.lock");
        let mut lock = Flock::lock(file, FlockArg::LockExclusiveNonblock)
            .map_err(|(_, e)| e)
            .expect("take store.lock");
        let pid = self
            .fakes
            .spawn
            .spawn(&Launch {
                program: "cpd".to_owned(),
                args: vec!["migrating-holder".to_owned()],
                ..Launch::default()
            })
            .expect("the FakeSpawn starts it");
        // `started_unix_ms` at the end of time: the waiter's `migrated_for` is `now - started`, saturating, so it is 0
        // and the ceiling (`MIGRATION_WAIT_CEILING`, tested in `store/lock.rs`) never fires while the test reads no wall clock.
        let marker = serde_json::json!({"migrating": true, "pid": pid.0, "started_unix_ms": u64::MAX, "from": 3, "to": 20, "command": "cpd migrating-holder"});
        lock.write_all(marker.to_string().as_bytes())
            .expect("write the marker");
        // The migrating holder also holds the database itself: a migration's WAL switch holds it exclusively, so a
        // follower's read-only schema check cannot land (contract §4.2) and it must take the setup lock and wait. A
        // rollback-journal `BEGIN EXCLUSIVE` is what blocks a read-only connection; a WAL one would not.
        let store = Connection::open(self.env.cpd_home().join("cpd.db")).expect("open cpd.db");
        store
            .pragma_update(None, "journal_mode", "DELETE")
            .expect("rollback journal");
        store
            .execute_batch("BEGIN EXCLUSIVE")
            .expect("hold the store");
        let migration = Arc::new(Migration {
            lock: Mutex::new(Some(lock)),
            store: Mutex::new(Some(store)),
            release_at: hold,
            clock: self.fakes.clock.clone(),
        });
        self.fakes.ports.clock = Arc::new(MigrationClock(migration.clone()));
        self.migration = Some(migration);
    }

    /// Runs `hook` at every `sleep` of the `Ports`' clock, on the thread that sleeps, in place of what another process
    /// does while a command waits (an `ask` waits for its `answer`): the `ManualClock` advances by the nap as usual, then
    /// the hook runs with a [`Nap`] that runs `cpd` on the ports as they were before the hook's clock (a `sleep` of that
    /// command runs no hook). Replaces the hook of an earlier call; it stays until replaced. Only on [`Fx::with_ports`].
    pub fn on_nap(&mut self, hook: impl FnMut(&Nap) + Send + 'static) {
        assert!(self.on_fake_ports, "on_nap: on an Fx::with_ports");
        let hook: Hook = Box::new(hook);
        match &self.ask_nap {
            Some(clock) => *clock.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook),
            None => {
                let nap = Nap {
                    ports: self.fakes.ports.clone(),
                };
                let clock = Arc::new(AskNapClock {
                    inner: self.fakes.ports.clock.clone(),
                    nap,
                    hook: Mutex::new(Some(hook)),
                });
                self.fakes.ports.clock = clock.clone();
                self.ask_nap = Some(clock);
            }
        }
    }
    /// The `i`th `Launch` the `FakeSpawn` recorded (from 0, in order).
    pub fn take_launch(&self, i: usize) -> Launch {
        let launches = self.fakes.spawn.launches();
        launches.get(i).cloned().unwrap_or_else(|| {
            panic!(
                "launch {i} was not recorded: the FakeSpawn has {}",
                launches.len()
            )
        })
    }

    /// Runs the `cpd` that `launch` describes through `run_with` on this thread: its `args` after the program, its
    /// `env_set` and `env_remove` and its `cwd` around the call, restored after it. Starts no process.
    pub fn run_launch(&self, launch: &Launch) -> Run {
        let cwd = match &launch.cwd {
            Some(cwd) => cwd.clone(),
            None => std::env::current_dir().expect("the current directory"),
        };
        let set: Vec<_> = launch
            .env_set
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let remove: Vec<_> = launch.env_remove.iter().map(String::as_str).collect();
        let args: Vec<_> = launch.args.iter().map(String::as_str).collect();
        // As the process would have it: what it writes to stderr (the driver's log lines) and then its result go to `stdout_to`.
        let log = launch.stdout_to.as_deref().map(StderrTo::enter);
        let run = self.run(&cwd, &args, &set, &remove);
        if let Some(log) = log {
            log.finish(&run.stdout);
        }
        run
    }

    /// [`Fx::run_launch_with`] for a program that may `exec`: `Err` is the [`Exec`] of a `Spawn::exec` the run reached (after
    /// [`Fx::exec_replaces_the_process`]), which ends the run where `exec(2)` ends a process; the env, directory and fd 2 are put back.
    // Err is the Exec payload of a caught panic, a small value passed through: the signature is the test API.
    #[allow(clippy::result_large_err)]
    pub fn run_launch_until_exec(
        &self,
        launch: &Launch,
        hook: impl FnMut(&Fx, usize),
    ) -> Result<Run, Exec> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_launch_with(launch, hook)
        })) {
            Ok(run) => Ok(run),
            Err(payload) => match payload.downcast::<Exec>() {
                Ok(exec) => Err(*exec),
                Err(other) => std::panic::resume_unwind(other),
            },
        }
    }

    /// From here a `Spawn::exec` of the product's ends the run that made it ([`Fx::run_launch_until_exec`]) instead of failing as a
    /// fake's does: the launch is recorded on the `FakeSpawn` as before. For an `Fx` of [`Fx::with_ports`].
    pub fn exec_replaces_the_process(&mut self) {
        assert!(
            self.on_fake_ports,
            "exec_replaces_the_process is for an Fx::with_ports"
        );
        self.fakes.ports.spawn = Arc::new(Replacing {
            inner: self.fakes.spawn.clone(),
        });
    }

    /// [`Fx::run_launch`] that calls `hook(self, n)` after the `n`th nap (from 1) the run takes on the fake clock, on this thread: a
    /// loop of the run (a driver's poll) naps, the hook changes the world (a Worker commits and submits, a cancel arrives), the loop goes
    /// on. The hook is not called by a nap inside itself (a command it runs). Panics when the run naps for [`LIMIT`] of virtual time
    /// without ending. Only for an `Fx` of [`Fx::with_ports`].
    pub fn run_launch_with(&self, launch: &Launch, mut hook: impl FnMut(&Fx, usize)) -> Run {
        assert!(
            self.on_fake_ports,
            "run_launch_with is for an Fx::with_ports"
        );
        let mut call = |n: usize| hook(self, n);
        let borrowed: &mut (dyn FnMut(usize) + '_) = &mut call;
        // SAFETY: only the lifetime is erased. `Installed` takes the pointer out of the clock before `call` (borrowed here) is dropped,
        // also when `run_launch` unwinds, and the clock calls it only on this thread during that time.
        let erased: *mut (dyn FnMut(usize) + 'static) = unsafe { std::mem::transmute(borrowed) };
        *self.nap.slot() = NapSlot {
            hook: Some(Erased(erased)),
            owner: Some(std::thread::current().id()),
            naps: 0,
            since: self.fakes.clock.elapsed(),
        };
        let _installed = Installed(&self.nap);
        self.run_launch(launch)
    }

    /// Makes `file` this process's fd 3 until the returned guard drops, as `Launch::hand_over` makes it a driver's: an
    /// `attempt run` of this `Fx` finds its lock there. The descriptor is not close-on-exec. Takes the `File` (its own
    /// descriptor is closed, or is fd 3 already, the lowest a process has free) and panics when another file is fd 3.
    /// The guard closes fd 3 (the driver closes it itself when it ends the way it should; a closed descriptor is not an
    /// error), so a test never leaves one behind.
    pub fn hand_over(&self, file: File) -> HandOver {
        if file.as_raw_fd() != HAND_OVER_FD {
            // SAFETY: only probes the descriptor number.
            let open = nix::fcntl::fcntl(
                unsafe { BorrowedFd::borrow_raw(HAND_OVER_FD) },
                nix::fcntl::FcntlArg::F_GETFD,
            );
            assert!(
                open.is_err(),
                "fd {HAND_OVER_FD} is open in the test process: nothing can be handed over on it"
            );
            // SAFETY: `file` is open and fd 3 is not: `dup2` makes a copy on it.
            let made = unsafe { nix::libc::dup2(file.as_raw_fd(), HAND_OVER_FD) };
            assert_eq!(
                made,
                HAND_OVER_FD,
                "dup2 onto fd {HAND_OVER_FD}: {}",
                std::io::Error::last_os_error()
            );
        } else {
            // Owned by the guard from here (it closes it); `File::open` made it close-on-exec.
            let _ = std::os::fd::IntoRawFd::into_raw_fd(file);
        }
        // SAFETY: fd 3 is open, whichever way it got here.
        let fd3 = unsafe { BorrowedFd::borrow_raw(HAND_OVER_FD) };
        nix::fcntl::fcntl(
            fd3,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        )
        .expect("clear close-on-exec on fd 3");
        HandOver::of_fd3()
    }

    /// An `Fx` over `env`. Waits for the other `Fx` of the process, if any.
    pub fn with(env: TestEnv) -> Fx {
        Fx::enter(env, LOCK.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// [`Fx::new`], or `None` when another `Fx` of the process lives.
    pub fn try_new() -> Option<Fx> {
        let lock = match LOCK.try_lock() {
            Ok(lock) => lock,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        Some(Fx::enter(bare_with_store(), lock))
    }

    fn enter(env: TestEnv, lock: MutexGuard<'static, ()>) -> Fx {
        // The path a driver reads as cpd's own executable (`Spawn::self_exe` of the real ports): this test binary. The first record wins.
        cpd_core::host::exe::record_start();
        // `isolation` reads `PATH` through `inherited_path`, cached at its first call: here, before any change.
        let (remove, set) = env.isolation();
        let saved_vars: Vec<_> = std::env::vars_os().collect();
        let saved_cwd = std::env::current_dir().expect("the current directory");
        let mut fakes = Fakes::new();
        let nap = Arc::new(NapClock {
            inner: (*fakes.clock).clone(),
            slot: Mutex::default(),
        });
        fakes.ports.clock = nap.clone();
        // The test kit's global git config (`TestEnv`'s `.gitconfig`), in the Universe: no file is read.
        let mut universe = Universe::new(fakes.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        universe.set_global("init.defaultBranch", "main");
        let universe = Arc::new(Mutex::new(universe));
        let git_route = Arc::new(GitRoute::new(universe.clone()));
        // Weak: the route lives in the runner's script, a strong handle back would be a cycle.
        let tmux_route = Arc::new(TmuxRoute::new().with_runner(fakes.runner.weak()));
        let _ = (*fakes.runner)
            .clone()
            .with_git(&git_route)
            .route("tmux", tmux_route.clone());
        let fx = Fx {
            env,
            fakes,
            nap,
            universe,
            git_route,
            tmux_route,
            on_fake_ports: false,
            allow_unscripted: Cell::new(false),
            migration: None,
            ask_nap: None,
            op_route: OnceCell::new(),
            saved_vars,
            saved_cwd,
            _lock: lock,
        };
        // From here `Drop` restores, also when a step below panics.
        // SAFETY: the lock serializes every `Fx`; nextest gives a test a process of its own, and a thread that does not
        // use `Fx` is not one that reads the environment while an `Fx` lives (docs/implementing.md, Tests).
        unsafe {
            for key in remove {
                std::env::remove_var(key);
            }
            for (key, value) in set {
                std::env::set_var(key, value);
            }
        }
        std::env::set_current_dir(fx.env.root()).expect("enter the env's root");
        fx
    }

    // ---- the git fixtures (the names of `TestEnv`'s), over the git route of the `FakeRunner`

    /// [`Fx::git`] that does not assert: the `Output` of a `git` that may fail (a merge that conflicts).
    pub fn git_try(&self, dir: &Path, args: &[&str]) -> Output {
        self.run_tool("git", Some(dir), &[], args)
    }

    /// `git` with `dir` as the cwd, through the route; panics unless it exits 0; returns trimmed stdout.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run_tool("git", Some(dir), &[], args);
        assert!(
            out.status.success(),
            "git {args:?} in {} exited {:?}: {}",
            dir.display(),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout)
            .expect("git's stdout is UTF-8")
            .trim_end()
            .to_owned()
    }

    /// `git` with `dir` as the cwd, through the route, whatever its exit code: for a command whose exit 1 is an answer
    /// (`config --get-regexp` with no match).
    pub fn git_output(&self, dir: &Path, args: &[&str]) -> Output {
        self.run_tool("git", Some(dir), &[], args)
    }

    /// A new repo `<root>/repos/<name>` of the `Universe`: branch `main`, one commit (`README.md`).
    pub fn git_repo(&self, name: &str) -> PathBuf {
        self.repo_of_universe(name);
        self.env.root().join("repos").join(name)
    }

    /// [`Fx::git_repo`] whose `origin` is `https://github.com/acme/<name>.git`, with `refs/remotes/origin/main`.
    pub fn keyed_repo(&self, name: &str) -> PathBuf {
        let repo = self.repo_of_universe(name);
        repo.keyed(name);
        // As `TestEnv::keyed_repo`: the repo `acme/<name>` is configured, unless a test wrote its policy already.
        let policy = self.env.cpd_home().join(format!("policy/acme/{name}.toml"));
        if !policy.exists() {
            self.env.repo_policy("acme", name, "");
        }
        self.env.root().join("repos").join(name)
    }

    /// `url.<bare>.insteadOf = <the repo's GitHub URL>`: the GitHub URL resolves to the bare repo `bare`.
    pub fn local_origin(&self, repo: &Path, bare: &Path) {
        let url = self.git(repo, &["config", "--get", "remote.origin.url"]);
        self.git(
            repo,
            &["config", &format!("url.{}.insteadOf", bare.display()), &url],
        );
    }

    /// Writes `path` (relative to `dir`) with `text` and commits it with `message`.
    pub fn commit_file(&self, dir: &Path, path: &str, text: &str, message: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a file has a parent"))
            .expect("create the file's directory");
        std::fs::write(&file, text).expect("write the file");
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "-q", "-m", message]);
    }

    /// The configured URL of `origin`, a GitHub one: adds `acme/<the repo's directory name>` when the repo has no
    /// `origin`, e.g. after [`Fx::drop_unreachable_origin`] (`TestEnv::github_origin`, over the git route).
    pub fn github_origin(&self, repo: &Path) -> String {
        if self.git(repo, &["remote"]).lines().any(|r| r == "origin") {
            return self.git(repo, &["config", "--get", "remote.origin.url"]);
        }
        let name = repo
            .file_name()
            .expect("a repo directory")
            .to_string_lossy()
            .into_owned();
        let url = format!("https://github.com/acme/{name}.git");
        self.git(repo, &["remote", "add", "origin", &url]);
        self.git(repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        if !self
            .env
            .cpd_home()
            .join(format!("policy/acme/{name}.toml"))
            .exists()
        {
            self.env.repo_policy("acme", &name, "");
        }
        url
    }

    /// Kills the fake server, after unsetting the supervising hooks (`TestEnv::kill_tmux_server`, over the route).
    pub fn kill_tmux_server(&self) {
        for hook in ["pane-died", "window-unlinked"] {
            self.tmux(&["set-hook", "-gu", hook]);
        }
        self.tmux(&["kill-server"]);
    }

    /// `<root>/repos/<name>` made in the `Universe` through its handle: `main`, `README.md`, one commit `initial`.
    fn repo_of_universe(&self, name: &str) -> Repo {
        let repo = Repo::init(&self.universe, &self.env.root().join("repos").join(name));
        repo.write("README.md", &format!("# {name}\n"));
        repo.commit("initial");
        repo
    }

    // ---- the tmux fixtures, over the tmux route of the `FakeRunner`

    /// `tmux -L <the env's private socket> <args>` through the route.
    pub fn tmux(&self, args: &[&str]) -> Output {
        let tmpdir = self.env.tmux_tmpdir().display().to_string();
        let envs = [("TMUX_TMPDIR".to_owned(), Some(tmpdir))];
        self.run_tool(
            "tmux",
            None,
            &envs,
            &[&["-L", self.env.tmux_socket()], args].concat(),
        )
    }

    /// The fake server's state of the env's socket; `None` while no server exists.
    pub fn tmux_state(&self) -> Option<cpd::fakes::tmux::State> {
        // Asks the route's fake directly, not through the runner: it is no call of the product's to record.
        let probe = Invocation {
            argv: ["-L", self.env.tmux_socket(), "list-sessions"]
                .map(str::to_owned)
                .to_vec(),
            tmux_tmpdir: self.env.tmux_tmpdir(),
            cwd: self.env.root().to_path_buf(),
            env: Vec::new(),
        };
        (self.tmux_route.fake().run(&probe).code == 0)
            .then(|| self.tmux_route.fake().state(self.env.tmux_socket()))
    }

    /// The fake server of the env's socket, for a step of a running view: a probe that owns its handles (an `Fx` is not
    /// `Clone`), so a closure can hold it. It asks the route's fake directly: no call of the product's to record.
    pub fn tmux_probe(&self) -> TmuxProbe {
        TmuxProbe {
            route: self.tmux_route.clone(),
            socket: self.env.tmux_socket().to_owned(),
            tmpdir: self.env.tmux_tmpdir(),
            cwd: self.env.root().to_path_buf(),
        }
    }

    /// Starts the cockpit in the fake server: session `cpd`, window `main` with three panes (`tree`, `plan`, `chat`),
    /// on a server with cpd's `SERVER_OPTIONS`. Only records: no pane program runs.
    pub fn cockpit(&self) {
        let ok = |args: &[&str]| {
            let out = self.tmux(args);
            assert!(
                out.status.success(),
                "tmux {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout)
                .expect("tmux's stdout is UTF-8")
                .trim()
                .to_owned()
        };
        let mut args: Vec<&str> = SERVER_OPTIONS
            .iter()
            .flat_map(|option| option.iter().map(String::as_str).chain([";"]))
            .collect();
        args.extend([
            "new-session",
            "-d",
            "-s",
            "cpd",
            "-n",
            "main",
            "-x",
            "215",
            "-y",
            "62",
            "-P",
            "-F",
            "#{pane_id}",
            "cpd tui view tree",
        ]);
        let tree = ok(&args);
        ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
        for view in ["plan", "chat"] {
            let pane = ok(&[
                "split-window",
                "-d",
                "-t",
                &tree,
                "-P",
                "-F",
                "#{pane_id}",
                &format!("cpd tui view {view}"),
            ]);
            ok(&["set", "-p", "-t", &pane, "@cpd_view", view]);
        }
    }

    /// Every pane of the fake server, in pane order, read from [`Fx::tmux_state`].
    pub fn panes(&self) -> Vec<PaneRow> {
        let Some(state) = self.tmux_state() else {
            return Vec::new();
        };
        let windows = state.windows();
        let option = |options: &[(String, String)], key: &str| {
            options
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        state
            .panes()
            .into_iter()
            .map(|pane| PaneRow {
                window_name: windows
                    .iter()
                    .find(|w| w.id == pane.window)
                    .map(|w| w.name.clone())
                    .unwrap_or_default(),
                session: option(&pane.options, "@cpd_session"),
                view: option(&pane.options, "@cpd_view"),
                history: option(&pane.options, "@cpd_history"),
                pane: pane.id,
                window: pane.window,
            })
            .collect()
    }

    /// Ends the program of `pane` with `status`, as a crash would: the fake marks the pane dead (`remain-on-exit`, as on
    /// cpd's server) and runs the `pane-died` hook, which leaves a Lead's pane dead and respawns a view's.
    pub fn pane_exit(&self, pane: &str, status: i32) {
        let out = self.tmux(&["__ctl", "pane-exit", pane, "--status", &status.to_string()]);
        assert!(
            out.status.success(),
            "tmux __ctl pane-exit {pane}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Whether the fake server has `pane` and its program has ended.
    pub fn pane_dead(&self, pane: &str) -> bool {
        let state = self.tmux_state().expect("a server");
        state
            .panes()
            .into_iter()
            .find(|p| p.id == pane)
            .unwrap_or_else(|| panic!("no pane {pane}"))
            .dead
    }

    /// One tick of the engine (`cpd watch`: E1-E5, E7-E14), run in this thread on the ports of this `Fx` through
    /// `cpd_engine::watch::run_with`: the loop ends at its first nap, which is on the `ManualClock`. Starts no process;
    /// what a step starts is recorded by the `FakeSpawn`, what it asks of tmux, `gh` or `cpd` is the `FakeRunner`'s.
    /// E5's in-process github poll is the fixture default ([`Fx::engine_gh_route`]): a test that needs another answer
    /// scripts `gh` first, which answers before the route.
    pub fn engine_tick(&self) {
        self.engine_gh_route();
        let until = Arc::new(AtomicBool::new(false));
        let clock = Arc::new(StopAtFirstNap {
            inner: self.fakes.clock.clone(),
            until: until.clone(),
        });
        let ports = Ports {
            clock,
            runner: self.fakes.ports.runner.clone(),
            spawn: self.fakes.ports.spawn.clone(),
            ..Ports::real()
        };
        cpd_engine::watch::run_with(&ports, &until)
            .unwrap_or_else(|e| panic!("the engine failed: {e:?}"));
    }

    /// A `cpd` reply for `tui view chat` and `tui view history` that exits at once.
    pub fn fake_cpd_views(&self) {
        let _ = (*self.fakes.runner)
            .clone()
            .on("cpd", |call| matches!(call.args.as_slice(), [tui, view, name, ..] if tui == "tui" && view == "view" && (name == "chat" || name == "history")), Reply::ok(""));
    }

    /// One call of `program` through the `FakeRunner`; panics when it could not be run.
    fn run_tool(
        &self,
        program: &str,
        cwd: Option<&Path>,
        envs: &[(String, Option<String>)],
        args: &[&str],
    ) -> Output {
        let call = Call {
            tool: program.to_owned(),
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            envs: envs.to_vec(),
            cwd: cwd.map(Path::to_path_buf),
            deadline: Duration::from_secs(60),
            stdin: None,
            stderr_to: None,
        };
        self.fakes
            .ports
            .runner
            .run(&call)
            .unwrap_or_else(|e| panic!("{program} {args:?} could not run: {e:?}"))
    }

    /// Runs `cpd <args>` from this env's root.
    pub fn cpd(&self, args: &[&str]) -> Run {
        self.cpd_in(self.env.root(), args)
    }

    /// Runs `cpd <args>` from this env's root with stdin and stdout pointing at `/dev/null` for the call, so the product's
    /// `on_a_terminal` is false whatever the test process's own stdin and stdout are (a run in a terminal): the refusal
    /// `not_a_terminal` of a command that owns its terminal. The two descriptors are put back after, also on a panic.
    pub fn cpd_without_a_terminal(&self, args: &[&str]) -> Run {
        let _detached = NoTerminal::enter();
        self.cpd(args)
    }

    /// Runs `f` with the process's stderr pointing at a file of the env's, and gives its result and what was written there:
    /// the lines the product prints with `eprintln!` (a `Run`'s `stderr` is empty). The descriptor is put back after, also
    /// on a panic. A harness that captures the output itself (the libtest default) takes `eprintln!` first and leaves the
    /// file empty: nextest, the Gate's runner, does not.
    pub fn with_stderr<T>(&self, f: impl FnOnce() -> T) -> (T, String) {
        let path = self.env.root().join("stderr-capture");
        let file = std::fs::File::create(&path).expect("create the stderr file");
        let result = {
            let _saved = Stderr::enter(&file);
            f()
        };
        (
            result,
            std::fs::read_to_string(&path).expect("read the stderr file"),
        )
    }

    /// Runs `cpd <args>` from `cwd`.
    pub fn cpd_in(&self, cwd: &Path, args: &[&str]) -> Run {
        self.cpd_with(cwd, args, &[])
    }

    /// Runs `cpd <args>` from `cwd` with the variables `vars` added to its environment.
    pub fn cpd_with(&self, cwd: &Path, args: &[&str], vars: &[(&str, &str)]) -> Run {
        self.run(cwd, args, vars, &[])
    }

    /// Runs `cpd <args>` from this env's root, for a command that writes nothing to the output (`Output::Owned`: the
    /// `tui action` commands, which print nothing whatever happens): panics unless it exits 0 with an empty output.
    pub fn cpd_silent(&self, args: &[&str]) {
        let (code, stdout) = self.run_raw(self.env.root(), args, &[], &[]);
        assert!(
            code == 0 && stdout.is_empty(),
            "cpd {args:?} exited {code}, wrote {stdout:?}: expected exit 0 and nothing"
        );
    }

    /// Runs `cpd <args>` from this env's root with `vars` added and `stdin` as the process's stdin for the call (a file
    /// of the env's, put back after, also on a panic): what a command that reads its stdin (`secret create`) sees.
    pub fn cpd_stdin(&self, args: &[&str], vars: &[(&str, &str)], stdin: &str) -> Run {
        let file = self.env.root().join("stdin");
        std::fs::write(&file, stdin).expect("write the stdin file");
        let _fed = Stdin::enter(&file);
        self.cpd_with(self.env.root(), args, vars)
    }

    /// Runs `cpd <args>` in `cwd` with `set` added to and `remove` taken out of its environment, on the ports of this
    /// `Fx`; the environment and the directory are put back after.
    fn run(&self, cwd: &Path, args: &[&str], set: &[(&str, &str)], remove: &[&str]) -> Run {
        let (exit, stdout) = self.run_raw(cwd, args, set, remove);
        finish(args, exit, stdout)
    }

    /// The exit code and the writer's text of one `cpd` call, as [`Fx::run`] sets it up.
    fn run_raw(
        &self,
        cwd: &Path,
        args: &[&str],
        set: &[(&str, &str)],
        remove: &[&str],
    ) -> (i32, String) {
        let _scope = Scope::enter(cwd, set, remove);
        let argv = std::iter::once("cpd")
            .chain(args.iter().copied())
            .map(OsString::from);
        let mut stdout = Vec::new();
        let code = if self.on_fake_ports {
            cpd::run_with(&self.fakes.ports, argv, &mut stdout)
        } else {
            cpd::main_with(argv, &mut stdout)
        };
        (
            exit_code(code),
            String::from_utf8(stdout).expect("stdout is UTF-8"),
        )
    }

    /// Runs `cpd <args>` from this env's root with the process's stderr pointing at a file for the call, and `Run::stderr`
    /// is what the product wrote there (a note it prints with `eprintln!`). The descriptor is put back after, also on a
    /// panic. It reads fd 2, so it needs a run without libtest's output capture, which nextest never has (a plain libtest
    /// run needs `--nocapture`).
    pub fn cpd_capturing_stderr(&self, args: &[&str]) -> Run {
        let capture = StderrCapture::enter();
        let mut run = self.cpd(args);
        run.stderr = capture.read();
        run
    }

    /// The `cpd` children the engine starts on this `Fx`'s `FakeSpawn`, which the test runs itself: [`Children`].
    pub fn children(&self) -> Children {
        Children {
            ports: self.fakes.ports.clone(),
            spawn: self.fakes.spawn.clone(),
            run: 0,
        }
    }

    /// Removes the remote `origin` from `repo` if it has been rewritten (has a `url.*.insteadof` config) or if it is unreachable.
    pub fn drop_unreachable_origin(&self, repo: &Path) {
        // Exit 1 and no output when nothing matches: no `Fx::git`, which panics on it.
        let rewritten = self.git_output(repo, &["config", "--get-regexp", r"^url\..*\.insteadof$"]);
        if rewritten.stdout.is_empty() && self.git(repo, &["remote"]).lines().any(|r| r == "origin")
        {
            self.git(repo, &["remote", "remove", "origin"]);
        }
    }

    /// Prepares a Plan for review: writes the outline of the Plan at `<env-root>/outline-<plan>.json` and records it with
    /// `cpd plan outline` (a Plan review refuses a Plan without one, hs-d502).
    pub fn outline_for_review(&self, plan: &str) {
        let shown = self.cpd(&["plan", "show", "--plan", plan]).ok();
        let mut done_when = serde_json::Map::new();
        for job in shown["jobs"].as_array().expect("plan show lists the jobs") {
            if job["status"] == "draft" {
                done_when.insert(
                    job["pr"].as_str().expect("a Job's Workflow").to_owned(),
                    serde_json::json!(["Done"]),
                );
            }
        }
        let outline = serde_json::json!({"goal": "Goal", "done_when": done_when});
        let path = self.env.root().join(format!("outline-{plan}.json"));
        std::fs::write(&path, outline.to_string()).expect("write the outline");
        self.cpd(&[
            "plan",
            "outline",
            "--file",
            &path.display().to_string(),
            "--plan",
            plan,
        ])
        .ok();
    }
}

/// `cpd <args>` in `cwd` with `set` added to and `remove` taken out of the environment, on `ports`, through `run_with`; the
/// environment and the directory are put back after.
fn run_in_process(
    ports: &Ports,
    cwd: &Path,
    args: &[&str],
    set: &[(&str, &str)],
    remove: &[&str],
) -> Run {
    let _scope = Scope::enter(cwd, set, remove);
    let argv = std::iter::once("cpd")
        .chain(args.iter().copied())
        .map(OsString::from);
    let mut stdout = Vec::new();
    let code = cpd::run_with(ports, argv, &mut stdout);
    finish(
        args,
        exit_code(code),
        String::from_utf8(stdout).expect("stdout is UTF-8"),
    )
}

/// The [`Run`] of `cpd <args>` that wrote `stdout` and ended with `exit`.
fn finish(args: &[&str], exit: i32, stdout: String) -> Run {
    // Every result but `Output::Owned` is one JSON line on the writer; a command that owns its stdout prints with
    // `print!` on the process's, which nobody can read back here.
    assert!(
        !stdout.is_empty(),
        "cpd {args:?} wrote nothing to the writer: it owns its stdout, which Fx cannot capture; run it with `env.cpd`"
    );
    Run {
        exit,
        stdout,
        stderr: String::new(),
    }
}

/// The first pid a `FakeSpawn` hands out (`FIRST_FAKE_PID` of core/src/host/spawn.rs): its nth start is `FIRST_PID + n`.
const FIRST_PID: u32 = 1000;

/// The detached `cpd` children an engine started on the `FakeSpawn` (E9's distill run), run by the test through `run_with`:
/// the fake only records a launch and starts nothing. Holds what it needs by value, so a `Send` closure of
/// [`Fx::watch_steps`] can own it and run the children as the engine naps.
pub struct Children {
    ports: Ports,
    spawn: Arc<FakeSpawn>,
    /// How many launches ran already.
    run: usize,
}

impl Children {
    /// Runs each launch recorded since the last call, in order, as [`Fx::run_launch`] does (its `env_set`, `env_remove` and
    /// `cwd` around the call, put back after), appends what the command wrote to the launch's `stdout_to` (the log the
    /// real child writes to) and marks its pid exited with the command's exit code: the engine's next sweep sees it end.
    /// Returns the launches run, with their exit codes. Every launch must be a `spawn` of the `FakeSpawn`, not an `exec`.
    pub fn run_pending(&mut self) -> Vec<(Launch, i32)> {
        let launches = self.spawn.launches();
        let total = launches.len();
        let mut done = Vec::new();
        for (index, launch) in launches.into_iter().enumerate().skip(self.run) {
            let pid = Pid(FIRST_PID + u32::try_from(index).expect("a few launches"));
            // An earlier `Children` (an engine run before a restart) already ran this launch and ended its pid.
            if self
                .spawn
                .try_wait(pid)
                .is_ok_and(|status| status.is_some())
            {
                continue;
            }
            assert!(
                self.spawn.alive(pid),
                "launch {index} ({}) is not a running spawn of pid {}",
                launch.program,
                pid.0
            );
            let cwd = launch
                .cwd
                .clone()
                .unwrap_or_else(|| std::env::current_dir().expect("the current directory"));
            let set: Vec<_> = launch
                .env_set
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect();
            let remove: Vec<_> = launch.env_remove.iter().map(String::as_str).collect();
            let args: Vec<_> = launch.args.iter().map(String::as_str).collect();
            let run = run_in_process(&self.ports, &cwd, &args, &set, &remove);
            let code = run.exit;
            if let Some(log) = &launch.stdout_to {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log)
                    .expect("open the child's log");
                file.write_all(run.stdout.as_bytes())
                    .expect("write the child's log");
            }
            self.spawn.exit(pid, code);
            done.push((launch, code));
        }
        self.run = total;
        done
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        // SAFETY: as in `enter`.
        unsafe {
            for (key, _) in std::env::vars_os() {
                if !self.saved_vars.iter().any(|(saved, _)| *saved == key) {
                    std::env::remove_var(key);
                }
            }
            for (key, value) in &self.saved_vars {
                std::env::set_var(key, value);
            }
        }
        // The directory may be gone when a test removed it; a panic in a drop during unwinding would abort.
        let _ = std::env::set_current_dir(&self.saved_cwd);
        // Last, and not while unwinding (a second panic aborts): the environment is already put back.
        if self.on_fake_ports && !std::thread::panicking() {
            // exit 1 is also a real answer to git: a call the route does not model must fail the test.
            self.git_route.assert_supported();
        }
        if self.on_fake_ports && !self.allow_unscripted.get() && !std::thread::panicking() {
            let unscripted = self.fakes.runner.unscripted();
            assert!(
                unscripted.is_empty(),
                "a call no fake answered (script it with Fx::fake*, or Fx::allow_unscripted): {}",
                unscripted
                    .iter()
                    .map(|call| format!("{} {:?}", call.program, call.args))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }
}

/// The clock of [`Fx::engine_tick`]: the `ManualClock`, whose first nap (the engine's wait between two ticks) ends the loop.
struct StopAtFirstNap {
    inner: Arc<ManualClock>,
    until: Arc<AtomicBool>,
}

impl Clock for StopAtFirstNap {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        self.until.store(true, Ordering::SeqCst);
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

/// The route of `op` ([`Fx::op`]): the reply of each subcommand, and the calls it answered.
#[derive(Default)]
pub struct OpRoute {
    replies: Mutex<Vec<(String, Reply)>>,
    calls: Mutex<Vec<Call>>,
}

impl OpRoute {
    /// `reply` is the answer of `item <sub>` from now on.
    pub fn says(&self, sub: &str, reply: Reply) {
        let mut replies = self.replies.lock().unwrap_or_else(PoisonError::into_inner);
        replies.retain(|(known, _)| known != sub);
        replies.push((sub.to_owned(), reply));
    }

    /// Every call it answered, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Handler for OpRoute {
    fn handle(&self, call: &Call) -> Reply {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call.clone());
        let replies = self.replies.lock().unwrap_or_else(PoisonError::into_inner);
        let sub = op_sub(call);
        replies
            .iter()
            .find(|(known, _)| known == sub)
            .map_or_else(|| Reply::ok(""), |(_, reply)| reply.clone())
    }
}

/// The word after `item` in `call`'s arguments (`get`, `list`, `create`, `edit`, `delete`); empty when there is none.
fn op_sub(call: &Call) -> &str {
    call.args
        .iter()
        .skip_while(|arg| *arg != "item")
        .nth(1)
        .map_or("", String::as_str)
}
/// The program's file name: `/usr/bin/gh` is `gh`.
fn file_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// A [`TestEnv::bare`] env with a store: a copy of the template.
pub fn bare_with_store() -> TestEnv {
    let env = TestEnv::bare();
    std::fs::copy(super::store_template(), env.cpd_home().join("cpd.db"))
        .expect("copy the store template");
    env
}

/// The number of `code`, which `ExitCode` does not give.
pub fn exit_code(code: ExitCode) -> i32 {
    (0..=u8::MAX)
        .find(|n| ExitCode::from(*n) == code)
        .map(i32::from)
        .expect("an ExitCode is a byte")
}

/// Stdin and stdout of the process pointing at `/dev/null`, put back on drop.
struct NoTerminal {
    stdin: OwnedFd,
    stdout: OwnedFd,
}

impl NoTerminal {
    fn enter() -> NoTerminal {
        let null = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
            .expect("open /dev/null");
        let stdin = dup(std::io::stdin()).expect("dup stdin");
        let stdout = dup(std::io::stdout()).expect("dup stdout");
        let saved = NoTerminal { stdin, stdout };
        dup2_stdin(&null).expect("point stdin at /dev/null");
        dup2_stdout(&null).expect("point stdout at /dev/null");
        saved
    }
}

impl Drop for NoTerminal {
    fn drop(&mut self) {
        let _ = dup2_stdin(&self.stdin);
        let _ = dup2_stdout(&self.stdout);
    }
}

/// The process's stderr pointing at a file, put back on drop.
struct Stderr {
    saved: OwnedFd,
}

impl Stderr {
    fn enter(file: &std::fs::File) -> Stderr {
        let saved = Stderr {
            saved: dup(std::io::stderr()).expect("dup stderr"),
        };
        dup2_stderr(file).expect("point stderr at the file");
        saved
    }
}

impl Drop for Stderr {
    fn drop(&mut self) {
        let _ = dup2_stderr(&self.saved);
    }
}

/// The process's stdin pointing at a file, put back on drop.
struct Stdin {
    saved: OwnedFd,
}

impl Stdin {
    fn enter(path: &Path) -> Stdin {
        let file = std::fs::File::open(path).expect("open the stdin file");
        let saved = Stdin {
            saved: dup(std::io::stdin()).expect("dup stdin"),
        };
        dup2_stdin(&file).expect("point stdin at the file");
        saved
    }
}

impl Drop for Stdin {
    fn drop(&mut self) {
        let _ = dup2_stdin(&self.saved);
    }
}

/// The extra variables and the directory of one call, restored when it ends or unwinds.
struct Scope {
    vars: Vec<(OsString, Option<OsString>)>,
    cwd: PathBuf,
}

impl Scope {
    fn enter(cwd: &Path, set: &[(&str, &str)], remove: &[&str]) -> Scope {
        let keys = set
            .iter()
            .map(|(key, _)| *key)
            .chain(remove.iter().copied());
        let scope = Scope {
            vars: keys
                .map(|key| (OsString::from(key), std::env::var_os(key)))
                .collect(),
            cwd: std::env::current_dir().expect("the current directory"),
        };
        // SAFETY: under the `Fx`'s lock (the caller holds `&Fx`).
        unsafe {
            for key in remove {
                std::env::remove_var(key);
            }
            for (key, value) in set {
                std::env::set_var(key, value);
            }
        }
        std::env::set_current_dir(cwd)
            .unwrap_or_else(|e| panic!("run cpd in {}: {e}", cwd.display()));
        scope
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        // SAFETY: as in `enter`.
        unsafe {
            for (key, previous) in self.vars.iter().rev() {
                match previous {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
        let _ = std::env::set_current_dir(&self.cwd);
    }
}

/// The fake tmux server of an `Fx`'s socket, asked directly ([`Fx::tmux_probe`]): it is `Clone`, so a step of a running
/// view holds one.
#[derive(Clone)]
pub struct TmuxProbe {
    route: Arc<TmuxRoute>,
    socket: String,
    tmpdir: PathBuf,
    cwd: PathBuf,
}

impl TmuxProbe {
    /// `tmux -L <socket> <args>`, which must succeed: its stdout without the trailing newline.
    pub fn run(&self, args: &[&str]) -> String {
        let argv = ["-L", self.socket.as_str()]
            .into_iter()
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect();
        let out = self.route.fake().run(&Invocation {
            argv,
            tmux_tmpdir: self.tmpdir.clone(),
            cwd: self.cwd.clone(),
            env: Vec::new(),
        });
        assert_eq!(out.code, 0, "tmux {args:?}: {}", out.stderr);
        out.stdout.trim_end_matches('\n').to_owned()
    }

    /// The server's state; the server must exist.
    pub fn state(&self) -> cpd::fakes::tmux::State {
        self.route.fake().state(&self.socket)
    }
}

/// `cpd <args>` in this process on `ports`, with the process's environment and directory as they are.
fn run_cpd(ports: &Ports, args: &[&str]) -> Run {
    let argv = std::iter::once("cpd")
        .chain(args.iter().copied())
        .map(OsString::from);
    let mut stdout = Vec::new();
    let code = cpd::run_with(ports, argv, &mut stdout);
    finish(
        args,
        exit_code(code),
        String::from_utf8(stdout).expect("stdout is UTF-8"),
    )
}

/// What a hook of [`Fx::on_nap`] runs `cpd` with: the ports of the `Fx` without the hook's clock.
pub struct Nap {
    ports: Ports,
}

impl Nap {
    /// `cpd <args>` in this process, in the environment and the directory of the command that sleeps.
    pub fn cpd(&self, args: &[&str]) -> Run {
        run_cpd(&self.ports, args)
    }
}

type Hook = Box<dyn FnMut(&Nap) + Send>;

/// The clock of [`Fx::on_nap`]: `inner`, and the hook after every `sleep`.
struct AskNapClock {
    inner: Arc<dyn Clock>,
    nap: Nap,
    hook: Mutex<Option<Hook>>,
}

impl Clock for AskNapClock {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        // Out of the mutex while it runs, and put back after (unless the call put another there), so the hook never
        // runs inside itself and a panic of it leaves none behind.
        let taken = self
            .hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut hook) = taken {
            hook(&self.nap);
            let mut slot = self.hook.lock().unwrap_or_else(PoisonError::into_inner);
            slot.get_or_insert(hook);
        }
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

/// The fixture of [`Fx::hold_store_migration`]: the held `flock`, the exclusively held store, and when they are released.
struct Migration {
    lock: Mutex<Option<Flock<File>>>,
    /// The `BEGIN EXCLUSIVE` write transaction that keeps a follower's read-only schema check from landing.
    store: Mutex<Option<Connection>>,
    release_at: Duration,
    clock: Arc<ManualClock>,
}

impl Migration {
    /// As the holder does at the end of its setup: releases the store first (a waiter on the `flock` proceeds as soon
    /// as it is free), then empties the file (the marker goes first) and releases the `flock`.
    fn release_when_due(&self) {
        if self.clock.elapsed() < self.release_at {
            return;
        }
        if let Some(store) = self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = store.execute_batch("ROLLBACK");
            drop(store);
        }
        if let Some(lock) = self
            .lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = lock.set_len(0);
            drop(lock);
        }
    }
}

/// The `ManualClock` of the `Fx`, as the `Ports`' clock while a migration is held: it ends the migration when its time is up.
struct MigrationClock(Arc<Migration>);

impl Clock for MigrationClock {
    fn now(&self) -> Instant {
        self.0.release_when_due();
        self.0.clock.now()
    }
    fn sleep(&self, d: Duration) {
        self.0.clock.sleep(d);
        self.0.release_when_due();
    }
    fn utc(&self) -> OffsetDateTime {
        self.0.clock.utc()
    }
}

/// Stderr of the process pointing at a temporary file, put back on drop.
struct StderrCapture {
    saved: OwnedFd,
    file: File,
}

impl StderrCapture {
    fn enter() -> StderrCapture {
        let file = tempfile::tempfile().expect("a temporary file for stderr");
        let saved = dup(std::io::stderr()).expect("dup stderr");
        dup2_stderr(&file).expect("point stderr at the file");
        StderrCapture { saved, file }
    }

    /// What was written to stderr since `enter`.
    fn read(&self) -> String {
        let mut file = &self.file;
        file.flush().expect("flush");
        file.seek(SeekFrom::Start(0)).expect("rewind");
        let mut text = String::new();
        file.read_to_string(&mut text).expect("stderr is UTF-8");
        text
    }
}

impl Drop for StderrCapture {
    fn drop(&mut self) {
        let _ = dup2_stderr(&self.saved);
    }
}
