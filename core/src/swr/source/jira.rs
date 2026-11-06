//! The `jira` source: a stub until its Workflow (swr.md §14).

use time::OffsetDateTime;

use serde_json::Value;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::error::CpdError;
use crate::host::runner::Call;
use crate::policy::McpServer;
use crate::store::Store;
use crate::swr::outbox::{Coalesce, Mutation};

/// The effective Jira fields of a ticket (swr-jira.md §6): the cached row with the overlay applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub status_name: String,
    pub status_category: String,
}

/// What a run needs from the policy, read when the row runs (swr-jira.md §8 step 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub target_name: String,
    pub later_names: Vec<String>,
    pub estimate_fields: Vec<String>,
}

/// The rank of a target (swr-jira.md §4): `in_progress` 1, `in_review` 2, `done` 3.
pub fn rank(_target: &str) -> Option<i64> {
    todo!("the rank table")
}

/// Enqueue the `transition` op of `ticket` with `Coalesce::Forward(rank)` (swr-jira.md §2, §4, §7).
/// `status_name` is the overlay's name (`None`: no overlay, §6); `context` is `{plan, workflow}`.
pub fn enqueue_transition(
    _store: &mut Store,
    _now: OffsetDateTime,
    _ticket: &str,
    _target: &str,
    _status_name: Option<&str>,
    _context: Value,
) -> Result<i64, CpdError> {
    todo!("enqueue the transition")
}

/// The skip rule on the cached row (swr-jira.md §5): `true` when the row exists and is not behind.
pub fn skips_on_cache(
    _store: &mut Store,
    _ticket: &str,
    _wanted: &Wanted,
) -> Result<bool, CpdError> {
    todo!("the cached skip test")
}

/// The effective state of `ticket`: `swr_jira_issues` with `patch_json` applied in `submitted_at` order (§6).
pub fn effective(_store: &mut Store, _ticket: &str) -> Result<Option<Issue>, CpdError> {
    todo!("the overlay")
}

/// Set `inactive_at` on the `swr_jira_issues` rows nothing observes, and clear it on the rest (swr-jira.md §10).
pub fn mark_inactive(_store: &mut Store, _now: OffsetDateTime) -> Result<(), CpdError> {
    todo!("mark inactive")
}

pub struct Jira;

impl Jira {
    /// Run one claimed transition on the Runner port (swr-jira.md §8 steps 4 to 6): the cache test, then `helper` with the
    /// request of §3.1 as its stdin (`cloud_id` from `swr_atlassian` when fresh), then the answer mapped by the §8 table.
    pub fn run(
        &self,
        _cx: &mut Cx<'_>,
        _m: &Mutation,
        _wanted: &Wanted,
        _server: &McpServer,
        _helper: &Call,
    ) -> ExecOutcome {
        todo!("run the helper")
    }
}

impl Source for Jira {
    fn system(&self) -> &'static str {
        "jira"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["jira"]
    }
    fn due(&self, _cx: &mut Cx<'_>, _now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        Ok(Vec::new())
    }
    fn fetch(
        &self,
        _cx: &mut Cx<'_>,
        _keys: &[QueryKey],
        _poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        Ok(())
    }
    fn execute(&self, _cx: &mut Cx<'_>, _m: &Mutation) -> ExecOutcome {
        ExecOutcome::Failed {
            error: "not implemented".into(),
        }
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}
