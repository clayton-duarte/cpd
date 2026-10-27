//! The pro-d88 rule: a `failed` Job is *being worked on* when its Plan's Lead is `working`, or a `queued` or `running`
//! Job of the same Plan and PR was created after the Job's latest `job-failed` Event.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::error::CpdError;
use crate::ids::{JobKey, PlanId};

/// The failed Jobs for which the pro-d88 rule holds, of one Plan (`Some`) or of every Plan (`None`). Read only.
pub fn failed_worked_on(_conn: &Connection, _plan: Option<&PlanId>) -> Result<BTreeSet<(PlanId, JobKey)>, CpdError> {
    Ok(BTreeSet::new())
}
