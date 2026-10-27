//! The deploy read of `pr poll` (contract §7.6 step 8): the workflow runs that wait for the User's approval, of every
//! repo whose policy declares `[deploys] workflows`, cached in `run/deploy-runs.json` (§1.4) for E8's `deploy` item.

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::runner::Runner;

/// Reads the deploys that wait for the User's approval and rewrites `run/deploy-runs.json`, `polled_at` its time.
// STUB (red tests): reads nothing and writes nothing.
pub(super) fn read(_runner: &dyn Runner, _home: &CpdHome, _polled_at: &str) -> Result<(), CpdError> {
    Ok(())
}
