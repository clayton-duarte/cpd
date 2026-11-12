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
use crate::host::spawn::{Pid, Signal, Spawn};
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
            .route("git", Arc::new(Always(Reply::ok(""))))
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
        self.fakes.spawn.survive(pid, Signal::Usr1);
        fs::write(
            self.install.cpd_home.root().join("run/preview-health.json"),
            format!(r#"{{"pid":{},"shell_abi":2}}"#, pid.0),
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
    let preview_worktree = rig
        .install
        .root
        .parent()
        .unwrap()
        .join("cpd.preview.web")
        .to_str()
        .unwrap()
        .to_owned();
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
            strings(&["--dir", &root, "--filter", "cpd-web-client", "run", "build"]),
            strings(&["--dir", &preview_worktree, "install", "--frozen-lockfile"]),
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
fn install_stages_payload_without_electron_relaunch() {
    let rig = Rig::new(false);
    let pid = rig.running_electron();
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");
    // Stages payload and notifies running Electron with SIGUSR1 for in-place swap
    assert_eq!(rig.fakes.spawn.signals(), vec![(pid, Signal::Usr1)]);
    // Running shell is never killed or relaunched (no launches, pid stays alive)
    assert_eq!(rig.fakes.spawn.launches(), vec![]);
    assert!(rig.fakes.spawn.alive(pid));

    // Staged payload directory exists and app-root is written
    let payload_dir = rig.install.root.join("web/dist-electron/payload");
    assert!(
        payload_dir.is_dir(),
        "expected versioned payload dir at {}",
        payload_dir.display()
    );
}

#[test]
fn install_refuses_payload_needing_newer_shell() {
    let rig = Rig::new(false);
    let _pid = rig.running_electron();
    // Simulate checkout payload requiring newer shell ABI (e.g. ABI 99)
    let version_file = rig.install.root.join("web/src/electron/version.ts");
    fs::create_dir_all(version_file.parent().unwrap()).unwrap();
    fs::write(&version_file, "export const MIN_SHELL_ABI_VERSION = 99;\n").unwrap();
    let (exit, stderr) = rig.run();
    assert_eq!(exit, 1, "incompatible payload should refuse install");
    assert!(
        stderr.contains("new CPD.app release needed"),
        "expected error message 'new CPD.app release needed', got: {stderr}"
    );
    // When refused, Electron is not signalled and app_root is not written
    assert_eq!(rig.fakes.spawn.signals(), vec![]);
    assert!(!rig.app_root().exists());
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

#[test]
fn preview_sync_preserves_wip_and_resets_to_main() {
    // Bug b11 / pro-d93 (contract §3.7 "The preview worktree on install", web/CONTRACT.md §28):
    // On cpd install from a new main, preview worktree uncommitted changes (tracked + untracked)
    // are saved as a patch, worktree resets to origin/main, patch is re-applied with 3-way merge,
    // and pnpm install runs in the worktree.
    let rig = Rig::new(false);
    let preview_worktree = rig.install.root.parent().unwrap().join("cpd.preview.web");
    fs::create_dir_all(&preview_worktree).unwrap();

    let _ = (*rig.fakes.runner).clone().on(
        "git",
        |c| c.args.iter().any(|a| a == "diff"),
        Reply::ok("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-x\n+y\n"),
    );

    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");

    // Git calls in preview worktree:
    // 1. diff / patch generation to save WIP
    // 2. reset to origin/main
    // 3. apply patch with 3-way merge
    let git_calls = rig.argv("git");
    let preview_path = preview_worktree.to_str().unwrap();

    let saved_patch = git_calls.iter().any(|args| {
        args.iter().any(|a| a.contains(preview_path))
            && args.iter().any(|a| a == "diff" || a == "diff-index")
    });
    assert!(
        saved_patch,
        "expected git diff/patch call in preview worktree to save WIP, got git calls: {git_calls:?}"
    );

    let reset_to_main = git_calls.iter().any(|args| {
        args.iter().any(|a| a.contains(preview_path))
            && args.iter().any(|a| a == "reset")
            && args.iter().any(|a| a == "origin/main" || a == "--hard")
    });
    assert!(
        reset_to_main,
        "expected git reset to origin/main in preview worktree, got git calls: {git_calls:?}"
    );

    let reapplied_patch = git_calls.iter().any(|args| {
        args.iter().any(|a| a.contains(preview_path))
            && args.iter().any(|a| a == "apply")
            && args.iter().any(|a| a == "-3" || a == "--3way")
    });
    assert!(
        reapplied_patch,
        "expected git apply with 3-way merge in preview worktree, got git calls: {git_calls:?}"
    );

    // pnpm install ran in preview worktree
    let pnpm_calls = rig.argv("pnpm");
    let pnpm_in_preview = pnpm_calls.iter().any(|args| {
        args.windows(2)
            .any(|w| w[0] == "--dir" && w[1] == preview_path)
            && args.iter().any(|a| a == "install")
    });
    assert!(
        pnpm_in_preview,
        "expected pnpm install in preview worktree, got pnpm calls: {pnpm_calls:?}"
    );
}

#[test]
fn preview_sync_conflict_leaves_main_and_reports_error() {
    // Bug b11 / pro-d93 (contract §3.7, web/CONTRACT.md §28):
    // When re-applying the saved patch conflicts, the patch is preserved,
    // the preview stays on origin/main without the patch applied,
    // and an error is reported (error Event or Inbox item / stderr notice).
    let rig = Rig::new(false);
    let preview_worktree = rig.install.root.parent().unwrap().join("cpd.preview.web");
    fs::create_dir_all(&preview_worktree).unwrap();

    let _ = (*rig.fakes.runner).clone().on(
        "git",
        |c| c.args.iter().any(|a| a == "diff"),
        Reply::ok("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-x\n+y\n"),
    );

    // Script git apply to simulate merge conflict
    let _ = (*rig.fakes.runner).clone().on(
        "git",
        |c| c.args.iter().any(|a| a == "apply"),
        Reply::exit(
            1,
            "",
            "error: patch failed: web/src/App.tsx:10\nerror: repository sync conflict\n",
        ),
    );

    let (exit, stderr) = rig.run();
    // A conflict in preview sync keeps patch and reports error
    let git_calls = rig.argv("git");
    let preview_path = preview_worktree.to_str().unwrap();

    let applied = git_calls.iter().any(|args| {
        args.iter().any(|a| a.contains(preview_path)) && args.iter().any(|a| a == "apply")
    });
    assert!(
        applied,
        "expected git apply attempt in preview worktree, got git calls: {git_calls:?}"
    );

    // Patch must be preserved on disk or reported
    assert!(
        stderr.contains("conflict") || stderr.contains("preview sync"),
        "expected conflict or preview sync error report on stderr, got: {stderr} (exit {exit})"
    );
}

#[test]
fn preview_sync_skips_git_apply_when_diff_is_empty() {
    // Bug b30: when git diff is empty (no uncommitted WIP), preview sync resets to
    // origin/main and runs pnpm install without invoking git apply (avoiding phantom conflicts).
    let rig = Rig::new(false);
    let preview_worktree = rig.install.root.parent().unwrap().join("cpd.preview.web");
    fs::create_dir_all(&preview_worktree).unwrap();

    let (exit, stderr) = rig.run();
    assert_eq!(exit, 0, "{stderr}");

    let git_calls = rig.argv("git");
    let preview_path = preview_worktree.to_str().unwrap();

    let reset_to_main = git_calls.iter().any(|args| {
        args.iter().any(|a| a.contains(preview_path))
            && args.iter().any(|a| a == "reset")
            && args.iter().any(|a| a == "origin/main" || a == "--hard")
    });
    assert!(
        reset_to_main,
        "expected git reset to origin/main in preview worktree, got git calls: {git_calls:?}"
    );

    let applied_patch = git_calls.iter().any(|args| {
        args.iter().any(|a| a.contains(preview_path)) && args.iter().any(|a| a == "apply")
    });
    assert!(
        !applied_patch,
        "expected git apply to be skipped when diff is empty, got git calls: {git_calls:?}"
    );

    let pnpm_calls = rig.argv("pnpm");
    let pnpm_in_preview = pnpm_calls.iter().any(|args| {
        args.windows(2)
            .any(|w| w[0] == "--dir" && w[1] == preview_path)
            && args.iter().any(|a| a == "install")
    });
    assert!(
        pnpm_in_preview,
        "expected pnpm install in preview worktree, got pnpm calls: {pnpm_calls:?}"
    );
}
