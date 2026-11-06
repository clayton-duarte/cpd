//! `core::app` (docs/implementing.md "`cpd app`", commands.md §14.4a), in-process on `Fakes`: the runner answers `pnpm`,
//! `plutil` and `sh`, `FakeSpawn` records the launches and signals, `HOME` and `CPD_HOME` are directories of a temp dir.
//! No real pnpm, plutil, Electron or signal, and no sleep.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use super::{App, Electron, LaunchAction, Preview, ensure, launch, preview, read_root, status};
use crate::error::ErrorCode;
use crate::host::runner::{Call, Handler, Reply};
use crate::host::spawn::{Pid, Signal};
use crate::ports::Fakes;

/// Answers every call of its program that no rule matched (rules come first, so a test's own rule wins).
struct Always(Reply);

impl Handler for Always {
    fn handle(&self, _: &Call) -> Reply {
        self.0.clone()
    }
}

struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    app: App,
}

impl Rig {
    /// A checkout whose bundle is built (`web/dist-electron/main.js`), and runner rules for `pnpm`, `plutil` and `sh`.
    fn new() -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let app = App {
            root: base.join("checkout"),
            home: base.join("home"),
            cpd_home: base.join("cpd-home"),
            preview_dir: base.join("preview"),
            macos: false,
        };
        for dir in [
            app.root.join("web/dist-electron"),
            app.root.join("scripts"),
            app.cpd_home.join("run"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(app.root.join("web/dist-electron/main.js"), "main\n").unwrap();
        let fakes = Fakes::new();
        let _ = (*fakes.runner)
            .clone()
            .route("pnpm", Arc::new(Always(Reply::ok(""))))
            .route("plutil", Arc::new(Always(Reply::ok(""))))
            .route("sh", Arc::new(Always(Reply::ok("script text\n"))));
        Rig {
            _tmp: tmp,
            fakes,
            app,
        }
    }

    fn missing_main_js(&self) {
        fs::remove_file(self.app.root.join("web/dist-electron/main.js")).unwrap();
    }

    fn calls(&self, program: &str) -> Vec<Call> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| {
                Path::new(&c.program)
                    .file_name()
                    .is_some_and(|n| n == program)
            })
            .collect()
    }

    fn argv(&self, program: &str) -> Vec<Vec<String>> {
        self.calls(program).into_iter().map(|c| c.args).collect()
    }

    fn build_argv(&self) -> Vec<String> {
        ["--dir", self.app.root.to_str().unwrap()]
            .into_iter()
            .chain(["--filter", "cpd-web-client", "run", "build:electron"])
            .map(str::to_owned)
            .collect()
    }

    fn plist(&self) -> String {
        self.app
            .root
            .join("web/node_modules/electron/dist/Electron.app/Contents/Info.plist")
            .to_str()
            .unwrap()
            .to_owned()
    }

    fn script(&self) -> String {
        self.app
            .root
            .join("scripts/preview-web.sh")
            .to_str()
            .unwrap()
            .to_owned()
    }

    fn health(&self, json: &str) {
        fs::write(self.app.cpd_home.join("run/preview-health.json"), json).unwrap();
    }

    /// A running Electron: a live pid of the fake spawn, named by the health file.
    fn running_electron(&self) -> Pid {
        let pid = self.fakes.spawn.add_process(Pid(1), "electron .");
        self.health(&format!(r#"{{"status":"ok","pid":{}}}"#, pid.0));
        pid
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn app_ensure_builds_when_main_js_is_missing() {
    let rig = Rig::new();
    rig.missing_main_js();
    ensure(&rig.fakes.ports, &rig.app, false).unwrap();
    assert_eq!(rig.argv("pnpm"), vec![rig.build_argv()]);
}

#[test]
fn app_ensure_skips_build_when_main_js_exists() {
    let rig = Rig::new();
    ensure(&rig.fakes.ports, &rig.app, false).unwrap();
    assert_eq!(rig.argv("pnpm"), Vec::<Vec<String>>::new());
    // The install always builds, to pick up new code.
    ensure(&rig.fakes.ports, &rig.app, true).unwrap();
    assert_eq!(rig.argv("pnpm"), vec![rig.build_argv()]);
}

#[test]
fn app_ensure_marks_spawn_helper_executable() {
    let rig = Rig::new();
    let helpers: Vec<PathBuf> = ["darwin-arm64", "linux-x64"]
        .iter()
        .map(|dir| {
            let dir = rig
                .app
                .root
                .join("web/node_modules/node-pty/prebuilds")
                .join(dir);
            fs::create_dir_all(&dir).unwrap();
            let helper = dir.join("spawn-helper");
            fs::write(&helper, "helper\n").unwrap();
            fs::set_permissions(&helper, fs::Permissions::from_mode(0o644)).unwrap();
            helper
        })
        .collect();
    ensure(&rig.fakes.ports, &rig.app, false).unwrap();
    for helper in helpers {
        let mode = fs::metadata(&helper).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "{}", helper.display());
    }
}

#[test]
fn app_ensure_sets_plist_names_when_macos() {
    let mut rig = Rig::new();
    rig.app.macos = true;
    ensure(&rig.fakes.ports, &rig.app, false).unwrap();
    let plist = rig.plist();
    let replace = |key: &str| strings(&["-replace", key, "-string", "CPD", &plist]);
    assert_eq!(
        rig.argv("plutil"),
        vec![replace("CFBundleName"), replace("CFBundleDisplayName")]
    );
}

#[test]
fn app_ensure_skips_plist_when_not_macos() {
    let rig = Rig::new();
    assert!(!rig.app.macos);
    ensure(&rig.fakes.ports, &rig.app, false).unwrap();
    assert_eq!(rig.argv("plutil"), Vec::<Vec<String>>::new());
}

#[test]
fn app_launch_spawns_detached_when_no_electron_runs() {
    let rig = Rig::new();
    let launched = launch(&rig.fakes.ports, &rig.app).unwrap();
    assert_eq!(launched.action, LaunchAction::Launched);
    let launches = rig.fakes.spawn.launches();
    assert_eq!(launches.len(), 1, "{launches:?}");
    let electron = &launches[0];
    assert_eq!(
        Path::new(&electron.program),
        rig.app.root.join("web/node_modules/.bin/electron")
    );
    assert_eq!(electron.args, strings(&["."]));
    assert_eq!(electron.cwd, Some(rig.app.root.join("web")));
    assert!(electron.setsid);
    assert_eq!(
        electron.stdout_to,
        Some(rig.app.cpd_home.join("run/app.log"))
    );
    assert_ne!(launched.pid, 0);
    assert_eq!(rig.fakes.spawn.signals(), vec![]);
}

#[test]
fn app_launch_reloads_with_sigusr1_when_electron_runs() {
    let rig = Rig::new();
    let pid = rig.running_electron();
    let reloaded = launch(&rig.fakes.ports, &rig.app).unwrap();
    assert_eq!(reloaded.action, LaunchAction::Reloaded);
    assert_eq!(reloaded.pid, pid.0);
    assert_eq!(rig.fakes.spawn.signals(), vec![(pid, Signal::Usr1)]);
    assert_eq!(rig.fakes.spawn.launches(), vec![]);
}

#[test]
fn app_launch_ignores_a_stale_health_pid() {
    let rig = Rig::new();
    let pid = rig.running_electron();
    rig.fakes.spawn.exit(pid, 0);
    let launched = launch(&rig.fakes.ports, &rig.app).unwrap();
    assert_eq!(launched.action, LaunchAction::Launched);
    assert_eq!(rig.fakes.spawn.launches().len(), 1);
    assert_eq!(rig.fakes.spawn.signals(), vec![]);
}

/// The one `sh` call: its argv and its deadline.
fn script_call(rig: &Rig) -> (Vec<String>, Duration) {
    let calls = rig.calls("sh");
    assert_eq!(calls.len(), 1, "{calls:?}");
    (calls[0].args.clone(), calls[0].deadline)
}

const FIFTEEN_MINUTES: Duration = Duration::from_secs(15 * 60);

#[test]
fn app_preview_on_runs_preview_script_with_worktree() {
    let rig = Rig::new();
    let text = preview(
        &rig.fakes.ports,
        &rig.app,
        Preview::On(Some(PathBuf::from("/work/tree"))),
    )
    .unwrap();
    assert!(text.contains("script text"), "{text}");
    assert_eq!(
        script_call(&rig),
        (
            strings(&[&rig.script(), "on", "/work/tree"]),
            FIFTEEN_MINUTES
        )
    );
}

#[test]
fn app_reload_runs_preview_script_reload() {
    let rig = Rig::new();
    preview(&rig.fakes.ports, &rig.app, Preview::Reload(None)).unwrap();
    assert_eq!(
        script_call(&rig),
        (strings(&[&rig.script(), "reload"]), FIFTEEN_MINUTES)
    );
}

#[test]
fn app_off_runs_preview_script_off() {
    let rig = Rig::new();
    preview(&rig.fakes.ports, &rig.app, Preview::Off).unwrap();
    assert_eq!(
        script_call(&rig),
        (strings(&[&rig.script(), "off"]), FIFTEEN_MINUTES)
    );
}

#[test]
fn app_status_reports_preview_and_electron_pid() {
    let rig = Rig::new();
    let _ = (*rig.fakes.runner).clone().on(
        "sh",
        |c| c.args.last().is_some_and(|a| a == "status"),
        Reply::exit(0, "preview: on\n", "worktree: /w\n"),
    );
    // No health file: no Electron, and the script's text is still reported.
    let none = status(&rig.fakes.ports, &rig.app).unwrap();
    assert_eq!(none.electron, None);
    assert!(none.output.contains("preview: on"), "{}", none.output);
    assert!(none.output.contains("worktree: /w"), "{}", none.output);
    let pid = rig.running_electron();
    let running = status(&rig.fakes.ports, &rig.app).unwrap();
    assert_eq!(
        running.electron,
        Some(Electron {
            pid: pid.0,
            running: true
        })
    );
    rig.fakes.spawn.exit(pid, 0);
    let stale = status(&rig.fakes.ports, &rig.app).unwrap();
    assert_eq!(
        stale.electron,
        Some(Electron {
            pid: pid.0,
            running: false
        })
    );
    // Status never ensures: nothing is built, and no signal is sent.
    assert_eq!(rig.argv("pnpm"), Vec::<Vec<String>>::new());
    assert_eq!(rig.fakes.spawn.signals(), vec![]);
}

#[test]
fn app_without_root_file_is_refused_app_not_installed() {
    let rig = Rig::new();
    let err = read_root(&rig.app.home).unwrap_err();
    assert_eq!(err.code(), ErrorCode::AppNotInstalled);
    assert!(
        err.message().contains("scripts/install-cpd.sh"),
        "{}",
        err.message()
    );
}
