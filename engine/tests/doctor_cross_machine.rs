//! Red tests for `cpd doctor` cross-machine probing on ports 3333 and 33333.
//!
//! Context records: pro-d541, pro-d543; web/CONTRACT.md §31; docs/architecture/doctor-cross-machine.md.
//! Fleet configuration is read from `config.toml` (`[fleet.<machine>]`).
//! Probes ports 3333 and 33333 for each machine over HTTP GET, with 10s deadline.
//! Network and SSH must be mocked; all external calls are mocked.

mod doctor_cross_machine {
    #[test]
    fn probes_ports_3333_and_33333_across_fleet() {
        panic!("stub: test not implemented yet");
    }

    #[test]
    fn fails_loud_when_machine_is_unreachable() {
        panic!("stub: test not implemented yet");
    }
}
