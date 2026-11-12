//! Red tests for retiring CPD.app, electron build, and legacy shell release tooling.
//!
//! Contract: pro-d567, docs/architecture/client.md, docs/commands.md.
//! CLI rejects `cpd desktop release` and `cpd install shell` subcommands,
//! and prevents any attempt to fetch legacy app-v* release assets.

mod retire_app_test {
    use crate::common::fx::Fx;
    use cpd_core::home::CpdHome;

    /// Tests `retire_app_test::desktop_release_command_removed`.
    ///
    /// Verifies CLI rejects `cpd desktop release`:
    /// `desktop` itself is removed / rejected, or `desktop release` is rejected with usage error,
    /// and no `gh release create app-v*` release command is run.
    #[test]
    fn desktop_release_command_removed() {
        let fx = Fx::with_ports();

        // `cpd desktop release` subcommand must be rejected with exit code 2 (usage)
        let run = fx.cpd(&["desktop", "release"]);
        assert_eq!(
            run.exit, 2,
            "cpd desktop release must be rejected with exit code 2 (usage), got: {:?}",
            run
        );
        let json = run.json();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"]["code"], "usage");

        // Verify no `gh release create app-v*` was attempted
        let gh_calls = fx.fakes.runner.calls();
        let app_v_release_attempted = gh_calls
            .iter()
            .any(|c| c.program == "gh" && c.args.iter().any(|a| a.contains("app-v")));
        assert!(
            !app_v_release_attempted,
            "cpd desktop release must not attempt to create legacy app-v* releases, but gh was called: {gh_calls:?}"
        );
    }

    /// Tests `retire_app_test::shell_install_removed`.
    ///
    /// Verifies CLI rejects `cpd install shell`, and cpd install never fetches legacy app-v* release assets.
    #[test]
    fn shell_install_removed() {
        let fx = Fx::with_ports();

        // 1. `cpd install shell` subcommand must be rejected as unexpected argument / usage error
        let run = fx.cpd(&["install", "shell"]);
        assert_eq!(
            run.exit, 2,
            "cpd install shell must be rejected with exit code 2 (usage), got: {:?}",
            run
        );
        let json = run.json();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"]["code"], "usage");

        // 2. Verification against core::install::shell::ensure_shell:
        // When ensure_shell runs on macOS, it must NOT execute `gh release download app-v*`.
        let mut err = Vec::new();
        let cpd_home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
        let install = cpd_core::install::Install {
            root: fx.env.root().join("checkout"),
            target_dir: fx.env.root().join("target"),
            preview_dir: fx.env.root().join("preview"),
            macos: true,
            home: fx.env.home().to_path_buf(),
            cpd_home,
            pid: 1234,
            lock_wait: cpd_core::install::DEFAULT_LOCK_WAIT,
        };
        cpd_core::install::shell::ensure_shell(&fx.fakes.ports, &install, &mut err);

        let gh_calls = fx.fakes.runner.calls();
        let app_v_download_attempted = gh_calls
            .iter()
            .any(|c| c.program == "gh" && c.args.iter().any(|a| a.contains("app-v")));
        assert!(
            !app_v_download_attempted,
            "ensure_shell must not attempt to download legacy app-v* assets (decision pro-d567), but gh was called: {gh_calls:?}"
        );
    }
}
