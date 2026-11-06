//! What `swr::source::github` uses of this module's private parts: the one read document and the header write.

pub use super::gh::{
    Check, PollNode, PollPlan, PollRead, RepoMeta, build_poll_query, checks_of_detail, describe_pr,
    head_ref_oid_on, merge_pr, poll_once, post_review, pull_request_id, reply_to_comment,
    reply_to_thread, rerun_failed, retitle_pr,
};
use super::pulls;
use crate::error::CpdError;
use crate::store::WriteTxn;

/// The header of `node` into `pull_requests` (only when a value differs), with the head commit and GitHub's `updatedAt` that
/// the next pass compares with `swr_pr_detail`.
pub fn write_header(tx: &WriteTxn<'_>, node: &PollNode) -> Result<(), CpdError> {
    pulls::upsert(tx, &node.listed, None)?;
    tx.execute(
        "UPDATE pull_requests SET head_oid = ?3, gh_updated_at = ?4
         WHERE repo = ?1 AND number = ?2 AND (head_oid IS NOT ?3 OR gh_updated_at IS NOT ?4)",
        rusqlite::params![
            node.listed.pr.repo,
            node.listed.pr.number,
            node.head_oid,
            node.listed.pr.updated_at
        ],
    )?;
    // pro-d453: what the Light read carries and `pull_requests` has no field of in `PolledPr`.
    tx.execute(
        "UPDATE pull_requests SET author_login = ?3, reviews_json = ?4, labels_json = ?5
         WHERE repo = ?1 AND number = ?2
           AND (author_login IS NOT ?3 OR reviews_json IS NOT ?4 OR labels_json IS NOT ?5)",
        rusqlite::params![
            node.listed.pr.repo,
            node.listed.pr.number,
            node.author_login,
            node.reviews.to_string(),
            node.labels.to_string()
        ],
    )?;
    Ok(())
}

/// Whether a label name of `labels` matches (ASCII case-insensitive) one of the `excused` labels (`status::policy_hold`).
pub fn policy_hold(labels: &[String], excused: &[String]) -> bool {
    super::status::policy_hold(labels, excused)
}

/// pro-d453: a Light node carries only the rollup state, so a PR whose Danger was excused by the hydrate would read `failed`
/// again at every pass. For a node without a heavy part, a failed rollup and an excused label, the checks of the stored
/// `detail` of the same head are reduced again under the policy hold. A detail with no contexts changes nothing.
pub fn excuse_from_detail(node: &mut PollNode, detail: &serde_json::Value, excused: &[String]) {
    use super::status::ChecksSummary;
    let labels: Vec<String> = node
        .labels
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if node.heavy.is_some()
        || node.listed.pr.checks != ChecksSummary::Failed
        || !policy_hold(&labels, excused)
    {
        return;
    }
    let checks = checks_of_detail(detail);
    if !checks.is_empty() {
        node.listed.pr.checks = ChecksSummary::of(&checks, &[], &[], true);
    }
}
