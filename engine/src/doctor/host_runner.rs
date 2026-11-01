//! The `host_runner` check (contract §6.19, P1.10): for `durable`, `<pi_adapter>/durable-host/main.ts` exists and
//! pi's Node can run it. Stub: passes without looking at anything; the implementation fills it in.

use cpd_core::config::Config;

use super::check::{Check, Severity};

/// Passes today; the implementation requires the durable host beside `pi_adapter` when either runner key is `durable`.
pub fn check(_config: &Config) -> Check {
    Check::pass("host_runner", Severity::Error)
}
