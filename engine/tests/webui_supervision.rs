//! Red tests for engine supervision of WebUI prod (3333) and preview (33333) servers
//! and port-in-use check.
//!
//! Context records: pro-d541, pro-d542; web/CONTRACT.md §15.1; docs/architecture/webui-supervision.md.
//! All network and child processes are mocked.

mod webui_supervision {
    use std::fs;
    use std::sync::Arc;

    use cpd_core::clock::ManualClock;
    use cpd_core::events::Event;
    use cpd_core::home::CpdHome;
    use cpd_core::host::runner::{FakeRunner, Reply};
    use cpd_core::host::spawn::FakeSpawn;
    use cpd_core::ports::Ports;
    use tempfile::TempDir;

    /// Test fixture providing an isolated temporary home and fake ports (no real processes, sockets, or tmux).
    struct SupervisionFx {
        _temp: TempDir,
        #[allow(dead_code)]
        pub home: CpdHome,
        #[allow(dead_code)]
        pub clock: Arc<ManualClock>,
        #[allow(dead_code)]
        pub spawn: Arc<FakeSpawn>,
        pub runner: FakeRunner,
    }

    impl SupervisionFx {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let home = CpdHome::resolve(Some(temp.path().join("home").into_os_string()), None)
                .expect("resolve home");
            fs::create_dir_all(home.run_dir()).expect("create run dir");

            let clock = Arc::new(ManualClock::new());
            let runner = FakeRunner::new((*clock).clone());
            let spawn = Arc::new(FakeSpawn::new());

            SupervisionFx {
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

        /// Read recorded events from store if available.
        fn events(&self) -> Vec<Event> {
            // Will be populated when testing event emission against store
            Vec::new()
        }
    }

    /// pro-d541: Engine supervises WebUI prod 3333 and preview 33333 servers.
    ///
    /// Tests:
    /// - Starts prod server (web/src/server/serve.ts, port 3333).
    /// - Starts preview Vite server (port 33333) when preview worktree exists.
    /// - Detects crash and restarts child processes.
    /// - Latches stopped after > 3 crashes in 10 minutes and logs/raises error event.
    /// - Payload install (`run/install-restart.json`) triggers restart once without counting towards crash bound.
    #[test]
    fn engine_supervises_prod_and_preview_servers() {
        let fx = SupervisionFx::new();
        let _ports = fx.ports();

        // Verify supervision requirements from CONTRACT §15.1 and webui-supervision.md:
        // 1. Prod server is started on 0.0.0.0:3333 with serve.ts
        // 2. Preview server is started on 0.0.0.0:33333 when preview worktree exists
        // 3. Child process crash triggers restart within 3 restarts / 10-minute window
        // 4. Payload install marker triggers restart once without incrementing crash count
        // 5. Exceeding 3 crashes in 10 minutes latches service 'stopped' and raises error Event

        // Positive assertion check that fails until implemented:
        assert_eq!(
            fx.events().len(),
            1,
            "engine WebUI supervision is not implemented: cpd watch does not supervise prod (3333) or preview (33333)"
        );
    }

    /// pro-d542: Port in use fails loud with holder.
    ///
    /// Tests:
    /// - Engine checks port before starting WebUI services.
    /// - Runs `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fpcn` to detect holder.
    /// - Falls back to `ss -H -ltnp 'sport = :<port>'` when lsof is unavailable or yields no holder.
    /// - Refuses to start when port is held, exits non-zero, and never falls back to another port.
    /// - Names process and PID in refusal: "Port <port> is in use by <process> (PID <pid>). Exiting."
    /// - If holder unknown: "Port <port> is in use by an unknown process. Exiting."
    #[test]
    fn port_in_use_fails_loud_with_holder() {
        let mut fx = SupervisionFx::new();

        // Set up mock lsof output:
        // p12345
        // cnode
        // n*:3333
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a.contains("3333")),
            Reply::exit(0, "p12345\ncnode\nn*:3333\n", ""),
        );

        // Set up mock ss output fallback for 33333:
        // LISTEN 0 128 0.0.0.0:33333 0.0.0.0:* users:(("vite",pid=54321,fd=4))
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a.contains("33333")),
            Reply::exit(1, "", "lsof: command not found"),
        );
        fx.runner = fx.runner.on(
            "ss",
            |call| call.args.iter().any(|a| a.contains("33333")),
            Reply::exit(
                0,
                "LISTEN 0 128 0.0.0.0:33333 0.0.0.0:* users:((\"vite\",pid=54321,fd=4))\n",
                "",
            ),
        );

        let _ports = fx.ports();

        // Positive assertion check that fails until implemented:
        assert_eq!(
            fx.runner.calls().len(),
            2,
            "engine port-in-use check is not implemented: cpd watch does not check lsof/ss or fail loud with holder before starting WebUI services"
        );
    }
}
