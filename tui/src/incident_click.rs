//! `cpd tui action incident` (commands.md §9.13, hs-d678): a click on the incident pill opens the
//! incident's Lead in the chat slot and sends it no message.
//!
//! With incident mode off, or with no resolvable Lead, `run` returns a `Usage` error, which the
//! generic [`crate::cockpit::action`] wrapper reports on the clicking client; this module prints
//! nothing.

use super::RunCommand;

use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::store::Store;

/// `cpd tui action incident --client <client>` (commands.md §9.13): opens the incident's Lead in
/// the chat slot; `run_command` runs `cpd`'s commands in this process.
pub fn run(
    home: &CpdHome,
    config: &Config,
    tmux: &Tmux,
    run_command: RunCommand,
) -> Result<(), CpdError> {
    let mut store = Store::open(home, u64::from(config.timeouts.store_busy_ms))?;
    let (on, lead) = store.read(|conn| {
        Ok((
            cpd_core::incident::is_on(conn),
            cpd_core::incident::lead(conn)?,
        ))
    })?;
    match lead {
        Some(session) => super::widgets::open_lead(
            run_command,
            tmux,
            config.tmux.cockpit_session.as_str(),
            &session,
        ),
        None if on => Err(CpdError::new(ErrorCode::Usage, "incidents require a lead")),
        None => Err(CpdError::new(
            ErrorCode::Usage,
            "no incident: nothing to open",
        )),
    }
}
