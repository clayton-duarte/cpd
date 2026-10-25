//! `cpd incident start | end | status` (commands.md §25, hs-d634): machine-wide incident mode.
//! When on (one row in the `incident` table), the engine pauses most scheduling and every running
//! Attempt is requeued when it ends (contract §2.2 "Incident", §3.7). `start` and `end` are
//! Manager-only; `status` is for any caller.
//!
//! This module is a stub: the tests of `incident_tests.rs` pin the contract, and the inc-core Job
//! replaces the bodies.

use crate::args::Text;
use crate::error::CpdError;
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::Store;

/// The `incident` commands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Turn incident mode on (Manager) (commands.md §25.1)
    Start(StartArgs),
    /// Turn incident mode off (Manager) (commands.md §25.2)
    End,
    /// Report whether incident mode is on (commands.md §25.3)
    Status,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The explanation of the incident (1 or more characters)
    #[arg(long, value_name = "text")]
    pub reason: Text,
}

/// Runs one `incident` command. The caller's Session is `$CPD_SESSION` (commands.md §25).
pub fn run(_command: Command) -> Result<Output, CpdError> {
    unimplemented!("the incident command is implemented by the inc-core Job")
}

/// `cpd incident start` (commands.md §25.1): one transaction that inserts the row, requests the
/// stop of every `starting` or `running` Attempt with no `stop_request` and appends one
/// `incident-started` Event. `caller` is the Manager's Session.
pub fn start(_store: &mut Store, _caller: Option<&SessionId>, _reason: &str) -> Result<Output, CpdError> {
    unimplemented!("incident start is implemented by the inc-core Job")
}

/// `cpd incident end` (commands.md §25.2): one transaction that deletes the row and appends one
/// `incident-ended` Event. `caller` is the Manager's Session.
pub fn end(_store: &mut Store, _caller: Option<&SessionId>) -> Result<Output, CpdError> {
    unimplemented!("incident end is implemented by the inc-core Job")
}

/// `cpd incident status` (commands.md §25.3): a read of the row, for any caller.
pub fn status(_store: &mut Store) -> Result<Output, CpdError> {
    unimplemented!("incident status is implemented by the inc-core Job")
}
