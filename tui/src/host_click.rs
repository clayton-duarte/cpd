//! `cpd tui action host` (commands.md §9.12, hs-d618): a click on the red version pill opens the
//! machine's host Manager and wakes it with the install incident.
//!
//! Stub for the red tests of `cli/tests/tui/tui_host_click.rs`: the behavior is implemented by a
//! later Job.

use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;

/// `cpd tui action host --client <client>` (commands.md §9.12).
pub fn run(_home: &CpdHome, _config: &Config, _tmux: &Tmux, _client: &str) -> Result<(), CpdError> {
    todo!("cpd tui action host")
}
