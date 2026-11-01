//! The durable host supervisor (contract §6.19, P1.10): the engine starts and supervises one host per live
//! durable Session. Stub: the real start, restart bound and flock are the implementation's (p1-10-impl).

use std::fs::File;

use super::engine::Engine;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::spawn::Pid;
use cpd_core::ids::SessionId;

/// Per-Session restart bookkeeping (the implementation fills this in).
#[derive(Default)]
pub struct State;

/// The hosts step of the engine tick (contract §6.19): stub, starts nothing.
pub fn run(_engine: &mut Engine) {}

/// Stops the host of `session` (contract §6.19): stub, does nothing.
pub fn stop(_engine: &mut Engine, _session: &SessionId) {}

/// The one start of a Session's host (contract §6.19): stub, not implemented.
pub(super) fn start(_engine: &mut Engine, _session: &SessionId) -> Result<(Pid, File), CpdError> {
    Err(CpdError::new(ErrorCode::Internal, "hosts::start is not implemented yet."))
}
