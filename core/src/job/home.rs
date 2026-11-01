//! Where a Job belongs.

use crate::error::{CpdError, ErrorCode};
use crate::ids::WorkflowKey;

/// Where a Job belongs (`jobs.pr`, contract §2.2): to a Workflow, or, for an explore Job, to the Plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobHome {
    Workflow(WorkflowKey),
    Plan,
}

impl JobHome {
    /// Parse `jobs.pr` once: a present key must be a valid [`WorkflowKey`] (else `StoreFailed` naming `what`), `None` is `Plan`.
    pub fn of(what: &str, workflow: Option<&str>) -> Result<JobHome, CpdError> {
        match workflow {
            None => Ok(JobHome::Plan),
            Some(v) => WorkflowKey::parse(v).map(JobHome::Workflow).map_err(|_| {
                CpdError::new(
                    ErrorCode::StoreFailed,
                    format!("The store holds an invalid {what}: {v:?}."),
                )
                .with("value", v)
            }),
        }
    }
}
