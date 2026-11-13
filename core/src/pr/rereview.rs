//! The bot re-review comment (pro-d588, b71): after a Lead pushes, the engine asks each bot that reviewed the PR to
//! re-review, through the `swr_mutations` outbox. STUB: the red tests of `rereview_tests` pin the contract.

use time::OffsetDateTime;

use crate::error::CpdError;
use crate::store::Store;

/// One push of the Lead to a PR that a bot reviewed.
pub struct Push<'a> {
    pub plan: &'a str,
    pub workflow: &'a str,
    pub repo: &'a str,
    pub number: i64,
    pub head_sha: &'a str,
    /// Logins that actually reviewed the PR.
    pub reviewers: &'a [String],
    /// `pr.rereview_bots`.
    pub configured: &'a [String],
}

/// The comment asking `bot` to re-review.
#[must_use]
pub fn comment_text(_bot: &str) -> String {
    String::new()
}

/// Queues one `comment` mutation per bot that reviewed and is configured, once per (plan, workflow, head_sha).
/// Returns the bots asked by this call.
pub fn post(
    _store: &mut Store,
    _now: OffsetDateTime,
    _push: &Push<'_>,
) -> Result<Vec<String>, CpdError> {
    unimplemented!("bot re-review post")
}
