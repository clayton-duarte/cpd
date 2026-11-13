//! Tests for engine supervision of WebUI prod (3333) and preview (33333) servers
//! and port-in-use check.
//!
//! Context records: pro-d541, pro-d542; web/CONTRACT.md §15.1; docs/architecture/webui-supervision.md.
//! All network and child processes are mocked.

mod webui_supervision {
    use std::fs;
    use std::sync::Arc;

    use cpd_core::clock::ManualClock;
    use cpd_core::config::Config;
    use cpd_core::events::Event;
    use cpd_core::home::CpdHome;
    use cpd_core::host::runner::{FakeRunner, Reply};
    use cpd_core::host::spawn::{FakeSpawn, Spawn};
    use cpd_core::host::tmux::Tmux;
    use cpd_core::ports::Ports;
    use cpd_core::store::Store;
    use cpd_engine::watch::engine::Engine;
    use cpd_engine::watch::webui::{
        self, ServiceKind, detect_port_holder, format_port_in_use_message,
    };
    use tempfile::TempDir;

    /// Test fixture providing an isolated temporary home and fake ports (no real processes, sockets, or tmux).
    struct SupervisionFx {
        _temp: TempDir,
        pub home: CpdHome,
        pub clock: Arc<ManualClock>,
        pub spawn: Arc<FakeSpawn>,
        pub runner: FakeRunner,
    }

    impl SupervisionFx {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let home = CpdHome::resolve(Some(temp.path().join("home").into_os_string()), None)
                .expect("resolve home");
            fs::create_dir_all(home.run_dir()).expect("create run dir");

            // Write minimal valid config.toml
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

        fn engine(&self) -> Engine {
            let ports = self.ports();
            let tmux = Tmux::new("test");
            Engine::with_ports(self.home.clone(), tmux, &ports)
        }

        /// Read recorded events from store.
        fn events(&self) -> Vec<Event> {
            let config = Config::load(&self.home).expect("config");
            let mut store = Store::open(&self.home, u64::from(config.timeouts.store_busy_ms))
                .expect("open store");
            store
                .read(|conn| {
                    let mut stmt = conn.prepare(
                        "SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id ASC",
                    )?;
                    let rows = stmt.query_map([], |r| {
                        let data_str: String = r.get(9)?;
                        let data = serde_json::from_str(&data_str).unwrap_or(serde_json::Value::Null);
                        Ok(Event {
                            id: r.get(0)?,
                            at: r.get(1)?,
                            kind: r.get(2)?,
                            context: r.get(3)?,
                            plan: r.get(4)?,
                            job: r.get(5)?,
                            attempt: r.get(6)?,
                            session: r.get(7)?,
                            text: r.get(8)?,
                            data,
                        })
                    })?;
                    let mut res = Vec::new();
                    for row in rows {
                        res.push(row?);
                    }
                    Ok(res)
                })
                .unwrap_or_default()
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

        // Create serve.ts so prod WebUI is supervised
        let serve_ts = fx.home.root().join("web/src/server/serve.ts");
        fs::create_dir_all(serve_ts.parent().unwrap()).expect("create web server dir");
        fs::write(&serve_ts, "// mock server").expect("write serve.ts");

        // 1. Create preview worktree indicator so preview server is also supervised
        fs::create_dir_all(fx.home.root().join("projects").join("cpd.preview"))
            .expect("create preview dir");

        let mut engine = fx.engine();

        // Initial run: starts both prod (3333) and preview (33333)
        webui::run(&mut engine);

        assert!(
            engine.webui.services.contains_key(&ServiceKind::Prod),
            "prod server not supervised"
        );
        let prod_pid = engine.webui.services[&ServiceKind::Prod]
            .pid
            .expect("prod pid should be set");

        assert!(
            engine.webui.services.contains_key(&ServiceKind::Preview),
            "preview server not supervised"
        );
        let preview_pid = engine.webui.services[&ServiceKind::Preview]
            .pid
            .expect("preview pid should be set");

        assert!(fx.spawn.alive(prod_pid));
        assert!(fx.spawn.alive(preview_pid));

        // 2. Simulate prod crash (mark dead in fake spawn)
        fx.spawn.exit(prod_pid, 1);
        assert!(!fx.spawn.alive(prod_pid));

        // Engine tick detects crash and restarts prod
        webui::run(&mut engine);
        let prod_pid2 = engine.webui.services[&ServiceKind::Prod]
            .pid
            .expect("prod pid should be restarted");
        assert_ne!(prod_pid, prod_pid2);
        assert!(fx.spawn.alive(prod_pid2));

        // 3. Crash loop: crash 3 more times inside 10 minutes
        for _ in 0..3 {
            let cur_pid = engine.webui.services[&ServiceKind::Prod].pid.unwrap();
            fx.spawn.exit(cur_pid, 1);
            webui::run(&mut engine);
        }

        // Should latch stopped and raise an error event
        assert!(
            engine.webui.services[&ServiceKind::Prod].latched_stopped,
            "prod server should be latched stopped after > 3 crashes"
        );
        let events = fx.events();
        assert!(
            events
                .iter()
                .any(|e| e.kind == "error" && e.text.contains("crashed 3 times")),
            "error event should be raised when service latches stopped"
        );

        // 4. Payload install restart marker triggers restart once without counting as crash
        let marker = fx.home.run_dir().join("install-restart.json");
        fs::write(&marker, "{}").expect("write marker");

        // Give preview server a clean restart on payload install
        let prev_preview_pid = engine.webui.services[&ServiceKind::Preview].pid.unwrap();
        webui::run(&mut engine);
        let new_preview_pid = engine.webui.services[&ServiceKind::Preview].pid.unwrap();
        assert_ne!(prev_preview_pid, new_preview_pid);
        assert_eq!(
            engine.webui.services[&ServiceKind::Preview].restart_count,
            0,
            "install restart should not count towards crash count"
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
            |call| call.args.iter().any(|a| a == "-iTCP:33333"),
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
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a == "-iTCP:3333"),
            Reply::exit(0, "p12345\ncnode\nn*:3333\n", ""),
        );

        // Verify detection on port 3333 (via lsof)
        let holder3333 = detect_port_holder(&fx.runner, 3333).expect("detect 3333 holder");
        assert_eq!(holder3333.process, "node");
        assert_eq!(holder3333.pid, 12345);
        let msg3333 = format_port_in_use_message(Some(&holder3333), 3333);
        assert_eq!(msg3333, "Port 3333 is in use by node (PID 12345). Exiting.");

        // Verify detection on port 33333 (via ss fallback)
        let holder33333 = detect_port_holder(&fx.runner, 33333).expect("detect 33333 holder");
        assert_eq!(holder33333.process, "vite");
        assert_eq!(holder33333.pid, 54321);
        let msg33333 = format_port_in_use_message(Some(&holder33333), 33333);
        assert_eq!(
            msg33333,
            "Port 33333 is in use by vite (PID 54321). Exiting."
        );

        // Unknown holder formatting
        let msg_unknown = format_port_in_use_message(None, 3333);
        assert_eq!(
            msg_unknown,
            "Port 3333 is in use by an unknown process. Exiting."
        );

        // Check runner call count was at least 2 (lsof for 3333, lsof and ss for 33333)
        assert!(fx.runner.calls().len() >= 2);

        // Create serve.ts so prod WebUI is supervised
        let serve_ts = fx.home.root().join("web/src/server/serve.ts");
        fs::create_dir_all(serve_ts.parent().unwrap()).expect("create web server dir");
        fs::write(&serve_ts, "// mock server").expect("write serve.ts");

        // Running supervisor with port held should refuse to start and latch stopped
        let mut engine = fx.engine();
        webui::run(&mut engine);
        assert!(
            engine.webui.services[&ServiceKind::Prod].latched_stopped,
            "service should latch stopped when port is in use"
        );
        let events = fx.events();
        assert!(
            events
                .iter()
                .any(|e| e.kind == "error"
                    && e.text.contains("Port 3333 is in use by node (PID 12345)")),
            "error event should name process and PID"
        );
    }

    /// Regression test for webui-adopt:
    /// Engine supervisor terminates holder (SIGTERM) and launches the supervised server
    /// instead of latching stopped permanently when detect_port_holder finds an existing holder.
    #[test]
    fn engine_supervisor_replaces_foreign_port_holder() {
        let mut fx = SupervisionFx::new();

        // Create serve.ts so prod WebUI is supervised
        let serve_ts = fx.home.root().join("web/src/server/serve.ts");
        fs::create_dir_all(serve_ts.parent().unwrap()).expect("create web server dir");
        fs::write(&serve_ts, "// mock server").expect("write serve.ts");

        // Existing holder process on port 3333
        let holder_pid = fx.spawn.add_process(
            cpd_core::host::spawn::Pid(1),
            "node web/src/server/serve.ts",
        );
        let holder_pid_u32 = holder_pid.0;

        // Set up mock lsof output indicating holder PID is listening on port 3333
        let lsof_out = format!("p{holder_pid_u32}\ncnode\nn*:3333\n");
        fx.runner = fx.runner.on(
            "lsof",
            move |call| call.args.iter().any(|a| a == "-iTCP:3333"),
            Reply::exit(0, &lsof_out, ""),
        );
        // Mock ps output to report the holder's command line
        fx.runner = fx.runner.on(
            "ps",
            move |call| call.args.iter().any(|a| a == &holder_pid_u32.to_string()),
            Reply::exit(0, "node web/src/server/serve.ts\n", ""),
        );

        let mut engine = fx.engine();
        webui::run(&mut engine);

        // Supervisor should terminate holder with SIGTERM
        assert!(
            fx.spawn
                .signals()
                .iter()
                .any(|&(p, sig)| p == holder_pid && sig == cpd_core::host::spawn::Signal::Term),
            "supervisor should terminate the holder with SIGTERM"
        );

        // Before webui-adopt implementation: supervisor latches stopped when port holder is found
        // Expected behavior after webui-adopt: supervisor terminates foreign/existing holder and does NOT latch stopped,
        // successfully starting the supervised server.
        assert!(
            !engine.webui.services[&ServiceKind::Prod].latched_stopped,
            "supervisor should not latch stopped permanently when port holder is found"
        );
        assert!(
            engine.webui.services[&ServiceKind::Prod].pid.is_some(),
            "supervisor should launch the supervised server"
        );
        let launched_pid = engine.webui.services[&ServiceKind::Prod].pid.unwrap();
        assert_ne!(
            launched_pid, holder_pid,
            "launched server should be a new process"
        );
        assert!(
            fx.spawn.alive(launched_pid),
            "supervised server process should be alive"
        );
    }

    /// pro-d592, bug b74: after an install the restarted engine has no pid of its own. The new WebUI the install
    /// prepared already holds port 3333 (`node web/src/server/serve.ts`): the engine adopts it, as its own service,
    /// and neither signals it nor spawns a second one.
    #[test]
    fn restarted_engine_adopts_prepared_webui() {
        let mut fx = SupervisionFx::new();
        let serve_ts = fx.home.root().join("web/src/server/serve.ts");
        fs::create_dir_all(serve_ts.parent().unwrap()).expect("create web server dir");
        fs::write(&serve_ts, "// mock server").expect("write serve.ts");

        let prepared = fx.spawn.add_process(
            cpd_core::host::spawn::Pid(1),
            "node web/src/server/serve.ts",
        );
        let pid = prepared.0;
        let lsof_out = format!("p{pid}\ncnode\nn*:3333\n");
        fx.runner = fx.runner.on(
            "lsof",
            |call| call.args.iter().any(|a| a == "-iTCP:3333"),
            Reply::exit(0, &lsof_out, ""),
        );
        fx.runner = fx.runner.on(
            "ps",
            move |call| call.args.iter().any(|a| a == &pid.to_string()),
            Reply::exit(0, "node web/src/server/serve.ts\n", ""),
        );

        let mut engine = fx.engine();
        webui::run(&mut engine);

        assert_eq!(
            engine.webui.services[&ServiceKind::Prod].pid,
            Some(prepared),
            "the engine adopts the prepared WebUI as its prod service"
        );
        assert!(
            fx.spawn.launches().is_empty(),
            "no second WebUI is spawned: {:?}",
            fx.spawn.launches()
        );
        assert!(
            fx.spawn.signals().is_empty(),
            "the prepared WebUI is not signalled: {:?}",
            fx.spawn.signals()
        );
        assert!(fx.spawn.alive(prepared), "the prepared WebUI keeps serving");
    }
}
