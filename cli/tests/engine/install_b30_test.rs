//! Red tests for Bug b30: Fix cpd install: preview sync patch, retire TUI respawn, verify engine.
//!
//! Context / contract:
//! - contract §3.7: step 2 (views respawn) retired (pro-d567, #1070, #1092).
//! - commands §14.4: follow-up list drops cpd tui --respawn.
//! - contract §3.7 step 3: restart verifies lock hold and pane liveness and names the failure.
//! - commands §14.4 step 7b & contract §3.7: preview sync skips git apply and reports no conflict on empty diff.

#[allow(clippy::module_inception)]
mod install_b30_test {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::common::fx::Fx;
    use cpd_core::fakes::git::Repo;
    use cpd_core::home::CpdHome;
    use cpd_core::host::runner::{Call, Reply};

    const OPENS: &str = r#"{"opens":true,"pending":[]}"#;

    struct Rig {
        fx: Fx,
        install: cpd_core::install::Install,
        preview_worktree: PathBuf,
    }

    impl Rig {
        fn new() -> Self {
            let fx = Fx::with_ports();
            let root = fx.env.root().to_path_buf();

            let origin = Repo::init(&fx.universe, &root.join("repos/install-origin"));
            for script in ["install-cpd.sh", "cargo.sh", "tag-version.sh"] {
                origin.write(&format!("scripts/{script}"), &format!("{script}\n"));
            }
            for file in [
                "package.json",
                "extension.ts",
                "lib/cpd.ts",
                "lib/adapter.ts",
                "test/cpd.test.ts",
            ] {
                origin.write(&format!("pi/{file}"), &format!("{file}\n"));
            }
            origin.write("web/package.json", r#"{"name":"web"}"#);
            origin.write(
                "core/src/store/migrations/0001-initial.sql",
                "0001-initial.sql\n",
            );
            origin.commit("install files");

            let clone = root.join("repos/install-clone");
            fx.git(
                &root,
                &[
                    "clone",
                    "-q",
                    origin.dir().to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );

            let preview_worktree = root.join("repos/cpd.preview.web");
            fs::create_dir_all(&preview_worktree).unwrap();

            let target_dir = root.join("target");
            fs::create_dir_all(target_dir.join("release")).unwrap();
            let (built, installed, tag) = (
                target_dir.join("release/cpd"),
                fx.env.home().join(".local/bin/cpd"),
                clone.join("scripts/tag-version.sh"),
            );
            fs::write(&built, "built cpd\n").unwrap();

            let socket = fx.env.tmux_socket();
            let version = format!(r#"{{"ok":true,"version":"0.0.0","store":{OPENS}}}"#);
            let config = format!(
                r#"{{"ok":true,"config":{{"tmux":{{"socket":"{socket}","cockpit_session":"cpd","work_session":"cpd-work"}}}}}}"#
            );

            let at = |path: PathBuf, args: &'static [&'static str]| {
                move |c: &Call| Path::new(&c.program) == path && (args.is_empty() || c.args == args)
            };

            let _ = (*fx.fakes.runner)
                .clone()
                .on("pnpm", |_| true, Reply::ok(""))
                .on("git", |c| c.args.iter().any(|a| a == "diff"), Reply::ok(""))
                .on(
                    "cpd",
                    at(built.clone(), &["--version"]),
                    Reply::ok(&version),
                )
                .on(
                    "cpd",
                    at(built.clone(), &["config", "--migrate"]),
                    Reply::ok(r#"{"ok":true,"migrated":[]}"#),
                )
                .on(
                    "cpd",
                    at(built, &["watch", "--check", "--migrate-scratch"]),
                    Reply::ok(r#"{"ok":true}"#),
                )
                .on(
                    "cpd",
                    at(installed.clone(), &["config"]),
                    Reply::ok(&config),
                )
                .on(
                    "cpd",
                    at(installed.clone(), &["watch", "--check"]),
                    Reply::ok(r#"{"ok":true}"#),
                )
                .on(
                    "cpd",
                    at(installed, &["watch", "--mark-restart"]),
                    Reply::ok(r#"{"ok":true}"#),
                )
                .on("tag-version.sh", at(tag, &[]), Reply::ok(""));

            let install = cpd_core::install::Install {
                root: clone,
                target_dir,
                home: fx.env.home().to_path_buf(),
                cpd_home: CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(),
                pid: 1234,
                preview_dir: fx.env.home().join(".cache/cpd-preview"),
                macos: false,
                lock_wait: cpd_core::install::DEFAULT_LOCK_WAIT,
            };

            Self {
                fx,
                install,
                preview_worktree,
            }
        }

        fn run(&self) -> (i32, String) {
            let mut stderr = Vec::new();
            let exit = cpd_core::install::run(&self.fx.fakes.ports, &self.install, &mut stderr);
            (exit, String::from_utf8_lossy(&stderr).into_owned())
        }
    }

    /// Verifies preview sync does not attempt git apply when diff is empty and does not report
    /// non-existent patch file conflict.
    #[test]
    fn preview_sync_skips_patch_apply_when_diff_is_empty() {
        let rig = Rig::new();
        let preview_path = rig.preview_worktree.to_str().unwrap();

        // Simulate git diff HEAD returning empty output (no uncommitted diffs)
        let _ = (*rig.fx.fakes.runner).clone().on(
            "git",
            |c| c.args.iter().any(|a| a == "diff"),
            Reply::ok(""),
        );

        let mut err = Vec::new();
        cpd_core::install::worktree::ensure_preview_worktree(
            &rig.fx.fakes.ports,
            &rig.install,
            &mut err,
        );

        let stderr = String::from_utf8_lossy(&err);
        let calls = rig.fx.fakes.runner.calls();

        let applied = calls.iter().any(|c| {
            c.program == "git"
                && c.args.iter().any(|a| a == "apply")
                && c.args.iter().any(|a| a.contains(preview_path))
        });

        assert!(
            !applied,
            "preview sync must skip git apply when diff is empty, but git apply was called: {calls:?}"
        );
        assert!(
            !stderr.contains("conflict"),
            "preview sync must not report non-existent patch file conflict on empty diff, got stderr: {stderr}"
        );
    }

    /// Verifies that install follow-up does not execute 'cpd tui --respawn'.
    #[test]
    fn install_followup_does_not_call_tui_respawn() {
        let rig = Rig::new();
        let installed = rig.fx.env.home().join(".local/bin/cpd");
        let at = |path: PathBuf, args: &'static [&'static str]| {
            move |c: &Call| Path::new(&c.program) == path && (args.is_empty() || c.args == args)
        };
        let _ = (*rig.fx.fakes.runner).clone().on(
            "cpd",
            at(installed, &["watch", "--ensure"]),
            Reply::ok(r#"{"ok":true}"#),
        );

        // Setup cockpit session and main window so follow-up would consider respawning views
        rig.fx.tmux(&["new-session", "-d", "-s", "cpd"]);
        rig.fx.tmux(&["new-window", "-t", "cpd:", "-n", "main"]);

        let (_exit, _stderr) = rig.run();

        let calls = rig.fx.fakes.runner.calls();
        let respawn_called = calls.iter().any(|c| {
            c.args
                .windows(2)
                .any(|w| w[0] == "tui" && w[1] == "--respawn")
                || c.args.iter().any(|a| a == "--respawn")
        });

        assert!(
            !respawn_called,
            "install follow-up must not call cpd tui --respawn, but it was called: {calls:?}"
        );
    }

    /// Verifies that install engine restart verifies engine is alive and fails loud with detailed
    /// diagnostic if engine fails to come up or has no live pane.
    #[test]
    fn install_engine_restart_fails_loud_when_engine_does_not_come_up() {
        let rig = Rig::new();

        // Setup work session and watch window
        rig.fx.tmux(&["new-session", "-d", "-s", "cpd-work"]);
        rig.fx
            .tmux(&["new-window", "-t", "cpd-work:", "-n", "watch"]);

        let lock_file = rig.install.cpd_home.watch_lock();
        fs::create_dir_all(lock_file.parent().unwrap()).unwrap();

        // Simulate watch --ensure acquiring the lock (e.g. by a process outside tmux)
        let lock_file_for_ensure = lock_file.clone();
        let holder = std::sync::Arc::new(std::sync::Mutex::new(None));
        let installed = rig.fx.env.home().join(".local/bin/cpd");
        struct EnsureHandler {
            lock: PathBuf,
            holder: std::sync::Arc<std::sync::Mutex<Option<fs::File>>>,
        }
        impl cpd_core::host::runner::Handler for EnsureHandler {
            fn handle(&self, _call: &Call) -> Reply {
                let file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(&self.lock)
                    .unwrap();
                cpd_core::lock::try_lock(&file).unwrap();
                *self.holder.lock().unwrap() = Some(file);
                Reply::ok(r#"{"ok":true}"#)
            }
        }
        let ensure_handler = std::sync::Arc::new(EnsureHandler {
            lock: lock_file_for_ensure,
            holder,
        });

        // Register a route for cpd that handles watch --ensure by taking the lock
        let original_routes = (*rig.fx.fakes.runner).clone();
        let _ = original_routes.route(
            "cpd",
            std::sync::Arc::new({
                struct EnsureRoute {
                    installed: PathBuf,
                    ensure: std::sync::Arc<EnsureHandler>,
                }
                impl cpd_core::host::runner::Handler for EnsureRoute {
                    fn handle(&self, call: &Call) -> Reply {
                        if Path::new(&call.program) == self.installed
                            && call.args == ["watch", "--ensure"]
                        {
                            self.ensure.handle(call)
                        } else {
                            Reply::ok(r#"{"ok":true}"#)
                        }
                    }
                }
                EnsureRoute {
                    installed,
                    ensure: ensure_handler,
                }
            }),
        );

        let (exit, stderr) = rig.run();

        assert_eq!(
            exit, 1,
            "install must fail loud when engine window has no live pane, got exit {exit}, stderr: {stderr}"
        );
        assert!(
            stderr.contains("has no live pane"),
            "install restart must fail loud with detailed diagnostic naming engine issue ('has no live pane'), got stderr: {stderr}"
        );
    }
}
