//! The install state file (contract §1.4, §3.7 E6; hs-d618): `$CPD_HOME/run/cpd-install.json`,
//! written by dotfiles' `scripts/cpd-install.sh` on every run and read by cpd each tick.
//!
//! One JSON object on one line, `{"status":"ok","since":"<RFC 3339 UTC, Z>"}` or
//! `{"status":"skipped","reason":"dirty|diverged|branch","since":"<RFC 3339 UTC, Z>"}` or
//! `{"status":"failed","reason":"<failure reason>","since":"<RFC 3339 UTC, Z>"}` or
//! `{"status":"restarting","since":"<RFC 3339 UTC, Z>"}` (no reason, no alert). `reason`
//! is present exactly when the status is `skipped` or `failed`; unknown fields are refused. A missing file is
//! `None`; a file that does not parse is one `install_state_invalid` (once until it clears).

use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// The reason the install's fast-forward was skipped (assumption A1): the checkout is `dirty`, its
/// `main` diverged from `origin/main`, or HEAD is `branch` (another branch or detached).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    Dirty,
    Diverged,
    Branch,
}

impl Reason {
    /// The `reason` field's text, and the value of `@cpd_host_alert`.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Dirty => "dirty",
            Reason::Diverged => "diverged",
            Reason::Branch => "branch",
        }
    }
}

/// The `status` field: `ok`, `skipped`, `failed`, or `restarting` (cpd install's detached waiter is
/// starting the engine; it alerts nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Skipped,
    Failed,
    Restarting,
}

/// The one JSON object on the line, as written: every field checked at the edge.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    status: Status,
    #[serde(default)]
    reason: Option<String>,
    since: String,
}

/// A parsed install state. The fields are true: `reason` is present exactly for `skipped` and `failed`,
/// and `since` is an RFC 3339 UTC time ending in `Z`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallState {
    /// The alerting reason, `None` when the state does not alert.
    reason: Option<String>,
    /// When the current status and reason began, an RFC 3339 UTC time ending in `Z`.
    since: String,
}

/// An `install_state_invalid` failure.
fn invalid(message: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::InstallStateInvalid, message)
}

/// Whether `text` is an RFC 3339 UTC time ending in `Z`.
fn utc_z(text: &str) -> bool {
    text.ends_with('Z') && OffsetDateTime::parse(text, &Rfc3339).is_ok_and(|t| t.offset().is_utc())
}

impl InstallState {
    /// `text` as the install state, or `install_state_invalid`: not JSON or not an object, an unknown
    /// field, an unknown status, a `reason` present exactly when the status is `skipped` or `failed`,
    /// a known reason for `skipped` (dirty, diverged, branch), a non-empty failure reason for `failed`,
    /// and a `since` that is an RFC 3339 UTC time ending in `Z`.
    pub fn parse(text: &str) -> Result<InstallState, CpdError> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| {
            invalid(format!(
                "The install state is not the one JSON object of contract §1.4: {e}."
            ))
            .with("reason", e.to_string())
        })?;
        let reason = match (raw.status, raw.reason) {
            (Status::Ok | Status::Restarting, None) => None,
            (Status::Skipped, Some(reason)) => match reason.as_str() {
                "dirty" | "diverged" | "branch" => Some(reason),
                _ => {
                    return Err(invalid(format!(
                        "The install state's `skipped` reason is unknown: {reason:?}."
                    ))
                    .with("reason", "unknown skipped reason"));
                }
            },
            (Status::Failed, Some(reason)) => {
                if reason.is_empty() {
                    return Err(invalid(
                        "The install state is `failed` but carries an empty `reason`.",
                    )
                    .with("reason", "empty reason with status failed"));
                }
                Some(reason)
            }
            (Status::Ok, Some(_)) => {
                return Err(invalid("The install state is `ok` but carries a `reason`.")
                    .with("reason", "reason present with status ok"));
            }
            (Status::Restarting, Some(_)) => {
                return Err(
                    invalid("The install state is `restarting` but carries a `reason`.")
                        .with("reason", "reason present with status restarting"),
                );
            }
            (Status::Skipped, None) => {
                return Err(
                    invalid("The install state is `skipped` but carries no `reason`.")
                        .with("reason", "reason missing with status skipped"),
                );
            }
            (Status::Failed, None) => {
                return Err(
                    invalid("The install state is `failed` but carries no `reason`.")
                        .with("reason", "reason missing with status failed"),
                );
            }
        };
        if !utc_z(&raw.since) {
            return Err(invalid(format!(
                "The install state's `since` is not an RFC 3339 UTC time ending in `Z`: {:?}.",
                raw.since
            ))
            .with("reason", "since is not an RFC 3339 UTC time ending in Z"));
        }
        Ok(InstallState {
            reason,
            since: raw.since,
        })
    }

    /// The file `$CPD_HOME/run/cpd-install.json`, or `None` when it does not exist; a file that
    /// cannot be read (other than missing) or does not parse is `install_state_invalid` with `path`.
    pub fn read(home: &CpdHome) -> Result<Option<InstallState>, CpdError> {
        let path = home.run_dir().join("cpd-install.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(invalid(format!(
                    "Cannot read the install state {}: {e}.",
                    path.display()
                ))
                .with("path", path.display().to_string())
                .with("reason", e.to_string()));
            }
        };
        InstallState::parse(&text)
            .map(Some)
            .map_err(|e| e.with("path", path.display().to_string()))
    }

    /// The reason the version pill turns red, or `None` when the state does not alert.
    pub fn alert(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// When the current status and reason began, an RFC 3339 UTC time ending in `Z` (the host
    /// Manager's message names the incident by it).
    pub fn since(&self) -> &str {
        &self.since
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarting_without_a_reason_parses_and_does_not_alert() {
        let state =
            InstallState::parse(r#"{"status":"restarting","since":"2026-01-15T12:00:00Z"}"#)
                .unwrap();
        assert_eq!(state.alert(), None);
        assert_eq!(state.since(), "2026-01-15T12:00:00Z");
    }

    #[test]
    fn restarting_with_a_reason_is_invalid() {
        let e = InstallState::parse(
            r#"{"status":"restarting","reason":"dirty","since":"2026-01-15T12:00:00Z"}"#,
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::InstallStateInvalid);
    }
}
