//! The install state file (contract §1.4, §3.7 E6; hs-d618): `$CPD_HOME/run/cpd-install.json`,
//! written by dotfiles' `scripts/cpd-install.sh` on every run and read by cpd each tick.
//!
//! One JSON object on one line, `{"status":"ok","since":"<RFC 3339 UTC, Z>"}` or
//! `{"status":"skipped","reason":"dirty|diverged|branch","since":"<RFC 3339 UTC, Z>"}`. `reason`
//! is present exactly when the status is `skipped`; unknown fields are refused. A missing file is
//! `None`; a file that does not parse is one `install_state_invalid` (once until it clears).

use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// The statuses §1.4 gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    Skipped,
}

/// The reasons that turn the version pill red (contract §3.7 E6, assumption A1).
const REASONS: [&str; 3] = ["dirty", "diverged", "branch"];

/// A parsed install state. The fields are true: `reason` is present exactly for `skipped`, and
/// `since` is an RFC 3339 UTC time ending in `Z`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallState {
    status: Status,
    reason: Option<String>,
    since: String,
}

/// The shape of the file, before its fields are checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    status: String,
    #[serde(default)]
    reason: Option<String>,
    since: String,
}

fn invalid(reason: impl Into<String>) -> CpdError {
    let reason = reason.into();
    CpdError::new(ErrorCode::InstallStateInvalid, format!("The install state is invalid: {reason}.")).with("reason", reason)
}

impl InstallState {
    /// `text` as the install state, or `install_state_invalid`: not JSON or not an object, an unknown
    /// field, an unknown status, a `reason` present exactly when the status is `skipped`, a known
    /// reason, and a `since` that is an RFC 3339 UTC time ending in `Z`.
    pub fn parse(text: &str) -> Result<InstallState, CpdError> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| invalid(format!("not the install state JSON: {e}")))?;
        let status = match raw.status.as_str() {
            "ok" => Status::Ok,
            "skipped" => Status::Skipped,
            other => return Err(invalid(format!("unknown status {other:?}"))),
        };
        let reason = match (status, raw.reason) {
            (Status::Ok, None) => None,
            (Status::Ok, Some(_)) => return Err(invalid("`reason` is present while the status is not `skipped`")),
            (Status::Skipped, None) => return Err(invalid("`reason` is missing while the status is `skipped`")),
            (Status::Skipped, Some(reason)) if REASONS.contains(&reason.as_str()) => Some(reason),
            (Status::Skipped, Some(reason)) => return Err(invalid(format!("unknown reason {reason:?}"))),
        };
        if !raw.since.ends_with('Z') || OffsetDateTime::parse(&raw.since, &Rfc3339).is_err() {
            return Err(invalid(format!("`since` is not an RFC 3339 UTC time ending in Z: {:?}", raw.since)));
        }
        Ok(InstallState { status, reason, since: raw.since })
    }

    /// The file `$CPD_HOME/run/cpd-install.json`, or `None` when it does not exist; a file that
    /// cannot be read (other than missing) or does not parse is `install_state_invalid` with `path`.
    pub fn read(home: &CpdHome) -> Result<Option<InstallState>, CpdError> {
        let path = home.run_dir().join("cpd-install.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(CpdError::file_unreadable(&path, e).with("path", path.display().to_string())),
        };
        InstallState::parse(&text).map(Some).map_err(|e| e.with("path", path.display().to_string()))
    }

    /// The reason the version pill turns red, or `None` when the state does not alert.
    pub fn alert(&self) -> Option<&str> {
        match self.status {
            Status::Ok => None,
            Status::Skipped => self.reason.as_deref(),
        }
    }

    /// When the current status and reason began (the click on a red pill carries it to the Manager).
    pub fn since(&self) -> &str {
        &self.since
    }
}
