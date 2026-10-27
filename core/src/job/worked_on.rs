//! The pro-d88 rule: a `failed` Job is *being worked on* when its Plan's Lead is `working`, or a `queued` or `running`
//! Job of the same Plan and PR was created after the Job's latest `job-failed` Event.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::error::{CpdError, ErrorCode};
use crate::ids::{JobKey, PlanId};

/// The failed Jobs for which the pro-d88 rule holds, of one Plan (`Some`) or of every Plan (`None`). Read only.
pub fn failed_worked_on(conn: &Connection, plan: Option<&PlanId>) -> Result<BTreeSet<(PlanId, JobKey)>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT j.plan, j.key
           FROM jobs j
           JOIN plans p ON p.id = j.plan
           JOIN sessions l ON l.id = p.lead_session
          WHERE j.status = 'failed' AND (?1 IS NULL OR j.plan = ?1)
            AND (l.state = 'working' OR EXISTS (
                 SELECT 1 FROM jobs f
                  WHERE f.plan = j.plan AND f.pr = j.pr AND f.status IN ('queued', 'running')
                    AND f.created_txn > (SELECT max(e.txn) FROM events e
                                          WHERE e.plan = j.plan AND e.job = j.key AND e.kind = 'job-failed')))",
    )?;
    let mut rows = stmt.query([plan.map(PlanId::as_str)])?;
    let mut out = BTreeSet::new();
    while let Some(row) = rows.next()? {
        let (p, k): (String, String) = (row.get(0)?, row.get(1)?);
        out.insert((id("Plan id", &p, PlanId::parse)?, id("Job key", &k, JobKey::parse)?));
    }
    Ok(out)
}

fn id<T>(what: &str, value: &str, parse: fn(&str) -> Result<T, CpdError>) -> Result<T, CpdError> {
    parse(value).map_err(|_| {
        CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid {what}: {value:?}.")).with("value", value)
    })
}
