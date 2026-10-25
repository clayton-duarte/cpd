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

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Duration;

use cpd_core::host::runner::{Call, Reply};
use cpd_core::host::spawn::Launch;
use cpd_core::ports::Fakes;

use super::{Run, TestEnv};

/// Held by every [`Fx`] from its creation to its drop.
static LOCK: Mutex<()> = Mutex::new(());

pub struct Fx {
    /// The env: the fixtures (`env.sql(..)`, `env.write_config(..)`, `env.git_repo(..)`) are its.
    pub env: TestEnv,
    /// The fake ports of [`Fx::with_ports`]: the `ManualClock`, the `FakeRunner` and the `FakeSpawn`, and the `Ports`
    /// over them. [`Fx::new`] builds them too, but its commands run on the real ports.
    pub fakes: Fakes,
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

    /// STUB (PR testkit2): an `Fx` like [`Fx::new`] whose `cpd`, `cpd_in` and `cpd_with` call
    /// `cpd::run_with(&self.fakes.ports, ..)`: no process is started and nothing external is run. Its drop fails the test
    /// when a program was called that no `fake*` rule answered, unless [`Fx::allow_unscripted`] was called.
    pub fn with_ports() -> Fx {
        todo!("Fx::with_ports")
    }

    /// STUB: scripts `reply` for every call of `program` (matched by file name) on the `FakeRunner`.
    pub fn fake(&self, _program: &str, _reply: Reply) {
        todo!("Fx::fake")
    }

    /// STUB: [`Fx::fake`] for `gh`.
    pub fn fake_gh(&self, _reply: Reply) {
        todo!("Fx::fake_gh")
    }

    /// STUB: [`Fx::fake`] for `pi`.
    pub fn fake_pi(&self, _reply: Reply) {
        todo!("Fx::fake_pi")
    }

    /// STUB: [`Fx::fake`] for `op`.
    pub fn fake_op(&self, _reply: Reply) {
        todo!("Fx::fake_op")
    }

    /// STUB: [`Fx::fake`] for `node`.
    pub fn fake_node(&self, _reply: Reply) {
        todo!("Fx::fake_node")
    }

    /// STUB: every call of `program` (matched by file name) so far, in order, as the `FakeRunner` saw it.
    pub fn calls(&self, _program: &str) -> Vec<Call> {
        todo!("Fx::calls")
    }

    /// STUB: lifts the refusal, at drop, of a call no rule answered.
    pub fn allow_unscripted(&self) {
        todo!("Fx::allow_unscripted")
    }

    /// STUB: moves the `ManualClock` forward by `d` (no nap is recorded).
    pub fn advance(&self, _d: Duration) {
        todo!("Fx::advance")
    }

    /// STUB: returns as soon as `cond` holds, advancing the `ManualClock` by one tick between two looks; panics
    /// `waited <limit> virtual time` when it never holds within the limit.
    pub fn until(&self, _cond: impl FnMut() -> bool) {
        todo!("Fx::until")
    }

    /// STUB: the `i`th `Launch` the `FakeSpawn` recorded (from 0, in order).
    pub fn take_launch(&self, _i: usize) -> Launch {
        todo!("Fx::take_launch")
    }

    /// STUB: runs the `cpd` that `launch` describes through `run_with` on this thread: its `args` after the program,
    /// its `env_set` and `env_remove` and its `cwd` around the call, restored after it. Starts no process.
    pub fn run_launch(&self, _launch: &Launch) -> Run {
        todo!("Fx::run_launch")
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
        let fx = Fx { env, fakes: Fakes::new(), saved_vars, saved_cwd, _lock: lock };
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

    /// Runs `cpd <args>` from `cwd`.
    pub fn cpd_in(&self, cwd: &Path, args: &[&str]) -> Run {
        self.cpd_with(cwd, args, &[])
    }

    /// Runs `cpd <args>` from `cwd` with the variables `vars` added to its environment.
    pub fn cpd_with(&self, cwd: &Path, args: &[&str], vars: &[(&str, &str)]) -> Run {
        let _scope = Scope::enter(cwd, vars);
        let argv = std::iter::once("cpd").chain(args.iter().copied()).map(OsString::from);
        let mut stdout = Vec::new();
        let code = cpd::main_with(argv, &mut stdout);
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
    }
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

/// The extra variables and the directory of one call, restored when it ends or unwinds.
struct Scope {
    vars: Vec<(OsString, Option<OsString>)>,
    cwd: PathBuf,
}

impl Scope {
    fn enter(cwd: &Path, vars: &[(&str, &str)]) -> Scope {
        let scope = Scope {
            vars: vars.iter().map(|(key, _)| (OsString::from(key), std::env::var_os(key))).collect(),
            cwd: std::env::current_dir().expect("the current directory"),
        };
        // SAFETY: under the `Fx`'s lock (the caller holds `&Fx`).
        unsafe {
            for (key, value) in vars {
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
