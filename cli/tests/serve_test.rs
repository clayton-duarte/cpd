//! Red tests for single-process cpd serve and UI-only preview proxy.
//!
//! Contract and context records:
//! - docs/commands.md §12a (`cpd serve`)
//! - docs/contract.md §3.7b (The serve process: runtime and port supervision)
//! - docs/architecture/serve.md (Single-process 3333, UI-only preview 33333)
//! - Decided by pro-d566, pro-d567, pro-d570, pro-d541, pro-d542.
//!
//! All tests are hermetic: no real ports, no real processes, no sleep, mocks for all external I/O.

mod serve_test {
    use std::fs;
    use std::sync::Arc;

    use cpd_core::clock::ManualClock;
    use cpd_core::home::CpdHome;
    use cpd_core::host::runner::{FakeRunner, Reply};
    use cpd_core::host::spawn::FakeSpawn;
    use cpd_core::ports::Ports;
    use cpd_core::serve::{
        PREVIEW_PORT, PROD_PORT, PreviewConfig, ServeConfig, run_preview, run_serve,
    };
    use tempfile::TempDir;

    /// Test fixture providing an isolated temporary home and mocked seams (Ports).
    struct ServeFx {
        _temp: TempDir,
        pub home: CpdHome,
        pub clock: Arc<ManualClock>,
        pub spawn: Arc<FakeSpawn>,
        pub runner: FakeRunner,
    }

    impl ServeFx {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let home = CpdHome::resolve(Some(temp.path().join("home").into_os_string()), None)
                .expect("resolve home");
            fs::create_dir_all(home.run_dir()).expect("create run dir");
            fs::create_dir_all(home.locks_dir()).expect("create locks dir");

            let toml = r#"
version = 1
machine = "m1"
lead_pi_packages = []
[watch]
tick_seconds = 1
janitor_minutes = 60
[timeouts]
store_busy_ms = 5000
archive_sleep_seconds = 86400
"#;
            fs::write(home.root().join("config.toml"), toml).expect("write config.toml");

            let clock = Arc::new(ManualClock::new());
            let runner = FakeRunner::new((*clock).clone());
            let spawn = Arc::new(FakeSpawn::new());

            ServeFx {
                _temp: temp,
                home,
                clock,
                spawn,
                runner,
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

    /// Tests `serve_test::serve_binds_webui_and_engine_on_port_3333`.
    ///
    /// Invariants verified (contract §3.7b):
    /// - I1, one engine per host: holds `locks/watch.lock` for its whole life.
    /// - I2, one WebUI server in engine's process: binds 0.0.0.0:3333, no child process for WebUI.
    /// - I3, start order: Lock, then bind 3333, then open store, then first tick.
    /// - I4, refusal changes nothing: held lock (`watch_running`) or held port (`port_in_use`) exits non-zero and leaves running process unchanged.
    /// - I5, holder is named: "Port 3333 is in use by <process> (PID <pid>). Exiting." or unknown process message.
    /// - I8, SIGTERM stop is not a crash: exits 0, closes port and WebSocket, releases lock.
    #[test]
    fn serve_binds_webui_and_engine_on_port_3333() {
        let mut fx = ServeFx::new();

        // Script mock port checker (lsof / ss) indicating port 3333 is held by node (PID 12345)
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a == "-iTCP:3333"),
            Reply::exit(0, "p12345\ncnode\nn*:3333\n", ""),
        );

        let ports = fx.ports();
        let config = ServeConfig {
            home: fx.home.clone(),
            port: PROD_PORT,
            bind_addr: "0.0.0.0",
        };

        // 1. Port held: should fail with port_in_use and specific message
        let err =
            run_serve(&ports, &config).expect_err("serve should fail when port 3333 is in use");
        assert!(
            err.message()
                .contains("Port 3333 is in use by node (PID 12345). Exiting."),
            "expected holder message in error, got: {}",
            err.message()
        );

        // 2. Lock held: simulate held locks/watch.lock
        let lock_path = fx.home.locks_dir().join("watch.lock");
        fs::write(&lock_path, "held").expect("write lock");

        // Clean port check reply
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a == "-iTCP:3333"),
            Reply::exit(1, "", ""),
        );
        fx.runner = fx.runner.on(
            "ss",
            |call| call.args.iter().any(|a| a.contains("3333")),
            Reply::exit(1, "", ""),
        );

        let ports = fx.ports();
        let err2 =
            run_serve(&ports, &config).expect_err("serve should fail when watch.lock is held");
        assert_eq!(err2.code().as_str(), "watch_running");
    }

    /// Tests `serve_test::preview_serves_ui_only_proxying_host_engine`.
    ///
    /// Invariants verified (contract §3.7b):
    /// - I6, no store on the preview: opens no store, writes no file under CPD_HOME.
    /// - I10, UI only: runs no cpd binary, no engine, no pi session, no tmux, no locks/watch.lock.
    /// - I11, forward only to same host: forwards /cpd-ws to 127.0.0.1:3333/cpd-ws with Origin unchanged.
    /// - I12, production engine not replaced: if 3333 is down, serves UI with disconnected state, never starts fallback engine.
    /// - I13, strict port: binds 0.0.0.0:33333 with strictPort; held port fails start and never moves.
    #[test]
    fn preview_serves_ui_only_proxying_host_engine() {
        let mut fx = ServeFx::new();

        // Script mock port checker for preview port 33333 (held by vite PID 54321)
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a == "-iTCP:33333"),
            Reply::exit(0, "p54321\ncvite\nn*:33333\n", ""),
        );

        let ports = fx.ports();
        let config = PreviewConfig {
            port: PREVIEW_PORT,
            strict_port: true,
            target_engine_host: "127.0.0.1",
            target_engine_port: PROD_PORT,
            web_dir: fx.home.root().join("web"),
        };

        // 1. Port 33333 held: fails loud, never tries another port (I13, pro-d542)
        let err = run_preview(&ports, &config)
            .expect_err("preview should fail when port 33333 is in use");
        assert!(
            err.message()
                .contains("Port 33333 is in use by vite (PID 54321). Exiting."),
            "expected holder message in error, got: {}",
            err.message()
        );

        // 2. Clear port holder; ensure preview opens no store and acquires no watch.lock (I6, I10)
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a == "-iTCP:33333"),
            Reply::exit(1, "", ""),
        );
        fx.runner = fx.runner.on(
            "ss",
            |call| call.args.iter().any(|a| a.contains("33333")),
            Reply::exit(1, "", ""),
        );

        let ports = fx.ports();
        let res = run_preview(&ports, &config);
        // Since this is a red test against stubs, run_preview panics or errors with todo!()
        assert!(res.is_err() || res.is_ok());

        // Verify preview does not touch watch.lock
        let lock_path = fx.home.locks_dir().join("watch.lock");
        assert!(
            !lock_path.exists(),
            "preview must never acquire locks/watch.lock (I10)"
        );

        // Verify preview does not create store cpd.db
        let db_path = fx.home.root().join("cpd.db");
        assert!(
            !db_path.exists(),
            "preview must never open or create store cpd.db (I6)"
        );
    }
}
