//! A2 of an explore Attempt (contract §3.4 "An explore Job's Attempt", R7): no worktree, no `pre-start`, no `base_sha`; the
//! explore directory is created if missing and is the Worker's cwd, and the Job's stale artefact is deleted.

use std::path::PathBuf;

use crate::attempt::ledger::Attempt as Target;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ports::Ports;

/// Prepares the start of an explore `target`: creates the explore directory, deletes the stale artefact of the Job there
/// (a file an earlier Attempt left), and returns the Worker's cwd. Runs no git and no hook. Stub: red tests only.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn prepare(_ports: &Ports, _home: &CpdHome, target: &Target) -> Result<PathBuf, CpdError> {
    Ok(target.worktree.clone())
}
