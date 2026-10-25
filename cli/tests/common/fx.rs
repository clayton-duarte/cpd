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
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use nix::unistd::{dup, dup2_stdin, dup2_stdout};

use cpd_core::clock::{Clock, ManualClock};
use cpd_core::fakes::git::{GitRoute, Repo, Universe};
use cpd_core::fakes::tmux::{Invocation, TmuxRoute};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::host::spawn::Launch;
use cpd_core::host::tmux::SERVER_OPTIONS;
use cpd_core::ports::{Fakes, Ports};
use time::OffsetDateTime;

use super::{Run, TestEnv};

/// What [`Fx::until`] advances the `ManualClock` by between two looks (the engine tests' tick, `TICK_SECONDS`).
const TICK: Duration = Duration::from_millis(100);

/// The virtual time [`Fx::until`] waits at most.
const LIMIT: Duration = Duration::from_secs(600);

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
        self.op().calls().iter().map(|call| call.args.join(" ")).collect()
    }

    /// What the route of `op` read on stdin in its last call of `item <sub>` that had one (`create` or `edit`).
    pub fn op_stdin(&self, sub: &str) -> String {
        let call = self.op().calls().into_iter().rev().find(|call| op_sub(call) == sub && call.stdin.is_some());
        let bytes = call.unwrap_or_else(|| panic!("op got no stdin for {sub}")).stdin.expect("found by its stdin");
        String::from_utf8(bytes).expect("the stdin is UTF-8")
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
        let fakes = Fakes::new();
        // The test kit's global git config (`TestEnv`'s `.gitconfig`), in the Universe: no file is read.
        let mut universe = Universe::new(fakes.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        universe.set_global("init.defaultBranch", "main");
        let universe = Arc::new(Mutex::new(universe));
        let git_route = Arc::new(GitRoute::new(universe.clone()));
        // Weak: the route lives in the runner's script, a strong handle back would be a cycle.
        let tmux_route = Arc::new(TmuxRoute::new().with_runner(fakes.runner.weak()));
        let _ = (*fakes.runner).clone().with_git(&git_route).route("tmux", tmux_route.clone());
        let fx = Fx { env, fakes, universe, git_route, tmux_route, on_fake_ports: false, allow_unscripted: Cell::new(false), op_route: OnceCell::new(), saved_vars, saved_cwd, _lock: lock };
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
        String::from_utf8(out.stdout).expect("git's stdout is UTF-8").trim_end().to_owned()
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
        let policy = self.env.cpd_home().join(format!("policy/repos/acme/{name}.toml"));
        if !policy.exists() {
            self.env.repo_policy("acme", name, "");
        }
        self.env.root().join("repos").join(name)
    }

    /// `url.<bare>.insteadOf = <the repo's GitHub URL>`: the GitHub URL resolves to the bare repo `bare`.
    pub fn local_origin(&self, repo: &Path, bare: &Path) {
        let url = self.git(repo, &["config", "--get", "remote.origin.url"]);
        self.git(repo, &["config", &format!("url.{}.insteadOf", bare.display()), &url]);
    }

    /// Writes `path` (relative to `dir`) with `text` and commits it with `message`.
    pub fn commit_file(&self, dir: &Path, path: &str, text: &str, message: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a file has a parent")).expect("create the file's directory");
        std::fs::write(&file, text).expect("write the file");
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "-q", "-m", message]);
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
        self.run_tool("tmux", None, &envs, &[&["-L", self.env.tmux_socket()], args].concat())
    }

    /// The fake server's state of the env's socket; `None` while no server exists.
    pub fn tmux_state(&self) -> Option<cpd::fakes::tmux::State> {
        // Asks the route's fake directly, not through the runner: it is no call of the product's to record.
        let probe = Invocation {
            argv: ["-L", self.env.tmux_socket(), "list-sessions"].map(str::to_owned).to_vec(),
            tmux_tmpdir: self.env.tmux_tmpdir(),
            cwd: self.env.root().to_path_buf(),
            env: Vec::new(),
        };
        (self.tmux_route.fake().run(&probe).code == 0).then(|| self.tmux_route.fake().state(self.env.tmux_socket()))
    }

    /// Starts the cockpit in the fake server: session `cpd`, window `main` with three panes (`tree`, `plan`, `chat`),
    /// on a server with cpd's `SERVER_OPTIONS`. Only records: no pane program runs.
    pub fn cockpit(&self) {
        let ok = |args: &[&str]| {
            let out = self.tmux(args);
            assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).expect("tmux's stdout is UTF-8").trim().to_owned()
        };
        let mut args: Vec<&str> = SERVER_OPTIONS.iter().flat_map(|option| option.iter().map(String::as_str).chain([";"])).collect();
        args.extend(["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "215", "-y", "62", "-P", "-F", "#{pane_id}", "cpd tui view tree"]);
        let tree = ok(&args);
        ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
        for view in ["plan", "chat"] {
            let pane = ok(&["split-window", "-d", "-t", &tree, "-P", "-F", "#{pane_id}", &format!("cpd tui view {view}")]);
            ok(&["set", "-p", "-t", &pane, "@cpd_view", view]);
        }
    }

    /// Every pane of the fake server, in pane order, read from [`Fx::tmux_state`].
    pub fn panes(&self) -> Vec<PaneRow> {
        let Some(state) = self.tmux_state() else { return Vec::new() };
        let windows = state.windows();
        let option = |options: &[(String, String)], key: &str| options.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default();
        state
            .panes()
            .into_iter()
            .map(|pane| PaneRow {
                window_name: windows.iter().find(|w| w.id == pane.window).map(|w| w.name.clone()).unwrap_or_default(),
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
        assert!(out.status.success(), "tmux __ctl pane-exit {pane}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Whether the fake server has `pane` and its program has ended.
    pub fn pane_dead(&self, pane: &str) -> bool {
        let state = self.tmux_state().expect("a server");
        state.panes().into_iter().find(|p| p.id == pane).unwrap_or_else(|| panic!("no pane {pane}")).dead
    }

    /// One tick of the engine (`cpd watch`: E1-E5, E7-E14), run in this thread on the ports of this `Fx` through
    /// `cpd_engine::watch::run_with`: the loop ends at its first nap, which is on the `ManualClock`. Starts no process;
    /// what a step starts is recorded by the `FakeSpawn`, what it asks of tmux, `gh` or `cpd` is the `FakeRunner`'s.
    pub fn engine_tick(&self) {
        let until = Arc::new(AtomicBool::new(false));
        let clock = Arc::new(StopAtFirstNap { inner: self.fakes.clock.clone(), until: until.clone() });
        let ports = Ports { clock, runner: self.fakes.ports.runner.clone(), spawn: self.fakes.ports.spawn.clone() };
        cpd_engine::watch::run_with(&ports, &until).unwrap_or_else(|e| panic!("the engine failed: {e:?}"));
    }

    /// A `cpd` reply for `tui view chat` and `tui view history` that exits at once.
    pub fn fake_cpd_views(&self) {
        let _ = (*self.fakes.runner)
            .clone()
            .on("cpd", |call| matches!(call.args.as_slice(), [tui, view, name, ..] if tui == "tui" && view == "view" && (name == "chat" || name == "history")), Reply::ok(""));
    }

    /// One call of `program` through the `FakeRunner`; panics when it could not be run.
    fn run_tool(&self, program: &str, cwd: Option<&Path>, envs: &[(String, Option<String>)], args: &[&str]) -> Output {
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
        self.fakes.ports.runner.run(&call).unwrap_or_else(|e| panic!("{program} {args:?} could not run: {e:?}"))
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
        if self.on_fake_ports && !std::thread::panicking() {
            // exit 1 is also a real answer to git: a call the route does not model must fail the test.
            self.git_route.assert_supported();
        }
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
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Handler for OpRoute {
    fn handle(&self, call: &Call) -> Reply {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push(call.clone());
        let replies = self.replies.lock().unwrap_or_else(PoisonError::into_inner);
        let sub = op_sub(call);
        replies.iter().find(|(known, _)| known == sub).map_or_else(|| Reply::ok(""), |(_, reply)| reply.clone())
    }
}

/// The word after `item` in `call`'s arguments (`get`, `list`, `create`, `edit`, `delete`); empty when there is none.
fn op_sub(call: &Call) -> &str {
    call.args.iter().skip_while(|arg| *arg != "item").nth(1).map_or("", String::as_str)
}
/// The program's file name: `/usr/bin/gh` is `gh`.
fn file_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// A [`TestEnv::bare`] env with a store: a copy of the template.
pub fn bare_with_store() -> TestEnv {
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

/// The process's stdin pointing at a file, put back on drop.
struct Stdin {
    saved: OwnedFd,
}

impl Stdin {
    fn enter(path: &Path) -> Stdin {
        let file = std::fs::File::open(path).expect("open the stdin file");
        let saved = Stdin { saved: dup(std::io::stdin()).expect("dup stdin") };
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
