//! The closed-unmerged predicate: an `open` PR of cpd whose stored pull request is `closed` (never merged).

use rusqlite::Connection;

use crate::error::CpdError;
use crate::ids::{ContextId, PlanId, PrKey, SessionId};

/// One closed-unmerged PR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedUnmerged {
    pub plan: PlanId,
    pub pr: PrKey,
    pub context: ContextId,
    /// The Plan's Lead session; `None` when it has none.
    pub lead_session: Option<SessionId>,
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: String,
}

/// Every closed-unmerged PR of `plan` (every Plan when `None`), ordered by plan id then PR key.
pub fn closed_unmerged(_conn: &Connection, _plan: Option<&str>) -> Result<Vec<ClosedUnmerged>, CpdError> {
    Ok(Vec::new())
}
