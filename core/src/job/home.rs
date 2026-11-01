//! Where a Job belongs.

use crate::error::{CpdError, ErrorCode};
use crate::ids::PrKey;

/// Where a Job belongs (`jobs.pr`, contract §2.2): to a PR, or, for an explore Job, to the Plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobHome {
    Pr(PrKey),
    Plan,
}

impl JobHome {
    /// Parse `jobs.pr` once: a present key must be a valid [`PrKey`] (else `StoreFailed` naming `what`), `None` is `Plan`.
    pub fn of(what: &str, pr: Option<&str>) -> Result<JobHome, CpdError> {
        match pr {
            None => Ok(JobHome::Plan),
            Some(v) => PrKey::parse(v).map(JobHome::Pr).map_err(|_| {
                CpdError::new(
                    ErrorCode::StoreFailed,
                    format!("The store holds an invalid {what}: {v:?}."),
                )
                .with("value", v)
            }),
        }
    }
}
