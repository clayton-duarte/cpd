//! `cpd install`'s Electron step (commands.md §14.4 step 7a): it writes the `app-root` state file `cpd desktop` reads
//! (commands.md §14.4a, docs/implementing.md step 8a), in-process on `Fakes`: the whole install runs on a minimal
//! checkout (`pi/package.json` only), with the built and the installed `cpd`, `git`, `pnpm`, `plutil` and the tagger as
//! runner rules, `FakeSpawn` for the Electron pid. No real process, no sleep.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use super::{DEFAULT_LOCK_WAIT, Install, run};
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::host::spawn::{Pid, Signal};
use crate::ports::Fakes;

const NO_VERDICT: &str = r#"{"ok":true,"version":"0.0.0","protocol":1,"store":null}"#;
const MIGRATED: &str = r#"{"ok":true,"migrated":[]}"#;
const CONFIG: &str =
    r#"{"ok":true,"config":{"tmux":{"socket":"","cockpit_session":"","work_session":""}}}"#;

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
    install: Install,
}

fn named(program: &str, call: &Call) -> bool {
    Path::new(&call.program)
        .file_name()
        .is_some_and(|n| n == program)
}

impl Rig {
    fn new(macos: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let home = base.join("home");
        let cpd_home = base.join("cpd-home");
        let root = base.join("checkout");
        let target_dir = base.join("target");
        for dir in [
            &home,
            &cpd_home,
            &root.join("pi"),
            &root.join("scripts"),
            &target_dir.join("release"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::create_dir_all(cpd_home.join("run")).unwrap();
        fs::write(root.join("pi/package.json"), "{}\n").unwrap();
        fs::create_dir_all(root.join("web")).unwrap();
        fs::write(root.join("web/package.json"), "{}\n").unwrap();
        let built = target_dir.join("release/cpd");
        fs::write(&built, "built cpd\n").unwrap();
        fs::set_permissions(&built, fs::Permissions::from_mode(0o700)).unwrap();
        let installed = home.join(".local/bin/cpd");
        let tag = root.join("scripts/tag-version.sh");
        let fakes = Fakes::new();
        let on_built = |args: &'static [&'static str]| {
            let built = built.clone();
            move |c: &Call| Path::new(&c.program) == built && c.args == args
        };
        let on_installed = |args: &'static [&'static str]| {
            let installed = installed.clone();
            move |c: &Call| Path::new(&c.program) == installed && c.args == args
        };
        let _ = (*fakes.runner)
            .clone()
            .on("git", |_| true, Reply::ok(""))
            .on("cpd", on_built(&["--version"]), Reply::ok(NO_VERDICT))
            .on(
                "cpd",
                on_built(&["config", "--migrate"]),
                Reply::ok(MIGRATED),
            )
            .on("cpd", on_installed(&["config"]), Reply::ok(CONFIG))
            .on(
                "cpd",
                on_installed(&["watch", "--check"]),
                Reply::ok(r#"{"ok":true}"#),
            )
            .on(
                "tag-version.sh",
                move |c| Path::new(&c.program) == tag,
                Reply::ok(""),
            )
            .route("pnpm", Arc::new(Always(Reply::ok(""))))
            .route("plutil", Arc::new(Always(Reply::ok(""))))
            .route("node", Arc::new(Always(Reply::ok(""))));
        let install = Install {
            root,
            target_dir,
            preview_dir: base.join("preview"),
            macos,
            home,
            cpd_home: CpdHome::resolve(Some(OsString::from(cpd_home)), None).unwrap(),
            pid: 4242,
            lock_wait: DEFAULT_LOCK_WAIT,
        };
        Rig {
            _tmp: tmp,
            fakes,
            install,
        }
    }

    fn run(&self) -> (i32, String) {
        let mut err = Vec::new();
        let exit = run(&self.fakes.ports, &self.install, &mut err);
        (exit, String::from_utf8(err).unwrap())
    }

    fn argv(&self, program: &str) -> Vec<Vec<String>> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| named(program, c))
            .map(|c| c.args)
            .collect()
    }

    fn app_root(&self) -> std::path::PathBuf {
        self.install.home.join(".local/share/cpd/app-root")
    }

    /// A running Electron: a live pid of the fake spawn, named by the health file.
    fn running_electron(&self) -> Pid {
        let pid = self.fakes.spawn.add_process(Pid(1), "electron .");
        fs::write(
            self.install.cpd_home.root().join("run/preview-health.json"),
            format!(r#"{{"pid":{}}}"#, pid.0),
        )
        .unwrap();
        pid
    }

    fn root_arg(&self) -> String {
        self.install.root.to_str().unwrap().to_owned()
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn install_builds_electron_and_writes_app_root() {
    let rig = Rig::new(true);
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    let root = rig.root_arg();
    assert_eq!(
        rig.argv("pnpm"),
        vec![
            strings(&["--dir", &root, "install", "--frozen-lockfile"]),
            strings(&[
                "--dir",
                &root,
                "--filter",
                "cpd-web-client",
                "run",
                "build:electron"
            ]),
        ]
    );
    let plist = rig
        .install
        .root
        .join("web/node_modules/electron/dist/Electron.app/Contents/Info.plist");
    let plist = plist.to_str().unwrap();
    let replace = |key: &str| strings(&["-replace", key, "-string", "CPD", plist]);
    assert_eq!(
        rig.argv("plutil"),
        vec![replace("CFBundleName"), replace("CFBundleDisplayName")]
    );
    assert_eq!(
        fs::read_to_string(rig.app_root()).unwrap(),
        format!("{root}\n")
    );
}

#[test]
fn install_signals_running_electron_when_no_preview() {
    let rig = Rig::new(false);
    let pid = rig.running_electron();
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    assert_eq!(rig.fakes.spawn.signals(), vec![(pid, Signal::Usr1)]);
    // With no live Electron nothing is started.
    assert_eq!(rig.fakes.spawn.launches(), vec![]);
}

#[test]
fn install_leaves_electron_alone_while_a_preview_is_on() {
    let rig = Rig::new(false);
    let _pid = rig.running_electron();
    fs::create_dir_all(&rig.install.preview_dir).unwrap();
    fs::write(rig.install.preview_dir.join("web-state"), "on\n").unwrap();
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    assert_eq!(rig.fakes.spawn.signals(), vec![]);
    assert!(
        stderr.contains("cpd install: web preview is on; Electron was not signalled"),
        "{stderr}"
    );
    // The build and `app-root` are not skipped by a preview.
    assert!(rig.app_root().is_file());
}

#[test]
fn install_reports_a_failed_electron_build_and_exits_nonzero() {
    let rig = Rig::new(false);
    let _pid = rig.running_electron();
    let _ = (*rig.fakes.runner).clone().on(
        "pnpm",
        |c| c.args.iter().any(|a| a == "build:electron"),
        Reply::exit(1, "", "vite exploded\n"),
    );
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 1, "{stderr}");
    assert!(
        stderr.contains("cpd install follow-up failed: electron b:"),
        "{stderr}"
    );
    assert!(stderr.contains("vite exploded"), "{stderr}");
    // The binary is installed already, `app-root` is not written and Electron is not signalled.
    assert!(rig.install.home.join(".local/bin/cpd").is_file());
    assert!(!rig.app_root().exists());
    assert_eq!(rig.fakes.spawn.signals(), vec![]);
}

#[test]
fn install_skips_the_electron_step_when_the_checkout_has_no_web_client() {
    let rig = Rig::new(true);
    fs::remove_dir_all(rig.install.root.join("web")).unwrap();
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    assert!(
        stderr.contains("no web client in the checkout: electron step skipped"),
        "{stderr}"
    );
    assert_eq!(rig.argv("pnpm"), Vec::<Vec<String>>::new());
    assert!(!rig.app_root().exists());
}

fn electron_dir(rig: &Rig) -> std::path::PathBuf {
    rig.install.root.join("web/node_modules/electron")
}

/// Every runner call of the step as `program args`, in order.
fn sequence(rig: &Rig) -> Vec<String> {
    rig.fakes
        .runner
        .calls()
        .into_iter()
        .filter(|c| named("node", c) || named("plutil", c))
        .map(|c| {
            let program = Path::new(&c.program)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            format!("{program} {}", c.args.join(" "))
        })
        .collect()
}

#[test]
fn electron_binary_missing_runs_install_js_before_plutil() {
    let rig = Rig::new(true);
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    let install_js = electron_dir(&rig).join("install.js");
    assert_eq!(
        rig.argv("node"),
        vec![strings(&[install_js.to_str().unwrap()])]
    );
    let seq = sequence(&rig);
    assert!(seq[0].starts_with("node "), "{seq:?}");
    assert_eq!(seq.len(), 3, "{seq:?}");
    assert!(seq[1..].iter().all(|c| c.starts_with("plutil ")), "{seq:?}");
}

#[test]
fn electron_binary_present_skips_install_js() {
    let rig = Rig::new(true);
    let dir = electron_dir(&rig);
    fs::create_dir_all(dir.join("dist/Electron.app")).unwrap();
    fs::write(dir.join("path.txt"), "Electron.app/Contents/MacOS/Electron").unwrap();
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    assert_eq!(rig.argv("node"), Vec::<Vec<String>>::new());
    assert_eq!(rig.argv("plutil").len(), 2);

    // Either half missing (here path.txt) still counts as a missing binary: install.js runs.
    let half = Rig::new(true);
    fs::create_dir_all(electron_dir(&half).join("dist/Electron.app")).unwrap();
    let (exit, stderr) = half.run();
    assert_eq!(exit, 0, "{stderr}");
    assert_eq!(
        half.argv("node").len(),
        1,
        "path.txt absent must run install.js"
    );
}

#[test]
fn electron_binary_install_failure_stops_before_plutil() {
    let rig = Rig::new(true);
    let _ =
        (*rig.fakes.runner)
            .clone()
            .on("node", |_| true, Reply::exit(1, "", "download failed\n"));
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 1, "{stderr}");
    assert!(stderr.contains("download failed"), "{stderr}");
    assert_eq!(rig.argv("node").len(), 1);
    assert_eq!(rig.argv("plutil"), Vec::<Vec<String>>::new());
}
