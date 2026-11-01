//! `cpd doctor` (commands.md §14.1): checks the machine. Every check yields a
//! [`Check`]; any failing `error` check makes the result `doctor_failed`.
//! Read-only, except the migration of a database's first open.

mod check;
mod holographic;
mod host_runner;
mod pi;
mod state;
mod tools;
mod worker_runner;

#[cfg(test)]
mod runner_tests;

use std::time::Duration;

use serde_json::json;

use crate::watch;
use check::{Check, Severity};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::limit::limit;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

/// Each probe's deadline (commands.md §14.1): `doctor` is run when something is wrong,
/// so a hung tool is a failed check, not a hung `doctor`.
const DEADLINE: Duration = Duration::from_secs(10);

/// [`DEADLINE`], or the test's `CPD_TEST_DOCTOR_DEADLINE` ([`limit`]). A probe's own
/// `CPD_TEST_DEADLINE_<TOOL>` wins over both (`host::deadline`).
fn deadline() -> Duration {
    limit("DOCTOR_DEADLINE", DEADLINE)
}

/// The `tmux_restart` check, present only while a restart of the tmux server is pending or has
/// failed (contract §6.1): pending is a planned hold, not an error (the engine starts no Attempt
/// until it has run); a failed restart is a warning with its reason.
fn tmux_restart(home: &CpdHome, zone: &cpd_core::clock::TimeZone) -> Option<Check> {
    let name = "tmux_restart";
    match watch::restart::read(home) {
        Ok(None) => None,
        Ok(Some(pending)) if pending.failed.is_none() => Some(
            Check::pass(name, Severity::Warn)
                .message("tmux restart pending")
                .found(pending.describe(zone)),
        ),
        Ok(Some(pending)) => Some(Check::fail(name, Severity::Warn, pending.describe(zone))),
        Err(e) => Some(Check::from_error(name, Severity::Warn, &e)),
    }
}

/// `cpd doctor` takes no arguments.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct Args {}

/// `zone` is the one prose times are written in, read once at the edge (the Ports clock's).
pub fn run(_args: Args, zone: &cpd_core::clock::TimeZone) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home);
    let mut store = config
        .as_ref()
        .ok()
        .map(|c| Store::open(&home, c.timeouts.store_busy_ms.into()));
    let needs_config = "config.toml is not valid.";

    let mut checks = vec![match &config {
        Ok(_) => Check::pass("config", Severity::Error).found(home.config().display().to_string()),
        Err(e) => Check::from_error("config", Severity::Error, e),
    }];
    let (store_check, mut store) = match store.take() {
        Some(opened) => state::store(opened),
        None => (
            Check::not_checked("store", Severity::Error, needs_config),
            None,
        ),
    };
    checks.push(store_check);
    // Whether the store holds a live durable Session (contract §6.19): the `host_runner` check must look even when
    // both config keys are `pi`. False when the store is unavailable, like the other 'not checked' paths.
    let durable_session = store
        .as_mut()
        .and_then(|store| {
            store
                .read(|conn| {
                    Ok(conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sessions WHERE role IN ('lead', 'manager') AND runner = 'durable' AND archived_at IS NULL)",
                        [],
                        |r| r.get::<_, bool>(0),
                    )?)
                })
                .ok()
        })
        .unwrap_or(false);
    checks.push(state::tamper(store.as_mut()));
    checks.extend(tools::tools());
    checks.push(tools::gh_auth());
    checks.push(tools::pi_node());
    let with_config = |name, severity, run: &dyn Fn(&Config) -> Check| match &config {
        Ok(config) => run(config),
        Err(_) => Check::not_checked(name, severity, needs_config),
    };
    checks.push(with_config("tiers", Severity::Error, &pi::tiers));
    checks.push(with_config("pi_adapter", Severity::Error, &pi::adapter));
    checks.push(with_config(
        "worker_runner",
        Severity::Error,
        &worker_runner::worker_runner,
    ));
    checks.push(with_config(
        "host_runner",
        Severity::Error,
        &|config| host_runner::check_with(config, durable_session, &*Ports::current().runner),
    ));
    checks.push(with_config(
        "lead_packages",
        Severity::Warn,
        &pi::lead_packages,
    ));
    checks.push(state::watch(&home, store.as_mut()));
    checks.extend(tmux_restart(&home, zone));
    checks.push(with_config(
        "holographic",
        Severity::Error,
        &holographic::holographic,
    ));

    let failed: Vec<&str> = checks
        .iter()
        .filter(|c| c.is_error_failure())
        .map(|c| c.name)
        .collect();
    if failed.is_empty() {
        return Ok(Output::success(json!({ "checks": checks })));
    }
    Err(CpdError::new(
        ErrorCode::DoctorFailed,
        format!("These checks failed: {}.", failed.join(", ")),
    )
    .with(
        "checks",
        serde_json::to_value(&checks).expect("checks serialize"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_probes_deadline_is_10_seconds_unless_a_test_sets_it() {
        // No test sets CPD_TEST_DOCTOR_DEADLINE in this process (a test sets variables on the commands it runs).
        assert_eq!(deadline(), Duration::from_secs(10));
    }
}
