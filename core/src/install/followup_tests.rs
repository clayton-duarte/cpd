//! `cpd install`'s follow-ups on the new binary, the wait for the engine lock, the engine order, the restart marker
//! and the tag (docs/implementing.md "`cpd install`" steps 7-8 and "The install's engine order"; contract §3.7),
//! in-process on `Fakes` (ADR 0013): the checkout is a clone through the git route, the built and the installed
//! `cpd` are a rule and a route of the runner, `tmux` is a route with a window table, `HOME` and `CPD_HOME` are
//! directories of a temp dir, and time is the `ManualClock` (a wait is virtual). The engine lock is a real `flock`
//! on a file of the temp dir (`crate::lock`), held by a `File` of the test: no process, no real git, no sleep.
//!
//! Each test is the test of the same name of `cli/tests/tooling/install_script.rs` (group C), which ran
//! `scripts/install-cpd.sh` with fake binaries on `PATH`, a real tmux server and `flock(1)` holders. Changes of
//! subject: the old engine is a `File` holding the lock until the window is killed (or until virtual time passes);
//! the bound of the wait is `Install::lock_wait` (the script's `CPD_INSTALL_LOCK_WAIT_SECS`, read at the edge).

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use time::OffsetDateTime;

use super::{DEFAULT_LOCK_WAIT, Install, run};
use crate::clock::{Clock, ManualClock};
use crate::fakes::git::{GitRoute, Repo, Universe};
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::host::spawn::{FakeSpawn, Pid, Signal, Spawn as _};
use crate::ports::{Fakes, Ports};

const MIGRATIONS: &str = "core/src/store/migrations";

const CONFIG: &str = r#"{"ok":true,"config":{"tmux":{"socket":"s","cockpit_session":"cock","work_session":"work"}}}"#;
const CHECK_OK: &str = r#"{"ok":true,"checked":["config","store","tmux","listing","home"],"tmux":{"version":"3.8","server":"running"}}"#;
const CHECK_FAILS: &str = r#"{"ok":false,"error":{"code":"tmux_failed","message":"tmux list-panes printed unexpected output."}}"#;
const RESPAWN_OK: &str = r#"{"ok":true,"respawned":[]}"#;
const MARK_OK: &str = r#"{"ok":true,"marked":true,"engine_pid":1}"#;
const MARK_FAILS: &str = r#"{"ok":false,"error":{"code":"watch_not_started","message":"No engine has started in this home."}}"#;
const ENSURE_FAILS: &str =
    r#"{"ok":false,"error":{"code":"watch_start_failed","message":"The engine did not start."}}"#;
const NO_VERDICT: &str = r#"{"ok":true,"version":"0.0.0","protocol":1,"store":null}"#;
const MIGRATED: &str = r#"{"ok":true,"migrated":[]}"#;
const REFUSING: &str = r#"{"ok":true,"version":"0.0.0","protocol":1,"store":{"opens":false,"refusal":{"code":"store_failed","message":"not a store"}}}"#;

/// What one run of the install left: its exit and its stderr.
struct Run {
    exit: i32,
    stderr: String,
}

/// The old engine: a holder of `locks/watch.lock` (a `File` of the test) that lets go at a virtual time.
#[derive(Default)]
struct OldEngine {
    file: Mutex<Option<fs::File>>,
    release_at: Mutex<Option<Duration>>,
}

impl OldEngine {
    fn hold(&self, lock: &Path) {
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock)
            .unwrap();
        assert!(
            crate::lock::try_lock(&file).unwrap(),
            "the old engine's lock was free"
        );
        *self.file.lock().unwrap() = Some(file);
    }

    /// The engine lets go of the lock once virtual time is `at`.
    fn release_at(&self, at: Duration, now: Duration) {
        *self.release_at.lock().unwrap() = Some(at);
        self.tick(now);
    }

    fn tick(&self, now: Duration) {
        let mut at = self.release_at.lock().unwrap();
        if at.is_some_and(|at| now >= at) {
            self.file.lock().unwrap().take();
            *at = None;
        }
    }
}

/// The `ManualClock`, and the old engine's release when a nap brings virtual time to it.
struct WatchingClock {
    inner: Arc<ManualClock>,
    old: Arc<OldEngine>,
}

impl Clock for WatchingClock {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        self.old.tick(self.inner.elapsed());
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

/// What killing the work session's `watch` window does to the old engine.
#[derive(Clone, Copy)]
enum Kill {
    /// The window's process ends with it: the lock is free at once.
    EndsTheEngine,
    /// The lock is held for this long after the kill (virtual time).
    HoldsFor(Duration),
    /// Another process holds the lock, for ever.
    HoldsForEver,
    /// `kill-window` fails: `no such window`.
    Fails,
    /// `kill-window` fails `no such window`: the engine stood down first, its window and lock are already gone.
    AlreadyGone,
}

/// The `tmux` route: a table of windows (session, name). `list-windows` answers one session's, `kill-window`
/// removes a window and lets the old engine go as `kill` says; every other call answers exit 0 and nothing.
struct Tmux {
    windows: Mutex<Vec<(String, String)>>,
    kill: Mutex<Kill>,
    old: Arc<OldEngine>,
    clock: Arc<ManualClock>,
}

impl Handler for Tmux {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        let target = args
            .iter()
            .position(|a| *a == "-t")
            .and_then(|i| args.get(i + 1))
            .and_then(|t| t.strip_prefix('='))
            .unwrap_or("");
        if args.contains(&"list-panes") {
            return Reply::ok("0\n");
        }
        if args.contains(&"list-windows") {
            let names: Vec<String> = self
                .windows
                .lock()
                .unwrap()
                .iter()
                .filter(|(s, _)| s == target)
                .map(|(_, w)| format!("{w}\n"))
                .collect();
            return Reply::ok(&names.concat());
        }
        if args.contains(&"kill-window") {
            let kill = *self.kill.lock().unwrap();
            let now = self.clock.elapsed();
            match kill {
                Kill::Fails => return Reply::exit(1, "", "no such window"),
                Kill::AlreadyGone => {
                    self.old.release_at(now, now);
                    let (session, window) = target.split_once(':').unwrap_or((target, ""));
                    self.windows
                        .lock()
                        .unwrap()
                        .retain(|(s, w)| !(s == session && w == window));
                    return Reply::exit(1, "", "no such window");
                }
                Kill::EndsTheEngine => self.old.release_at(now, now),
                Kill::HoldsFor(d) => self.old.release_at(now + d, now),
                Kill::HoldsForEver => {}
            }
            let (session, window) = target.split_once(':').unwrap_or((target, ""));
            self.windows
                .lock()
                .unwrap()
                .retain(|(s, w)| !(s == session && w == window));
            return Reply::ok("");
        }
        Reply::ok("")
    }
}

/// The new WebUI instance as the new `cpd` prepares it (pro-d592): `cpd webui --prepare` starts it beside the old one
/// and answers its pid; `cpd webui --ready` answers whether it listens, from virtual time `ready_at` on (`None`:
/// never). Every `--ready` poll logs the answer and how many signals the old instance had received by then.
struct WebUi {
    spawn: Arc<FakeSpawn>,
    clock: Arc<ManualClock>,
    new_pid: Pid,
    prepare: Option<Reply>,
    ready_at: Option<Duration>,
    polls: Mutex<Vec<(bool, usize)>>,
}

impl WebUi {
    fn handle(&self, args: &[&str]) -> Option<Reply> {
        match args {
            ["webui", "--prepare"] => Some(self.prepare.clone().unwrap_or_else(|| {
                Reply::ok(&format!(r#"{{"ok":true,"pid":{}}}"#, self.new_pid.0))
            })),
            ["webui", "--ready"] => {
                let ready = self.ready_at.is_some_and(|at| self.clock.elapsed() >= at);
                self.polls
                    .lock()
                    .unwrap()
                    .push((ready, self.spawn.signals().len()));
                Some(Reply::ok(&format!(r#"{{"ok":true,"ready":{ready}}}"#)))
            }
            _ => None,
        }
    }
}

/// How the new `cpd watch --ensure` ends.
#[derive(Clone, Copy)]
enum Ensure {
    /// It starts an engine: a holder of `locks/watch.lock`.
    Starts,
    /// It answers `started` and no process holds the lock.
    AnswersOnly,
    /// It fails with `watch_start_failed`.
    Fails,
}

/// The new `cpd` as a Lead's install finds it: its answers are replies, its `watch --ensure` starts an engine (the
/// `File` is kept until the handler drops). Every call is logged by the runner.
struct NewCpd {
    lock: PathBuf,
    config: Reply,
    check: Reply,
    respawn: Reply,
    mark: Reply,
    ensure: Ensure,
    engine: Mutex<Vec<fs::File>>,
    webui: Arc<WebUi>,
}

impl Handler for NewCpd {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        if let Some(reply) = self.webui.handle(&args) {
            return reply;
        }
        match args.as_slice() {
            ["config"] => self.config.clone(),
            ["watch", "--check"] => self.check.clone(),
            ["tui", "--respawn"] => self.respawn.clone(),
            ["watch", "--mark-restart"] => self.mark.clone(),
            ["watch", "--ensure"] => match self.ensure {
                Ensure::Fails => Reply::exit(1, ENSURE_FAILS, ""),
                Ensure::AnswersOnly => Reply::ok(r#"{"ok":true,"started":true}"#),
                Ensure::Starts => {
                    fs::create_dir_all(self.lock.parent().unwrap()).unwrap();
                    let file = fs::OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .read(true)
                        .write(true)
                        .open(&self.lock)
                        .unwrap();
                    assert!(
                        crate::lock::try_lock(&file).unwrap(),
                        "the engine's lock was free when `watch --ensure` ran"
                    );
                    self.engine.lock().unwrap().push(file);
                    Reply::ok(r#"{"ok":true,"started":true}"#)
                }
            },
            _ => Reply::ok(""),
        }
    }
}

/// A checkout (a clone through the git route), a built binary, a `HOME` and a `CPD_HOME`, the new `cpd`, the tmux
/// route with its windows and the old engine.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    ports: Ports,
    route: Arc<GitRoute>,
    install: Install,
    old: Arc<OldEngine>,
    tmux: Arc<Tmux>,
    webui: Arc<WebUi>,
    old_webui: Pid,
}

/// What a rig is built with: the replies of the new `cpd`, the tagger's, the built binary's.
struct Script {
    config: Reply,
    check: Reply,
    respawn: Reply,
    mark: Reply,
    ensure: Ensure,
    kill: Kill,
    tag: Reply,
    refuse_first: bool,
    lock_wait: Duration,
    /// Virtual time from which the new WebUI reports ready; `None` never.
    webui_ready_at: Option<Duration>,
    webui_prepare: Option<Reply>,
}

impl Default for Script {
    fn default() -> Script {
        Script {
            config: Reply::ok(CONFIG),
            check: Reply::ok(CHECK_OK),
            respawn: Reply::ok(RESPAWN_OK),
            mark: Reply::ok(MARK_OK),
            ensure: Ensure::Starts,
            kill: Kill::EndsTheEngine,
            tag: Reply::ok(""),
            refuse_first: false,
            lock_wait: DEFAULT_LOCK_WAIT,
            webui_ready_at: Some(Duration::ZERO),
            webui_prepare: None,
        }
    }
}

impl Rig {
    fn new() -> Rig {
        Rig::with(Script::default())
    }

    fn with(script: Script) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let fakes = Fakes::new();
        let mut universe = Universe::new(fakes.ports.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        let universe = Arc::new(Mutex::new(universe));
        let route = Arc::new(GitRoute::new(universe.clone()));

        let home = root.join("home");
        let cpd_home = root.join("cpd-home");
        let target_dir = root.join("target");
        for dir in [
            &home,
            &cpd_home,
            &root.join("repos"),
            &target_dir.join("release"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        let built = target_dir.join("release/cpd");
        fs::write(&built, "built cpd\n").unwrap();
        fs::set_permissions(&built, fs::Permissions::from_mode(0o700)).unwrap();

        let origin = Repo::init(&universe, &root.join("repos/install-origin"));
        for file in ["package.json", "extension.ts", "lib/cpd.ts"] {
            origin.write(&format!("pi/{file}"), &format!("{file}\n"));
        }
        origin.write("scripts/tag-version.sh", "tag-version.sh\n");
        origin.write(&format!("{MIGRATIONS}/.keep"), "");
        origin.commit("install files");

        let install = Install {
            root: root.join("repos/install-clone"),
            target_dir,
            preview_dir: home.join(".cache/cpd-preview"),
            macos: false,
            home,
            cpd_home: CpdHome::resolve(Some(OsString::from(cpd_home)), None).unwrap(),
            pid: 4242,
            lock_wait: script.lock_wait,
        };
        let old = Arc::new(OldEngine::default());
        let tmux = Arc::new(Tmux {
            windows: Mutex::default(),
            kill: Mutex::new(script.kill),
            old: old.clone(),
            clock: fakes.clock.clone(),
        });
        // The old WebUI serves on 3333 and recorded its pid in `run/webui.pid`.
        let old_webui = fakes
            .spawn
            .add_process(Pid(1), "node web/src/server/serve.ts");
        let new_webui = fakes
            .spawn
            .add_process(Pid(1), "node web/src/server/serve.ts");
        fs::create_dir_all(install.cpd_home.run_dir()).unwrap();
        fs::write(
            install.cpd_home.run_dir().join("webui.pid"),
            format!("{}\n", old_webui.0),
        )
        .unwrap();
        let webui = Arc::new(WebUi {
            spawn: fakes.spawn.clone(),
            clock: fakes.clock.clone(),
            new_pid: new_webui,
            prepare: script.webui_prepare.clone(),
            ready_at: script.webui_ready_at,
            polls: Mutex::default(),
        });
        let new_cpd = Arc::new(NewCpd {
            lock: install.cpd_home.watch_lock(),
            config: script.config,
            check: script.check,
            respawn: script.respawn,
            mark: script.mark,
            ensure: script.ensure,
            engine: Mutex::default(),
            webui: webui.clone(),
        });
        let tag = install.root.join("scripts/tag-version.sh");
        let built_migrate = {
            let built = built.clone();
            move |c: &Call| Path::new(&c.program) == built && c.args == ["config", "--migrate"]
        };
        let built_version =
            move |c: &Call| Path::new(&c.program) == built && c.args == ["--version"];
        let runner = (*fakes.runner).clone().with_git(&route);
        let runner = if script.refuse_first {
            runner.once("cpd", built_version.clone(), Reply::ok(REFUSING))
        } else {
            runner
        };
        runner
            .on("cpd", built_version, Reply::ok(NO_VERDICT))
            .on("cpd", built_migrate, Reply::ok(MIGRATED))
            .on(
                "tag-version.sh",
                move |c| Path::new(&c.program) == tag,
                script.tag,
            )
            .route("tmux", tmux.clone())
            .route("cpd", new_cpd);

        let ports = Ports {
            clock: Arc::new(WatchingClock {
                inner: fakes.clock.clone(),
                old: old.clone(),
            }),
            ..fakes.ports.clone()
        };
        let rig = Rig {
            _tmp: tmp,
            fakes,
            ports,
            route,
            install,
            old,
            tmux,
            webui,
            old_webui,
        };
        let clone = rig.git(
            &root,
            &[
                "clone",
                "-q",
                origin.dir().to_str().unwrap(),
                rig.install.root.to_str().unwrap(),
            ],
        );
        assert_eq!(clone.0, 0, "the fixture's clone: {}", clone.1);
        rig
    }

    fn git(&self, dir: &Path, args: &[&str]) -> (i32, String) {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir).args(args);
        let out = self
            .ports
            .runner
            .output(&mut cmd, "git", Duration::from_secs(60))
            .unwrap_or_else(|e| panic!("git {args:?}: {e:?}"));
        (
            std::os::unix::process::ExitStatusExt::into_raw(out.status) >> 8,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn installed_cpd(&self) -> PathBuf {
        self.install.home.join(".local/bin/cpd")
    }

    fn tagger(&self) -> PathBuf {
        self.install.root.join("scripts/tag-version.sh")
    }

    /// The cockpit has its `main` window and the work session its `watch` window, whose process holds
    /// `locks/watch.lock` (killing the window ends it, as the route's `Kill` says).
    fn running_old_engine(&self) {
        self.cockpit_and_work_windows();
        self.old.hold(&self.install.cpd_home.watch_lock());
    }

    /// The same windows, and no old engine process.
    fn cockpit_and_work_windows(&self) {
        let mut windows = self.tmux.windows.lock().unwrap();
        windows.push(("cock".into(), "main".into()));
        windows.push(("work".into(), "watch".into()));
    }

    fn run(&self) -> Run {
        let mut stderr = Vec::new();
        let exit = run(&self.ports, &self.install, &mut stderr);
        self.route.assert_supported();
        Run {
            exit,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }

    /// The follow-ups the installed `cpd` was asked for, in order.
    fn follow_ups(&self) -> Vec<String> {
        let installed = self.installed_cpd();
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| Path::new(&c.program) == installed)
            .map(|c| c.args.join(" "))
            .filter(|c| c.starts_with("tui") || c.starts_with("watch"))
            .collect()
    }

    /// The one log of the order: `cpd <argv>` of the installed binary and `tmux <argv>`, one line each.
    fn order(&self) -> Vec<String> {
        let installed = self.installed_cpd();
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter_map(|c| {
                if Path::new(&c.program) == installed {
                    Some(format!("cpd {}", c.args.join(" ")))
                } else if c.program.rsplit('/').next() == Some("tmux") {
                    Some(format!("tmux {}", c.args.join(" ")))
                } else {
                    None
                }
            })
            .collect()
    }

    fn tagger_runs(&self) -> usize {
        let tagger = self.tagger();
        self.fakes
            .runner
            .calls()
            .iter()
            .filter(|c| Path::new(&c.program) == tagger)
            .count()
    }

    /// Whether some other open file description holds the engine's lock.
    fn lock_held(&self) -> bool {
        let lock = self.install.cpd_home.watch_lock();
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock)
            .unwrap();
        !crate::lock::try_lock(&file).unwrap()
    }

    fn work_windows(&self) -> Vec<String> {
        self.tmux
            .windows
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s == "work")
            .map(|(_, w)| w.clone())
            .collect()
    }

    fn assert_old_engine_runs(&self, run: &Run) {
        assert!(
            self.work_windows().iter().any(|w| w == "watch"),
            "the old engine's window was killed: {}",
            run.stderr
        );
        assert!(
            self.lock_held(),
            "the old engine's process was stopped: {}",
            run.stderr
        );
    }
}

fn lines_with<'a>(log: &'a [String], word: &str) -> Vec<&'a String> {
    log.iter().filter(|l| l.contains(word)).collect()
}

const ALL_FOUR: [&str; 3] = ["watch --check", "watch --mark-restart", "watch --ensure"];

#[test]
fn a_failing_config_is_printed_and_makes_the_script_exit_non_zero() {
    let rig = Rig::with(Script {
        config: Reply::exit(
            1,
            r#"{"ok":false,"error":{"code":"lock_timeout"}}"#,
            "waiting",
        ),
        ..Script::default()
    });
    rig.cockpit_and_work_windows();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(rig.installed_cpd().exists());
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: cpd config: "),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("lock_timeout"), "{}", run.stderr);
    assert!(rig.follow_ups().is_empty());
}

#[test]
fn a_failing_follow_up_is_printed_and_the_later_ones_still_run() {
    let rig = Rig::with(Script {
        mark: Reply::exit(3, MARK_FAILS, "no engine"),
        ..Script::default()
    });
    rig.cockpit_and_work_windows();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: cpd watch --mark-restart: "),
        "{}",
        run.stderr
    );
    assert_eq!(rig.follow_ups(), ALL_FOUR);
}

#[test]
fn the_follow_ups_run_after_a_slow_config() {
    // A migration holds the setup lock: cpd config answers only after a while (virtual time).
    let rig = Rig::with(Script {
        config: Reply::ok(CONFIG).takes(Duration::from_secs(2)),
        ..Script::default()
    });
    rig.cockpit_and_work_windows();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(rig.follow_ups(), ALL_FOUR);
}

#[test]
fn an_old_engine_that_holds_its_lock_for_longer_than_ten_seconds_is_waited_for() {
    // time-claim: the old engine outlives the former 10 s give-up: it lets go twelve virtual seconds after the kill.
    let rig = Rig::with(Script {
        kill: Kill::HoldsFor(Duration::from_secs(12)),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(rig.follow_ups(), ALL_FOUR);
    assert!(rig.lock_held(), "the new engine holds the lock");
    assert!(
        rig.fakes.clock.elapsed() >= Duration::from_secs(12),
        "the install waited for the old engine's lock: {:?}",
        rig.fakes.clock.elapsed()
    );
}

#[test]
fn an_old_engine_that_never_frees_its_lock_fails_the_install_with_the_reason() {
    // time-claim: the bound is 2 s here; the old engine holds well past it (120 virtual seconds).
    let rig = Rig::with(Script {
        kill: Kill::HoldsFor(Duration::from_secs(120)),
        lock_wait: Duration::from_secs(2),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: the old engine still holds "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("after 2s: it was told to stop and has not; the new engine was not started"),
        "{}",
        run.stderr
    );
    assert_eq!(rig.follow_ups(), ["watch --check", "watch --mark-restart"]);
}

#[test]
fn a_new_engine_that_does_not_run_fails_the_install_with_the_reason() {
    // --ensure answers, but no process holds the lock.
    let rig = Rig::with(Script {
        ensure: Ensure::AnswersOnly,
        ..Script::default()
    });
    rig.cockpit_and_work_windows();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("but no engine holds "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("no engine is running"),
        "{}",
        run.stderr
    );
}

// ---- The install's engine order (hs-d463; contract §3.7 "The install's engine order", docs/implementing.md "The install's engine order") ----
//
// Verify with the new binary (`cpd config`, `cpd watch --check`), then retire the old engine, then start the new one: the running
// engine is never stopped before the new binary is known to be able to start one. The tmux route holds the old engine's window
// (`work:watch`, a process holding `locks/watch.lock`); the runner logs the calls of the new `cpd` and of `tmux`, in order.

#[test]
fn a_failing_watch_check_leaves_the_old_engine_and_the_views_alone() {
    let rig = Rig::with(Script {
        check: Reply::exit(1, CHECK_FAILS, ""),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: cpd watch --check: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("tmux_failed"),
        "the failing step's own code: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("the engine was not touched"),
        "{}",
        run.stderr
    );
    assert!(
        lines_with(&log, "cpd watch --check").len() == 1,
        "the new binary's watch --check ran once: {log:?}"
    );
    assert!(
        lines_with(&log, "kill-window").is_empty(),
        "a kill-window ran: {log:?}"
    );
    assert!(
        lines_with(&log, "respawn-pane").is_empty()
            && lines_with(&log, "cpd tui --respawn").is_empty(),
        "the views were respawned: {log:?}"
    );
    assert!(
        lines_with(&log, "cpd watch --ensure").is_empty(),
        "watch --ensure ran: {log:?}"
    );
    rig.assert_old_engine_runs(&run);
}

#[test]
fn a_failing_config_leaves_the_old_engine_alone_and_says_so() {
    let rig = Rig::with(Script {
        config: Reply::exit(
            1,
            r#"{"ok":false,"error":{"code":"config_invalid","message":"bad"}}"#,
            "",
        ),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: cpd config: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("the engine was not touched"),
        "{}",
        run.stderr
    );
    assert!(
        lines_with(&log, "kill-window").is_empty() && lines_with(&log, "watch --ensure").is_empty(),
        "{log:?}"
    );
    rig.assert_old_engine_runs(&run);
}

#[test]
fn a_new_engine_that_cannot_start_after_the_retirement_says_the_old_one_was_stopped() {
    let rig = Rig::with(Script {
        ensure: Ensure::Fails,
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert_eq!(lines_with(&log, "kill-window").len(), 1, "{log:?}");
    assert_eq!(lines_with(&log, "cpd watch --ensure").len(), 1, "{log:?}");
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: cpd watch --ensure: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("the old engine was stopped and no engine is running"),
        "{}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("still runs"),
        "a failure after the retirement cannot say the old engine runs: {}",
        run.stderr
    );
}

#[test]
fn a_lock_the_old_engine_keeps_past_the_wait_means_the_new_engine_is_never_started() {
    // The window is killed, but its process is not the one holding the lock: another process does, for longer than the wait.
    // time-claim: the bound is 1 s here; the lock is held for ever (virtual time).
    let rig = Rig::with(Script {
        kill: Kill::HoldsForEver,
        lock_wait: Duration::from_secs(1),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: the old engine still holds "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("after 1s: it was told to stop and has not; the new engine was not started"),
        "{}",
        run.stderr
    );
    assert!(lines_with(&log, "cpd watch --ensure").is_empty(), "{log:?}");
}

#[test]
fn a_kill_window_that_fails_says_the_engine_was_not_stopped_and_still_runs_the_old_binary() {
    let rig = Rig::with(Script {
        kill: Kill::Fails,
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: tmux kill-window work:watch failed: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("the engine was not stopped and still runs the old binary"),
        "{}",
        run.stderr
    );
    rig.assert_old_engine_runs(&run);
}

#[test]
fn a_kill_window_that_fails_because_the_engine_already_left_is_not_a_failure() {
    let rig = Rig::with(Script {
        kill: Kill::AlreadyGone,
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        !run.stderr.contains("cpd install follow-up failed"),
        "{}",
        run.stderr
    );
    let ensures = lines_with(&log, "cpd watch --ensure");
    assert_eq!(ensures.len(), 1, "{log:?}");
    let at = |word: &str| {
        log.iter()
            .position(|l| l.contains(word))
            .unwrap_or_else(|| panic!("no `{word}` in {log:?}"))
    };
    assert!(at("kill-window") < at("cpd watch --ensure"), "{log:?}");
    assert!(rig.lock_held(), "the new engine holds the lock");
}

#[test]
fn a_good_install_verifies_then_respawns_the_views_then_retires_the_old_engine_and_starts_the_new_one()
 {
    let rig = Rig::new();
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(lines_with(&log, "kill-window").len(), 1, "{log:?}");
    assert_eq!(lines_with(&log, "cpd watch --ensure").len(), 1, "{log:?}");
    let at = |word: &str| {
        log.iter()
            .position(|l| l.contains(word))
            .unwrap_or_else(|| panic!("no `{word}` in {log:?}"))
    };
    let steps = [
        at("cpd watch --check"),
        at("kill-window"),
        at("cpd watch --ensure"),
    ];
    assert!(
        steps.windows(2).all(|w| w[0] < w[1]),
        "the order is --check, kill-window, --ensure: {steps:?} in {log:?}"
    );
    assert!(
        !rig.work_windows().iter().any(|w| w == "watch"),
        "the old engine's window is gone"
    );
    assert!(rig.lock_held(), "the new engine holds the lock");
}

// ---- The tag step (scripts/tag-version.sh runs once, at the end of a successful install; a failure of it only warns) ----

#[test]
fn a_successful_install_runs_tag_version_once_at_the_end() {
    let rig = Rig::new();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(rig.installed_cpd().exists(), "the fake cpd is installed");
    assert_eq!(rig.tagger_runs(), 1, "scripts/tag-version.sh ran once");
}

#[test]
fn a_failing_tag_step_warns_and_does_not_fail_the_install() {
    let rig = Rig::with(Script {
        tag: Reply::exit(1, "", "tag-version: boom"),
        ..Script::default()
    });

    let run = rig.run();

    assert_eq!(rig.tagger_runs(), 1, "scripts/tag-version.sh ran once");
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(rig.installed_cpd().exists(), "the fake cpd is installed");
    assert!(run.stderr.contains("tag-version: boom"), "{}", run.stderr);
}

#[test]
fn a_refused_install_does_not_run_tag_version() {
    // The built binary refuses its first `--version`, and answers a store that does not exist after that.
    let rig = Rig::with(Script {
        refuse_first: true,
        ..Script::default()
    });

    let refused = rig.run();

    assert_eq!(refused.exit, 1, "{}", refused.stderr);
    assert_eq!(
        rig.tagger_runs(),
        0,
        "the tag step ran in a refused install"
    );
    // Control: the same recorder runs in an install that is not refused.
    let accepted = rig.run();
    assert_eq!(accepted.exit, 0, "{}", accepted.stderr);
    assert_eq!(
        rig.tagger_runs(),
        1,
        "the recorder is wired: an accepted install runs it once"
    );
}

// ---------- the install marks the engine restart as planned (hs-d471, contract §3.7 "The install's engine order", step 3) ----------

#[test]
fn a_good_install_marks_the_restart_after_the_check_and_right_before_the_kill_window() {
    let rig = Rig::new();
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(
        lines_with(&log, "cpd watch --mark-restart").len(),
        1,
        "{log:?}"
    );
    let at = |word: &str| {
        log.iter()
            .position(|l| l.contains(word))
            .unwrap_or_else(|| panic!("no `{word}` in {log:?}"))
    };
    let steps = [
        at("cpd watch --check"),
        at("cpd watch --mark-restart"),
        at("kill-window"),
        at("cpd watch --ensure"),
    ];
    assert!(
        steps.windows(2).all(|w| w[0] < w[1]),
        "the order is --check, --mark-restart, kill-window, --ensure: {steps:?} in {log:?}"
    );
    let between = &log[at("cpd watch --mark-restart") + 1..at("kill-window")];
    assert!(
        between
            .iter()
            .all(|l| l.starts_with("tmux ") && !l.contains("respawn-pane")),
        "nothing but the retirement's own lookups between the mark and the kill: {between:?}"
    );
}

#[test]
fn a_mark_restart_that_fails_is_printed_and_the_retirement_goes_on() {
    let rig = Rig::with(Script {
        mark: Reply::exit(3, MARK_FAILS, ""),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let log = rig.order();
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install follow-up failed: cpd watch --mark-restart: ")
            && run
                .stderr
                .contains("the engine restart is not marked as planned"),
        "{}",
        run.stderr
    );
    assert_eq!(
        lines_with(&log, "kill-window").len(),
        1,
        "the retirement went on: {log:?}"
    );
    assert_eq!(lines_with(&log, "cpd watch --ensure").len(), 1, "{log:?}");
}

fn waiter_calls(rig: &Rig) -> Vec<Call> {
    let installed = rig.installed_cpd();
    rig.fakes
        .runner
        .calls()
        .into_iter()
        .filter(|c| Path::new(&c.program) == installed && c.args == super::follow_up::WAITER_ARGS)
        .collect()
}

#[test]
fn lock_timeout_hands_off_restart_waiter() {
    // time-claim: the old engine holds 600 virtual seconds, well past DEFAULT_LOCK_WAIT.
    let rig = Rig::with(Script {
        kill: Kill::HoldsFor(Duration::from_secs(600)),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    assert_eq!(run.exit, 1, "install_failed still: {}", run.stderr);
    assert!(
        run.stderr.contains("the old engine still holds "),
        "{}",
        run.stderr
    );
    assert_eq!(
        waiter_calls(&rig).len(),
        1,
        "one detached waiter is spawned"
    );
    assert!(
        !rig.follow_ups().contains(&"watch --ensure".to_owned()),
        "the install itself does not start the engine"
    );
    let state = fs::read_to_string(rig.install.cpd_home.run_dir().join("cpd-install.json"))
        .expect("cpd-install.json is written");
    let state: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["status"], "restarting", "{state}");
}

#[test]
fn restart_waiter_runs_ensure_once_when_lock_frees() {
    let rig = Rig::new();
    rig.running_old_engine();
    rig.old
        .release_at(Duration::from_secs(90), rig.fakes.clock.elapsed());

    super::follow_up::restart_waiter(&rig.ports, &rig.install.cpd_home, &rig.installed_cpd())
        .unwrap();

    let ensures = rig
        .follow_ups()
        .into_iter()
        .filter(|c| c == "watch --ensure")
        .count();
    assert_eq!(ensures, 1, "exactly one `cpd watch --ensure`");
    assert!(
        rig.fakes.clock.elapsed() >= Duration::from_secs(90),
        "it waited for the lock on the clock"
    );
}

// The WebUI handoff (pro-d592, bug b74, #1245). The interface these tests pin: after the engine restart the install
// runs the new `cpd webui --prepare` (starts the new WebUI beside the old one, answers `{"pid":N}`), polls
// `cpd webui --ready` on the clock until it answers `{"ready":true}`, and only then sends SIGTERM to the old WebUI,
// whose pid is in `run/webui.pid`. A new WebUI that is never ready leaves the old one unsignalled and fails the install.

#[test]
fn old_webui_kept_until_new_webui_ready() {
    // The engine is slow to stop (30 s) and the new WebUI is ready only at 45 s.
    let rig = Rig::with(Script {
        kill: Kill::HoldsFor(Duration::from_secs(30)),
        webui_ready_at: Some(Duration::from_secs(45)),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    let polls = rig.webui.polls.lock().unwrap().clone();
    assert!(
        !polls.is_empty(),
        "the install polls the new WebUI's readiness: {}",
        run.stderr
    );
    for (ready, signals) in &polls {
        if !ready {
            assert_eq!(
                *signals, 0,
                "the old WebUI was signalled before the new one was ready"
            );
        }
    }
    assert!(
        rig.fakes.clock.elapsed() >= Duration::from_secs(45),
        "the install waited, on the clock, for the new WebUI"
    );
}

#[test]
fn old_webui_stopped_only_after_readiness_handoff() {
    let rig = Rig::with(Script {
        webui_ready_at: Some(Duration::from_secs(5)),
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let polls = rig.webui.polls.lock().unwrap().clone();
    assert_eq!(
        polls.last(),
        Some(&(true, 0)),
        "the last poll answered ready with the old WebUI not yet signalled"
    );
    assert_eq!(
        rig.fakes.spawn.signals(),
        vec![(rig.old_webui, Signal::Term)],
        "exactly the old WebUI is told to stop, once the new one is ready"
    );
    assert!(
        rig.fakes.spawn.alive(rig.webui.new_pid),
        "the new WebUI keeps running and owns the port"
    );
}

#[test]
fn new_webui_not_ready_leaves_old_serving_and_fails_install() {
    let rig = Rig::with(Script {
        webui_ready_at: None,
        ..Script::default()
    });
    rig.running_old_engine();

    let run = rig.run();

    assert_ne!(run.exit, 0, "the install fails: {}", run.stderr);
    assert!(
        run.stderr.contains("install_failed"),
        "the install reports install_failed: {}",
        run.stderr
    );
    assert!(
        !rig.webui.polls.lock().unwrap().is_empty(),
        "the install polled the new WebUI before giving up"
    );
    assert!(
        rig.fakes.spawn.signals().is_empty(),
        "the old WebUI is never signalled"
    );
    assert!(
        rig.fakes.spawn.alive(rig.old_webui),
        "the old WebUI keeps serving"
    );
}

#[test]
fn failed_webui_prepare_leaves_the_running_engine_untouched() {
    let rig = Rig::with(Script {
        webui_prepare: Some(Reply::exit(1, "", "cannot bind listener")),
        ..Script::default()
    });
    rig.running_old_engine();
    let out = rig.run();
    assert_ne!(out.exit, 0, "{}", out.stderr);
    assert!(rig.fakes.spawn.signals().is_empty());
    let calls = rig.fakes.runner.calls();
    assert!(
        !calls
            .iter()
            .any(|c| c.args.iter().any(|a| a == "kill-window"))
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.args.iter().any(|a| a == "--mark-restart"))
    );
}

#[test]
fn an_ok_webui_prepare_naming_no_instance_proceeds_to_the_engine_restart() {
    let rig = Rig::with(Script {
        webui_prepare: Some(Reply::ok(r#"{"ok":true}"#)),
        ..Script::default()
    });
    rig.running_old_engine();
    let out = rig.run();
    assert!(!out.stderr.contains("webui --prepare"), "{}", out.stderr);
    let calls = rig.fakes.runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.args.iter().any(|a| a == "kill-window"))
    );
}

#[test]
fn failed_restart_waiter_reports_ensure_diagnostics_and_records_failed() {
    let rig = Rig::with(Script {
        ensure: Ensure::Fails,
        ..Script::default()
    });
    let error =
        super::follow_up::restart_waiter(&rig.ports, &rig.install.cpd_home, &rig.installed_cpd())
            .unwrap_err();
    assert!(error.message().contains("watch --ensure failed"), "{error}");
    assert!(
        error.message().contains("The engine did not start"),
        "{error}"
    );
    let state = crate::host::install_state::InstallState::read(&rig.install.cpd_home)
        .unwrap()
        .unwrap();
    assert!(state.alert().unwrap().contains("watch --ensure failed"));
}

#[test]
fn expired_restart_waiter_records_failure_without_ensuring_the_old_engine() {
    let rig = Rig::new();
    rig.running_old_engine();
    let error =
        super::follow_up::restart_waiter(&rig.ports, &rig.install.cpd_home, &rig.installed_cpd())
            .unwrap_err();
    assert!(error.message().contains("timed out"), "{error}");
    assert!(!rig.follow_ups().iter().any(|call| call == "watch --ensure"));
    let state = crate::host::install_state::InstallState::read(&rig.install.cpd_home)
        .unwrap()
        .unwrap();
    assert!(state.alert().unwrap().contains("timed out"));
}
