//! The reader of GitHub state (docs/architecture/swr-github.md §6): the one function every consumer calls. A stub until
//! the implementing Job.

use crate::error::CpdError;
use crate::pr::pulls::PullRequest;
use crate::store::Store;

/// A pull request as a reader sees it: the raw row with the active mutations' patches applied.
pub struct EffectivePr {
    /// The raw `pull_requests` row, unchanged.
    pub row: PullRequest,
    /// The heavy part (`swr_pr_detail.detail_json`), `None` before the first hydrate.
    pub detail: Option<serde_json::Value>,
    /// Effective: `open`, `merged`, `closed`, or `merging` while a merge is queued or not yet proven.
    pub state: String,
    pub checks: String,
    pub review_decision: String,
    pub title: String,
    pub head_oid: Option<String>,
    pub feedback_open: Option<usize>,
    /// The ids of the mutations applied, in order.
    pub overlay: Vec<i64>,
}

/// The effective state of `repo#number`: `None` when there is no raw row.
pub fn effective_pr(
    _store: &mut Store,
    _repo: &str,
    _number: i64,
) -> Result<Option<EffectivePr>, CpdError> {
    todo!("swr::github::effective_pr")
}
