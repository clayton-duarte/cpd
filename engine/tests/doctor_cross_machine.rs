//! Red tests for `cpd doctor` cross-machine probing on ports 3333 and 33333.
//!
//! Context records: pro-d541, pro-d543; web/CONTRACT.md §31; docs/architecture/doctor-cross-machine.md.
//! Fleet configuration is read from `config.toml` (`[fleet.<machine>]`).
//! Probes ports 3333 and 33333 for each machine over HTTP GET, with 10s deadline.
//! Network and SSH must be mocked; all external calls are mocked.

mod doctor_cross_machine {
    use std::fs;
    use std::sync::Arc;

    use cpd_core::clock::ManualClock;
    use cpd_core::error::CpdError;
    use cpd_core::home::CpdHome;
    use cpd_core::host::runner::FakeRunner;
    use cpd_core::host::spawn::FakeSpawn;
    use cpd_core::output::Output;
    use cpd_core::ports::Ports;
    use cpd_engine::doctor::cross_machine::{ProbeOutcome, with_mock_probe};
    use cpd_engine::doctor::{self, Args};
    use serde_json::Value;
    use tempfile::TempDir;

    /// Test fixture providing an isolated temporary home and fake ports.
    struct DoctorFx {
        _temp: TempDir,
        pub home: CpdHome,
        pub clock: Arc<ManualClock>,
        pub spawn: Arc<FakeSpawn>,
        pub runner: FakeRunner,
    }

    impl DoctorFx {
        fn new(config_toml: &str) -> Self {
            let temp = TempDir::new().expect("tempdir");
            let home = CpdHome::resolve(Some(temp.path().join("home").into_os_string()), None)
                .expect("resolve home");
            fs::create_dir_all(home.run_dir()).expect("create run dir");

            fs::write(home.root().join("config.toml"), config_toml).expect("write config.toml");

            let clock = Arc::new(ManualClock::new());
            let runner = FakeRunner::new((*clock).clone());
            let spawn = Arc::new(FakeSpawn::new());

            DoctorFx {
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

        fn run_doctor(&self) -> Result<Output, CpdError> {
            let ports = self.ports();
            Ports::scope(&ports, || {
                let prev_home = std::env::var_os("CPD_HOME");
                unsafe {
                    std::env::set_var("CPD_HOME", self.home.root());
                }
                let res = doctor::run(Args {}, &ports.clock.zone());
                if let Some(prev) = prev_home {
                    unsafe {
                        std::env::set_var("CPD_HOME", prev);
                    }
                } else {
                    unsafe {
                        std::env::remove_var("CPD_HOME");
                    }
                }
                res
            })
        }
    }

    /// Finds the `cross_machine` check object from the doctor output checks list.
    fn find_cross_machine_check(checks: &[Value]) -> &Value {
        checks
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("cross_machine"))
            .expect("expected cross_machine check in doctor report")
    }

    fn extract_checks(res: Result<Output, CpdError>) -> Vec<Value> {
        match res {
            Ok(Output::Success(map)) => map
                .get("checks")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default(),
            Ok(Output::Owned) => Vec::new(),
            Err(err) => err
                .details()
                .get("checks")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Tests that cpd doctor reads fleet configuration, probes ports 3333 and 33333 across the fleet,
    /// and reports reachable status for each machine and port.
    ///
    /// Contract §31:
    /// - `name`: `cross_machine`, `severity`: `warn`.
    /// - `ok`: `true` when every probe is reachable.
    /// - `found`: array with one object per probe, in machine then port order:
    ///   `{"machine": "...", "host": "...", "port": ..., "reachable": true, "status": ...}`
    /// - `expected`: `"every fleet machine answers on 3333 and 33333"`
    #[test]
    fn probes_ports_3333_and_33333_across_fleet() {
        let toml = r#"
version = 1
machine = "pro"
lead_pi_packages = []
[watch]
tick_seconds = 1
janitor_minutes = 60
[timeouts]
store_busy_ms = 5000
archive_sleep_seconds = 86400

[fleet.air]
host = "192.168.3.5"

[fleet.hs]
host = "homeserver"
"#;
        let fx = DoctorFx::new(toml);
        let res = with_mock_probe(
            |_host, _port, _timeout| ProbeOutcome::Reachable { status: 200 },
            || fx.run_doctor(),
        );
        let checks = extract_checks(res);

        let check = find_cross_machine_check(&checks);
        assert_eq!(
            check.get("name").and_then(|v| v.as_str()),
            Some("cross_machine")
        );
        assert_eq!(check.get("severity").and_then(|v| v.as_str()), Some("warn"));
        assert_eq!(check.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            check.get("expected").and_then(|v| v.as_str()),
            Some("every fleet machine answers on 3333 and 33333")
        );

        let found = check
            .get("found")
            .and_then(|v| v.as_array())
            .expect("found array");
        assert_eq!(
            found.len(),
            4,
            "expected 4 probes: air (3333, 33333), hs (3333, 33333)"
        );

        assert_eq!(found[0]["machine"], "air");
        assert_eq!(found[0]["host"], "192.168.3.5");
        assert_eq!(found[0]["port"], 3333);
        assert_eq!(found[0]["reachable"], true);

        assert_eq!(found[1]["machine"], "air");
        assert_eq!(found[1]["host"], "192.168.3.5");
        assert_eq!(found[1]["port"], 33333);
        assert_eq!(found[1]["reachable"], true);

        assert_eq!(found[2]["machine"], "hs");
        assert_eq!(found[2]["host"], "homeserver");
        assert_eq!(found[2]["port"], 3333);
        assert_eq!(found[2]["reachable"], true);

        assert_eq!(found[3]["machine"], "hs");
        assert_eq!(found[3]["host"], "homeserver");
        assert_eq!(found[3]["port"], 33333);
        assert_eq!(found[3]["reachable"], true);
    }

    /// Tests that cpd doctor fails loud (warns with details) when any machine/port is unreachable.
    ///
    /// Contract §31:
    /// - `ok`: `false` when any probe fails.
    /// - `severity`: `warn` (never fails doctor overall exit, since it's an environment fact).
    /// - `found`: objects with `reachable: false`, `reason`: "refused" | "timeout" | "unresolved" | etc.
    /// - `message`: `<n> of <m> fleet probes unreachable: <machine>:<port> <reason>, ...`
    #[test]
    fn fails_loud_when_machine_is_unreachable() {
        let toml = r#"
version = 1
machine = "pro"
lead_pi_packages = []
[watch]
tick_seconds = 1
janitor_minutes = 60
[timeouts]
store_busy_ms = 5000
archive_sleep_seconds = 86400

[fleet.air]
host = "192.168.3.5"

[fleet.hs]
host = "homeserver"
"#;
        let fx = DoctorFx::new(toml);
        let res = with_mock_probe(
            |host, port, _timeout| {
                if host == "homeserver" && port == 33333 {
                    ProbeOutcome::Unreachable {
                        reason: "refused",
                        message: None,
                    }
                } else {
                    ProbeOutcome::Reachable { status: 200 }
                }
            },
            || fx.run_doctor(),
        );
        let checks = extract_checks(res);

        let check = find_cross_machine_check(&checks);
        assert_eq!(
            check.get("name").and_then(|v| v.as_str()),
            Some("cross_machine")
        );
        assert_eq!(check.get("severity").and_then(|v| v.as_str()), Some("warn"));
        assert_eq!(check.get("ok").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            check.get("expected").and_then(|v| v.as_str()),
            Some("every fleet machine answers on 3333 and 33333")
        );

        let msg = check
            .get("message")
            .and_then(|v| v.as_str())
            .expect("message when unreachable");
        assert!(
            msg.contains("fleet probes unreachable"),
            "expected fail-loud message containing unreachable probe summary, got: {msg}"
        );

        let found = check
            .get("found")
            .and_then(|v| v.as_array())
            .expect("found array");
        assert!(
            found.iter().any(|p| p["reachable"] == false),
            "expected at least one probe with reachable: false"
        );
    }
}
