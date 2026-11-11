//! Red tests for cpd install preview worktree creation and macOS CPD.app install
//! (Plan `ports-everywhere`, pro-d545, pro-d553, web/CONTRACT.md §30).
//!
//! - `creates_preview_worktree_if_missing`: cpd install creates ~/projects/cpd.preview.web if missing
//!   (`git worktree add -B preview/web <path> origin/main`), runs `pnpm install --frozen-lockfile` in it,
//!   and builds prod browser assets (`pnpm --dir <root> --filter cpd-web-client run build`).
//! - `installs_cpd_app_shell_on_macos_only`: downloads/unpacks shell release `app-v0.1.0` on macOS
//!   to /Applications/CPD.app, while skipping on Linux.
//!
//! External dependencies (git, network, fs/runner) are mocked.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::install::{self, Install};
use cpd_core::ports::Fakes;

const NO_VERDICT: &str = r#"{"ok":true,"version":"0.0.0","protocol":1,"store":null}"#;
const MIGRATED: &str = r#"{"ok":true,"migrated":[]}"#;
const CONFIG: &str =
    r#"{"ok":true,"config":{"tmux":{"socket":"","cockpit_session":"","work_session":""}}}"#;

struct Always(Reply);

impl Handler for Always {
    fn handle(&self, _: &Call) -> Reply {
        self.0.clone()
    }
}

fn named(program: &str, call: &Call) -> bool {
    Path::new(&call.program)
        .file_name()
        .is_some_and(|n| n == program)
}

struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    install: Install,
    expected_worktree: PathBuf,
}

impl Rig {
    fn new(macos: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let home = base.join("home");
        let cpd_home = base.join("cpd-home");
        let root = base.join("projects/cpd");
        let expected_worktree = base.join("projects/cpd.preview.web");
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
            .route("node", Arc::new(Always(Reply::ok(""))))
            .route("gh", Arc::new(Always(Reply::ok(""))))
            .route("xattr", Arc::new(Always(Reply::ok(""))));

        let install = Install {
            root,
            target_dir,
            preview_dir: base.join("preview"),
            macos,
            home,
            cpd_home: CpdHome::resolve(Some(OsString::from(cpd_home)), None).unwrap(),
            pid: 4242,
            lock_wait: install::DEFAULT_LOCK_WAIT,
        };

        Rig {
            _tmp: tmp,
            fakes,
            install,
            expected_worktree,
        }
    }

    fn run(&self) -> (i32, String) {
        let mut err = Vec::new();
        let exit = install::run(&self.fakes.ports, &self.install, &mut err);
        (exit, String::from_utf8(err).unwrap())
    }

    fn calls_of(&self, program: &str) -> Vec<Call> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| named(program, c))
            .collect()
    }
}

mod install_preview_worktree {
    use super::*;

    #[test]
    fn creates_preview_worktree_if_missing() {
        let rig = Rig::new(false);
        assert!(
            !rig.expected_worktree.exists(),
            "preview worktree must initially be missing"
        );

        let (exit, stderr) = rig.run();
        assert_eq!(exit, 0, "{stderr}");

        // 1. Verify git worktree add was called to create the preview worktree
        let git_calls = rig.calls_of("git");
        let expected_wt_str = rig.expected_worktree.to_str().unwrap();
        let has_worktree_add = git_calls.iter().any(|c| {
            c.args.windows(5).any(|w| {
                w[0] == "worktree"
                    && w[1] == "add"
                    && w[2] == "-B"
                    && w[3] == "preview/web"
                    && w[4] == expected_wt_str
            }) || (c.args.contains(&"worktree".to_string())
                && c.args.contains(&"add".to_string())
                && c.args.contains(&"preview/web".to_string())
                && c.args.contains(&expected_wt_str.to_string()))
        });
        assert!(
            has_worktree_add,
            "expected git worktree add -B preview/web {expected_wt_str} origin/main call, got: {git_calls:?}"
        );

        // 2. Verify pnpm install was run in the preview worktree
        let pnpm_calls = rig.calls_of("pnpm");
        let has_worktree_pnpm_install = pnpm_calls.iter().any(|c| {
            (c.cwd.as_deref() == Some(rig.expected_worktree.as_path())
                || c.args
                    .windows(2)
                    .any(|w| w[0] == "--dir" && w[1] == expected_wt_str))
                && c.args.contains(&"install".to_string())
        });
        assert!(
            has_worktree_pnpm_install,
            "expected pnpm install in preview worktree {expected_wt_str}, got pnpm calls: {pnpm_calls:?}"
        );

        // 3. Verify prod browser static assets build: pnpm --filter cpd-web-client run build (or vite build)
        let root_str = rig.install.root.to_str().unwrap();
        let has_prod_build = pnpm_calls.iter().any(|c| {
            c.args.contains(&"build".to_string())
                && !c.args.contains(&"build:electron".to_string())
                && (c.args.contains(&"cpd-web-client".to_string())
                    || c.cwd.as_deref() == Some(rig.install.root.as_path())
                    || c.args
                        .windows(2)
                        .any(|w| w[0] == "--dir" && w[1] == root_str))
        });
        assert!(
            has_prod_build,
            "expected prod browser build (pnpm --filter cpd-web-client run build), got pnpm calls: {pnpm_calls:?}"
        );
    }

    #[test]
    fn installs_cpd_app_shell_on_macos_only() {
        // Part 1: On macOS (macos = true), install downloads/unpacks shell app-v0.1.0
        let mac_rig = Rig::new(true);
        let (mac_exit, mac_stderr) = mac_rig.run();
        assert_eq!(mac_exit, 0, "{mac_stderr}");

        let mac_gh_calls = mac_rig.calls_of("gh");
        let has_shell_download = mac_gh_calls.iter().any(|c| {
            c.args.contains(&"release".to_string())
                && c.args.contains(&"download".to_string())
                && (c.args.contains(&"app-v0.1.0".to_string())
                    || c.args
                        .windows(2)
                        .any(|w| w[0] == "--pattern" && w[1].contains("app")))
        });
        assert!(
            has_shell_download,
            "expected gh release download for app-v0.1.0 on macOS, got gh calls: {mac_gh_calls:?}"
        );

        // Part 2: On Linux (macos = false), install skips shell download
        let linux_rig = Rig::new(false);
        let (linux_exit, linux_stderr) = linux_rig.run();
        assert_eq!(linux_exit, 0, "{linux_stderr}");

        let linux_gh_calls = linux_rig.calls_of("gh");
        let has_linux_download = linux_gh_calls.iter().any(|c| {
            c.args.contains(&"release".to_string()) && c.args.contains(&"download".to_string())
        });
        assert!(
            !has_linux_download,
            "shell install must be skipped on Linux, but got gh calls: {linux_gh_calls:?}"
        );
    }
}
