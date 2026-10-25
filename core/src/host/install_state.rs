//! The install state file (contract §1.4, §3.7 E6; hs-d618): `$CPD_HOME/run/cpd-install.json`,
//! written by dotfiles' `scripts/cpd-install.sh` on every run and read by cpd each tick.
//!
//! One JSON object on one line, `{"status":"ok","since":"<RFC 3339 UTC, Z>"}` or
//! `{"status":"skipped","reason":"dirty|diverged|branch","since":"<RFC 3339 UTC, Z>"}`. `reason`
//! is present exactly when the status is `skipped`; unknown fields are refused. A missing file is
//! `None`; a file that does not parse is one `install_state_invalid` (once until it clears).
//!
//! STUB: the test Job pins the contract; the implementation Job fills these bodies.

use crate::error::CpdError;
use crate::home::CpdHome;

/// A parsed install state. The fields are true: `reason` is present exactly for `skipped`, and
/// `since` is an RFC 3339 UTC time ending in `Z`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallState;

impl InstallState {
    /// `text` as the install state, or `install_state_invalid`: not JSON or not an object, an unknown
    /// field, an unknown status, a `reason` present exactly when the status is `skipped`, a known
    /// reason, and a `since` that is an RFC 3339 UTC time ending in `Z`.
    pub fn parse(text: &str) -> Result<InstallState, CpdError> {
        todo!()
    }

    /// The file `$CPD_HOME/run/cpd-install.json`, or `None` when it does not exist; a file that
    /// cannot be read (other than missing) or does not parse is `install_state_invalid` with `path`.
    pub fn read(home: &CpdHome) -> Result<Option<InstallState>, CpdError> {
        todo!()
    }

    /// The reason the version pill turns red, or `None` when the state does not alert.
    pub fn alert(&self) -> Option<&str> {
        todo!()
    }
}
