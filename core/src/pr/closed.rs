//! The closed-unmerged predicate: an `open` PR of cpd whose stored pull request is `closed` (never merged).

use rusqlite::Connection;

use crate::error::CpdError;
use crate::ids::{ContextId, PlanId, WorkflowKey, SessionId};

/// One closed-unmerged PR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedUnmerged {
    pub plan: PlanId,
    pub pr: WorkflowKey,
    pub context: ContextId,
    /// The Plan's Lead session; `None` when it has none.
    pub lead_session: Option<SessionId>,
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: String,
}

/// Every closed-unmerged PR of `plan` (every Plan when `None`), ordered by plan id then PR key.
pub fn closed_unmerged(conn: &Connection, plan: Option<&str>) -> Result<Vec<ClosedUnmerged>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.plan, p.key, l.context, l.lead_session, g.repo, g.number, g.url, g.title
           FROM workflows p
           JOIN pull_requests g ON p.pull_request = g.id
           JOIN plans l ON l.id = p.plan
          WHERE p.status = 'open' AND g.state = 'closed' AND (?1 IS NULL OR p.plan = ?1)
          ORDER BY p.plan, p.key",
    )?;
    let rows = stmt
        .query_map([plan], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(plan, workflow, context, lead, repo, number, url, title)| {
            Ok(ClosedUnmerged {
                plan: PlanId::parse(&plan)?,
                pr: WorkflowKey::parse(&workflow)?,
                context: ContextId::parse(&context)?,
                lead_session: lead.as_deref().map(SessionId::parse).transpose()?,
                repo,
                number,
                url,
                title,
            })
        })
        .collect()
}
