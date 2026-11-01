//! The Lead's current Workflow: `sessions.working_pr` (decisions pro-d217, pro-d219, pro-d259).
//! cpd names it in the write transaction of the commands that act on a PR;
//! nothing matches (the User, a Worker, a Lead not `working`) is not an error.

use rusqlite::Connection;

use crate::error::CpdError;
use crate::ids::{PlanId, WorkflowKey};

pub fn name(tx: &Connection, plan: &PlanId, pr: &WorkflowKey) -> Result<(), CpdError> {
    let id = match std::env::var("CPD_SESSION") {
        Ok(id) if !id.is_empty() => id,
        _ => return Ok(()),
    };
    tx.execute(
        "UPDATE sessions SET working_pr = ?1 WHERE id = ?2 AND role = 'lead' AND plan = ?3 AND state = 'working' AND working_pr IS NOT ?1",
        rusqlite::params![pr.as_str(), id, plan.as_str()],
    )?;
    Ok(())
}
