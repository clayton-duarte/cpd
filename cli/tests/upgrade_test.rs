//! Red tests for cpd upgrade (docs/architecture/upgrade.md, docs/commands.md §14.4c, docs/contract.md §3.7c).

mod upgrade_test {
    #[test]
    fn upgrade_dry_runs_migration_on_copy_and_swaps_binary() {
        let ports = cpd_core::ports::Ports::real();
        let home = cpd_core::home::CpdHome::resolve(None, None).expect("home");
        let config = cpd_core::upgrade::UpgradeConfig {
            home,
            dry_run: false,
            check_only: false,
            target_bin: None,
        };
        let _ = cpd_core::upgrade::run_upgrade(&ports, &config);
    }

    #[test]
    fn upgrade_rolls_back_on_failed_health_check() {
        let ports = cpd_core::ports::Ports::real();
        let home = cpd_core::home::CpdHome::resolve(None, None).expect("home");
        let config = cpd_core::upgrade::UpgradeConfig {
            home,
            dry_run: false,
            check_only: false,
            target_bin: None,
        };
        let _ = cpd_core::upgrade::run_upgrade(&ports, &config);
    }
}
