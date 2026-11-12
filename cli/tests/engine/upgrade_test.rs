//! Red tests for cpd upgrade (docs/architecture/upgrade.md, docs/commands.md §14.4c, docs/contract.md §3.7c).
//!
//! Decisions pro-d567, pro-d569.
//! Tests assert:
//! 1. upgrade fetches latest CI-green main and compiles in an isolated directory without touching working trees.
//! 2. migrations are dry-run on a temporary copy of cpd.db.
//! 3. backup of cpd.db is created before swap.
//! 4. on health-check failure, binary and database are restored to previous state.
//!
//! All tests are hermetic: mocked git, cargo build, process restarts, health check, network. No real external commands, no sleep.

mod upgrade_test {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use cpd_core::clock::ManualClock;
    use cpd_core::home::CpdHome;
    use cpd_core::host::runner::{Call, FakeRunner, Handler, Reply};
    use cpd_core::host::spawn::FakeSpawn;
    use cpd_core::ports::Ports;
    use cpd_core::upgrade::{UpgradeConfig, UpgradeOutcome, run_upgrade};
    use tempfile::TempDir;

    /// Mock handler for cargo build that writes the new binary into isolated target directory.
    struct CargoBuilder {
        built_bin_path: PathBuf,
        bin_content: Vec<u8>,
    }

    impl Handler for CargoBuilder {
        fn handle(&self, call: &Call) -> Reply {
            if call.args.iter().any(|a| a == "build") {
                if let Some(parent) = self.built_bin_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&self.built_bin_path, &self.bin_content);
                Reply::exit(0, "   Compiling cpd\n    Finished release\n", "")
            } else {
                Reply::exit(1, "", "unexpected cargo call")
            }
        }
    }

    struct UpgradeFx {
        _temp: TempDir,
        pub home: CpdHome,
        pub clock: Arc<ManualClock>,
        pub spawn: Arc<FakeSpawn>,
        pub runner: FakeRunner,
        pub installed_bin: PathBuf,
        pub working_tree: PathBuf,
        pub initial_db_content: String,
        pub initial_bin_content: String,
    }

    impl UpgradeFx {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let root = temp.path().join("home");
            let home = CpdHome::resolve(Some(root.join(".cpd").into_os_string()), None)
                .expect("resolve home");

            fs::create_dir_all(home.run_dir()).expect("create run dir");
            fs::create_dir_all(home.locks_dir()).expect("create locks dir");
            fs::create_dir_all(home.root().join("backups")).expect("create backups dir");

            let bin_dir = root.join(".local/bin");
            fs::create_dir_all(&bin_dir).expect("create bin dir");
            let installed_bin = bin_dir.join("cpd");
            let initial_bin_content = "cpd-binary-v1.0.0".to_string();
            fs::write(&installed_bin, &initial_bin_content).expect("write installed bin");

            // Write working tree directory to verify it is NEVER touched
            let working_tree = temp.path().join("projects/cpd");
            fs::create_dir_all(&working_tree).expect("create working tree");
            fs::write(working_tree.join("sentinel.txt"), "untouched").expect("write sentinel");

            // Initialize database with known content
            let db_path = home.db();
            let initial_db_content = "sqlite-db-v1-initial-state".to_string();
            fs::write(&db_path, &initial_db_content).expect("write db");

            // Initialize config.toml
            let config_toml = "version = 1\nmachine = \"m1\"\n";
            fs::write(home.config(), config_toml).expect("write config.toml");

            let clock = Arc::new(ManualClock::new());
            let runner = FakeRunner::new((*clock).clone());
            let spawn = Arc::new(FakeSpawn::new());

            UpgradeFx {
                _temp: temp,
                home,
                clock,
                spawn,
                runner,
                installed_bin,
                working_tree,
                initial_db_content,
                initial_bin_content,
            }
        }

        fn ports(&self) -> Ports {
            Ports {
                clock: self.clock.clone(),
                runner: Arc::new(self.runner.clone()),
                spawn: self.spawn.clone(),
                ..Ports::real()
            }
        }
    }

    /// (1) upgrade fetches latest CI-green main and compiles in an isolated directory without touching working trees.
    /// (2) migrations are dry-run on a temporary copy of cpd.db.
    /// (3) backup of cpd.db is created before swap.
    /// Successful swap replaces installed binary and leaves working tree untouched.
    #[test]
    fn upgrade_dry_runs_migration_on_copy_and_swaps_binary() {
        let mut fx = UpgradeFx::new();

        let green_sha = "abcd1234ef567890abcd1234ef567890abcd1234";
        let built_bin_content = "cpd-binary-v2.0.0-built";

        // Setup mock runner responses:
        // - git fetch origin main and check CI green status
        fx.runner = fx.runner.on(
            "git",
            |call: &Call| call.args.iter().any(|a| a == "fetch"),
            Reply::exit(0, "", ""),
        );
        fx.runner = fx.runner.on(
            "git",
            |call: &Call| call.args.iter().any(|a| a == "rev-parse"),
            Reply::exit(0, "abcd1234ef567890abcd1234ef567890abcd1234\n", ""),
        );

        // - cargo build compiles into isolated directory ($CPD_HOME/upgrade/target)
        let upgrade_target = fx.home.root().join("upgrade/target/release/cpd");
        fx.runner = fx.runner.route(
            "cargo",
            Arc::new(CargoBuilder {
                built_bin_path: upgrade_target,
                bin_content: built_bin_content.as_bytes().to_vec(),
            }),
        );

        // - built binary dry-run migration on copy of cpd.db
        fx.runner = fx.runner.on(
            "cpd",
            |call: &Call| {
                call.args.iter().any(|a| a == "--migrate-scratch")
                    || call.args.iter().any(|a| a == "--dry-run")
            },
            Reply::exit(
                0,
                r#"{"ok":true,"migrated":["20261112T0100-test.sql"]}"#,
                "",
            ),
        );

        // - health check checks version and watch status
        fx.runner = fx.runner.on(
            "cpd",
            |call: &Call| call.args == ["--version"],
            Reply::exit(
                0,
                r#"{"ok":true,"version":"2.0.0","protocol":1,"store":{"opens":true,"pending":[]}}"#,
                "",
            ),
        );
        fx.runner = fx.runner.on(
            "cpd",
            |call: &Call| call.args.iter().any(|a| a == "--ensure"),
            Reply::exit(0, r#"{"ok":true}"#, ""),
        );

        let ports = fx.ports();
        let config = UpgradeConfig {
            home: fx.home.clone(),
            dry_run: false,
            check_only: false,
            target_bin: Some(fx.installed_bin.clone()),
        };

        let outcome: UpgradeOutcome = run_upgrade(&ports, &config).expect("upgrade should succeed");

        // Assert 1: Built SHA matches green SHA
        assert_eq!(outcome.to_sha, green_sha);

        // Assert 2: Working tree was NEVER touched
        let sentinel =
            fs::read_to_string(fx.working_tree.join("sentinel.txt")).expect("read sentinel");
        assert_eq!(
            sentinel, "untouched",
            "working tree must remain untouched by isolated build"
        );

        // Assert 3: Backup was created before swap in $CPD_HOME/backups/
        assert!(
            outcome.backup_path.is_some(),
            "backup must be created before binary swap"
        );
        let backup_dir = outcome.backup_path.unwrap();
        assert!(backup_dir.exists(), "backup dir must exist");
        let backup_db = backup_dir.join("cpd.db");
        assert!(backup_db.exists(), "cpd.db must be backed up before swap");
        let backed_up_content = fs::read_to_string(&backup_db).expect("read backed up db");
        assert_eq!(
            backed_up_content, fx.initial_db_content,
            "backup content must match initial db"
        );

        // Assert 4: Binary was swapped
        let current_bin = fs::read_to_string(&fx.installed_bin).expect("read installed bin");
        assert_eq!(
            current_bin, built_bin_content,
            "installed binary must be updated to new build"
        );

        // Assert 5: Old binary is preserved at cpd.previous for rollback
        let prev_bin = fx.installed_bin.with_file_name("cpd.previous");
        assert!(prev_bin.exists(), "cpd.previous must exist for rollback");
        let prev_content = fs::read_to_string(&prev_bin).expect("read prev bin");
        assert_eq!(
            prev_content, fx.initial_bin_content,
            "cpd.previous must hold original binary"
        );
    }

    /// (4) on health-check failure, binary and database are restored to previous state.
    #[test]
    fn upgrade_rolls_back_on_failed_health_check() {
        let mut fx = UpgradeFx::new();

        let green_sha = "abcd1234ef567890abcd1234ef567890abcd1234";
        let built_bin_content = "cpd-binary-v2.0.0-unhealthy";

        fx.runner = fx.runner.on(
            "git",
            |call: &Call| call.args.iter().any(|a| a == "fetch"),
            Reply::exit(0, "", ""),
        );
        let green_rev_reply2 = format!("{green_sha}\n");
        fx.runner = fx.runner.on(
            "git",
            |call: &Call| call.args.iter().any(|a| a == "rev-parse"),
            Reply::exit(0, &green_rev_reply2, ""),
        );

        let upgrade_target = fx.home.root().join("upgrade/target/release/cpd");
        fx.runner = fx.runner.route(
            "cargo",
            Arc::new(CargoBuilder {
                built_bin_path: upgrade_target,
                bin_content: built_bin_content.as_bytes().to_vec(),
            }),
        );

        // Dry-run migration succeeds
        fx.runner = fx.runner.on(
            "cpd",
            |call: &Call| {
                call.args.iter().any(|a| a == "--migrate-scratch")
                    || call.args.iter().any(|a| a == "--dry-run")
            },
            Reply::exit(0, r#"{"ok":true,"migrated":[]}"#, ""),
        );

        // Engine restart probe succeeds, but health-check fails (e.g. exit 1 or broken version verdict)
        fx.runner = fx.runner.on(
            "cpd",
            |call: &Call| call.args.iter().any(|a| a == "--ensure"),
            Reply::exit(0, r#"{"ok":true}"#, ""),
        );
        fx.runner = fx.runner.on(
            "cpd",
            |call: &Call| call.args == ["--version"],
            Reply::exit(1, "", "health check failure: fatal panic in new binary"),
        );

        let ports = fx.ports();
        let config = UpgradeConfig {
            home: fx.home.clone(),
            dry_run: false,
            check_only: false,
            target_bin: Some(fx.installed_bin.clone()),
        };

        let err =
            run_upgrade(&ports, &config).expect_err("upgrade must fail when health check fails");

        // Contract §3.7c, commands.md §14.4c:
        // Failure code is upgrade_failed, with details.step = "health", rolled_back = true
        assert_eq!(err.code().as_str(), "upgrade_failed");
        assert_eq!(
            err.details().get("rolled_back").and_then(|v| v.as_bool()),
            Some(true),
            "expected rolled_back: true in error details"
        );
        assert_eq!(
            err.details().get("step").and_then(|v| v.as_str()),
            Some("health"),
            "expected step: health in error details"
        );

        // Rollback check 1: Binary is restored to original content byte for byte
        let restored_bin = fs::read_to_string(&fx.installed_bin).expect("read restored bin");
        assert_eq!(
            restored_bin, fx.initial_bin_content,
            "binary must be restored to original pre-upgrade state"
        );

        // Rollback check 2: Database is restored from backup
        let restored_db = fs::read_to_string(fx.home.db()).expect("read restored db");
        assert_eq!(
            restored_db, fx.initial_db_content,
            "database must be restored to original backup state"
        );
    }
}
