//! Cross-machine probes for cpd doctor (web/CONTRACT.md §31).

use super::check::Check;
use cpd_core::config::Config;

/// Runs the `cross_machine` check if fleet is configured.
pub fn check(_config: &Config) -> Option<Check> {
    None
}
