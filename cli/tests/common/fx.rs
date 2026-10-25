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

use std::cell::Cell;
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Duration;

use nix::unistd::{dup, dup2_stdin, dup2_stdout};

use cpd_core::host::runner::{Call, Reply};
use cpd_core::host::spawn::Launch;
use cpd_core::ports::Fakes;

use super::{Run, TestEnv};

/// What [`Fx::until`] advances the `ManualClock` by between two looks (the engine tests' tick, `TICK_SECONDS`).
const TICK: Duration = Duration::from_millis(100);

/// The virtual time [`Fx::until`] waits at most.
const LIMIT: Duration = Duration::from_secs(600);

/// Held by every [`Fx`] from its creation to its drop.
static LOCK: Mutex<()> = Mutex::new(());

pub struct Fx {
    /// The env: the fixtures (`env.sql(..)`, `env.write_config(..)`, `env.git_repo(..)`) are its.
    pub env: TestEnv,
    /// The fake ports of [`Fx::with_ports`]: the `ManualClock`, the `FakeRunner` and the `FakeSpawn`, and the `Ports`
    /// over them. [`Fx::new`] builds them too, but its commands run on the real ports.
    pub fakes: Fakes,
    /// Whether `cpd` runs on `fakes.ports` ([`Fx::with_ports`]) or on the real ports ([`Fx::new`]).
    on_fake_ports: bool,
    allow_unscripted: Cell<bool>,
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
        let mut fx = Fx::new();
        fx.on_fake_ports = true;
        fx
    }

    /// Scripts `reply` for every call of `program` (matched by file name) on the `FakeRunner`.
    pub fn fake(&self, program: &str, reply: Reply) {
        let _ = (*self.fakes.runner).clone().on(program, |_| true, reply);
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

    /// [`Fx::fake`] for `node`.
    pub fn fake_node(&self, reply: Reply) {
        self.fake("node", reply);
    }

    /// Every call of `program` (matched by file name) so far, in order, as the `FakeRunner` saw it.
    pub fn calls(&self, program: &str) -> Vec<Call> {
        self.fakes.runner.calls().into_iter().filter(|call| file_name(&call.program) == file_name(program)).collect()
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
        let _scope = Scope::enter(&std::env::current_dir().expect("the current directory"), set, &[]);
        f()
    }

    /// Returns as soon as `cond` holds, advancing the `ManualClock` by [`TICK`] between two looks; panics
    /// `waited <limit> virtual time` when it never holds within [`LIMIT`].
    pub fn until(&self, mut cond: impl FnMut() -> bool) {
        let start = self.fakes.clock.elapsed();
        while !cond() {
            assert!(self.fakes.clock.elapsed() - start < LIMIT, "waited {LIMIT:?} virtual time");
            self.advance(TICK);
        }
    }

    /// The `i`th `Launch` the `FakeSpawn` recorded (from 0, in order).
    pub fn take_launch(&self, i: usize) -> Launch {
        let launches = self.fakes.spawn.launches();
        launches.get(i).cloned().unwrap_or_else(|| panic!("launch {i} was not recorded: the FakeSpawn has {}", launches.len()))
    }

    /// Runs the `cpd` that `launch` describes through `run_with` on this thread: its `args` after the program, its
    /// `env_set` and `env_remove` and its `cwd` around the call, restored after it. Starts no process.
    pub fn run_launch(&self, launch: &Launch) -> Run {
        let cwd = match &launch.cwd {
            Some(cwd) => cwd.clone(),
            None => std::env::current_dir().expect("the current directory"),
        };
        let set: Vec<_> = launch.env_set.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
        let remove: Vec<_> = launch.env_remove.iter().map(String::as_str).collect();
        let args: Vec<_> = launch.args.iter().map(String::as_str).collect();
        self.run(&cwd, &args, &set, &remove)
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
        // `isolation` reads `PATH` through `inherited_path`, cached at its first call: here, before any change.
        let (remove, set) = env.isolation();
        let saved_vars: Vec<_> = std::env::vars_os().collect();
        let saved_cwd = std::env::current_dir().expect("the current directory");
        let fx = Fx { env, fakes: Fakes::new(), on_fake_ports: false, allow_unscripted: Cell::new(false), saved_vars, saved_cwd, _lock: lock };
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

    /// Runs `cpd <args>` from `cwd`.
    pub fn cpd_in(&self, cwd: &Path, args: &[&str]) -> Run {
        self.cpd_with(cwd, args, &[])
    }

    /// Runs `cpd <args>` from `cwd` with the variables `vars` added to its environment.
    pub fn cpd_with(&self, cwd: &Path, args: &[&str], vars: &[(&str, &str)]) -> Run {
        self.run(cwd, args, vars, &[])
    }

    /// Runs `cpd <args>` in `cwd` with `set` added to and `remove` taken out of its environment, on the ports of this
    /// `Fx`; the environment and the directory are put back after.
    fn run(&self, cwd: &Path, args: &[&str], set: &[(&str, &str)], remove: &[&str]) -> Run {
        let _scope = Scope::enter(cwd, set, remove);
        let argv = std::iter::once("cpd").chain(args.iter().copied()).map(OsString::from);
        let mut stdout = Vec::new();
        let code = if self.on_fake_ports {
            cpd::run_with(&self.fakes.ports, argv, &mut stdout)
        } else {
            cpd::main_with(argv, &mut stdout)
        };
        // Every result but `Output::Owned` is one JSON line on the writer; a command that owns its stdout prints with
        // `print!` on the process's, which nobody can read back here.
        assert!(!stdout.is_empty(), "cpd {args:?} wrote nothing to the writer: it owns its stdout, which Fx cannot capture; run it with `env.cpd`");
        Run { exit: exit_code(code), stdout: String::from_utf8(stdout).expect("stdout is UTF-8"), stderr: String::new() }
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
        if self.on_fake_ports && !self.allow_unscripted.get() && !std::thread::panicking() {
            let unscripted = self.fakes.runner.unscripted();
            assert!(
                unscripted.is_empty(),
                "a call no fake answered (script it with Fx::fake*, or Fx::allow_unscripted): {}",
                unscripted.iter().map(|call| format!("{} {:?}", call.program, call.args)).collect::<Vec<_>>().join("; ")
            );
        }
    }
}

/// The program's file name: `/usr/bin/gh` is `gh`.
fn file_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn bare_with_store() -> TestEnv {
    let env = TestEnv::bare();
    std::fs::copy(super::store_template(), env.cpd_home().join("cpd.db")).expect("copy the store template");
    env
}

/// The number of `code`, which `ExitCode` does not give.
fn exit_code(code: ExitCode) -> i32 {
    (0..=u8::MAX).find(|n| ExitCode::from(*n) == code).map(i32::from).expect("an ExitCode is a byte")
}

/// Stdin and stdout of the process pointing at `/dev/null`, put back on drop.
struct NoTerminal {
    stdin: OwnedFd,
    stdout: OwnedFd,
}

impl NoTerminal {
    fn enter() -> NoTerminal {
        let null = std::fs::OpenOptions::new().read(true).write(true).open("/dev/null").expect("open /dev/null");
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

/// The extra variables and the directory of one call, restored when it ends or unwinds.
struct Scope {
    vars: Vec<(OsString, Option<OsString>)>,
    cwd: PathBuf,
}

impl Scope {
    fn enter(cwd: &Path, set: &[(&str, &str)], remove: &[&str]) -> Scope {
        let keys = set.iter().map(|(key, _)| *key).chain(remove.iter().copied());
        let scope = Scope {
            vars: keys.map(|key| (OsString::from(key), std::env::var_os(key))).collect(),
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
        std::env::set_current_dir(cwd).unwrap_or_else(|e| panic!("run cpd in {}: {e}", cwd.display()));
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
