//! `cpd install --no-follow-up`: the binary and the adapter are written all the same, but the follow-ups
//! (`follow_up::run`) do not run, so a running engine and its views are left as they are. In-process on `Fakes`,
//! like `app_step_tests`: the whole install runs on a minimal checkout with runner rules; no real process, no
//! sleep, no wall clock.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use super::{DEFAULT_LOCK_WAIT, Install, InstallOptions, run_with_options};
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
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

impl Rig {
    /// A minimal checkout (`pi/package.json`, `web/package.json`), a built binary, a `HOME` and a `CPD_HOME`, and
    /// runner rules for the built binary's verdict and config migration, the installed binary's follow-ups, git,
    /// the tagger, pnpm, plutil and node. No Electron, no tmux.
    fn new() -> Rig {
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
            macos: false,
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

    /// Runs the install as `cpd install --no-follow-up` does: (exit, stderr).
    fn run_no_follow_up(&self) -> (i32, String) {
        let mut err = Vec::new();
        let exit = run_with_options(
            &self.fakes.ports,
            &self.install,
            InstallOptions { no_follow_up: true },
            &mut err,
        );
        (exit, String::from_utf8(err).unwrap())
    }

    fn installed(&self) -> PathBuf {
        self.install.home.join(".local/bin/cpd")
    }

    /// The args of every call of the installed `cpd`; the follow-ups are its only caller.
    fn installed_cpd_args(&self) -> Vec<Vec<String>> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| Path::new(&c.program) == self.installed())
            .map(|c| c.args)
            .collect()
    }
}

#[test]
fn install_no_follow_up_skips_follow_ups() {
    let rig = Rig::new();

    let (exit, stderr) = rig.run_no_follow_up();

    assert_eq!(exit, 0, "{stderr}");
    // The binary is written all the same: only the follow-ups are deferred.
    assert!(rig.installed().is_file(), "no binary was installed");
    // The follow-ups (`cpd config`, `cpd watch --check`) did not run on the new binary.
    assert_eq!(
        rig.installed_cpd_args(),
        Vec::<Vec<String>>::new(),
        "the follow-ups ran under --no-follow-up"
    );
}
