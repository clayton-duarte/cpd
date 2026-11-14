//! What `swr::source::github` uses of this module's private parts: the one read document and the header write.

pub use super::gh::{
    Check, PollNode, PollPlan, PollRead, RepoMeta, build_poll_query, checks_of_detail, describe_pr,
    disable_auto_merge, head_ref_oid_on, merge_pr, poll_once, post_review, pull_request_id,
    reply_to_comment, reply_to_thread, reply_to_uncached, rerun_failed, retitle_pr,
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
    write_review_metadata(tx, node)
}

pub fn review_metadata_held(
    conn: &rusqlite::Connection,
    node: &PollNode,
) -> Result<bool, CpdError> {
    use rusqlite::OptionalExtension;
    let reviewers = node.reviews["nodes"].as_array().map(|nodes| {
        serde_json::to_string(
            &nodes
                .iter()
                .filter_map(|r| r["author"]["login"].as_str())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    });
    let threads = node
        .threads_complete
        .then(|| serde_json::to_string(&node.listed.pr.open_threads).unwrap());
    Ok(conn.query_row("SELECT review_threads_complete = ?3
        AND (?4 IS NULL OR open_threads IS ?4) AND (?5 IS NULL OR reviewers IS ?5)
        AND (?5 IS NULL OR rereview_head IS NOT NULL) FROM pull_requests WHERE repo = ?1 AND number = ?2",
        rusqlite::params![node.listed.pr.repo, node.listed.pr.number, node.threads_complete, threads, reviewers],
        |r| r.get(0)).optional()?.unwrap_or(false))
}

/// A missing/truncated connection is unknown, never evidence that no thread is open.
pub fn write_review_metadata(tx: &WriteTxn<'_>, node: &PollNode) -> Result<(), CpdError> {
    let reviewers = node.reviews["nodes"].as_array().map(|nodes| {
        nodes
            .iter()
            .filter_map(|r| r["author"]["login"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    });
    let reviewers = reviewers.map(|r| serde_json::to_string(&r).unwrap());
    let threads = node
        .threads_complete
        .then(|| serde_json::to_string(&node.listed.pr.open_threads).unwrap());
    tx.execute(
        "UPDATE pull_requests SET review_threads_complete = ?3,
        open_threads = COALESCE(?4, open_threads), reviewers = COALESCE(?5, reviewers),
        rereview_head = COALESCE(rereview_head, ?6)
        WHERE repo = ?1 AND number = ?2 AND (review_threads_complete IS NOT ?3
          OR (?4 IS NOT NULL AND open_threads IS NOT ?4) OR (?5 IS NOT NULL AND reviewers IS NOT ?5)
          OR (rereview_head IS NULL AND ?6 IS NOT NULL))",
        rusqlite::params![
            node.listed.pr.repo,
            node.listed.pr.number,
            node.threads_complete,
            threads,
            reviewers,
            reviewers.as_ref().map(|_| &node.head_oid)
        ],
    )?;
    Ok(())
}

/// Whether a label name of `labels` matches (ASCII case-insensitive) one of the `excused` labels (`status::policy_hold`).
pub fn policy_hold(labels: &[String], excused: &[String]) -> bool {
    super::status::policy_hold(labels, excused)
}

/// pro-d453: a Light node carries only the rollup state, so a PR whose Danger was excused by the hydrate would read `failed`
/// again at every pass, and the coarse rollup is `FAILURE` while any check is CANCELLED (pro-d489). For a node without a heavy
/// part and a failed rollup, the checks of the stored `detail` of the same head are reduced again (which drops CANCELLED,
/// SKIPPED and older attempts), under the policy hold when an excused label is present. A detail with no contexts changes nothing.
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
    if node.heavy.is_none() && node.listed.pr.checks == ChecksSummary::Failed {
        let checks = checks_of_detail(detail);
        if !checks.is_empty() {
            node.listed.pr.checks =
                ChecksSummary::of(&checks, &[], &[], policy_hold(&labels, excused));
        }
    }
    // pro-d490: UNSTABLE only reflects a non-SUCCESS rollup; with the checks passed it is clean.
    if node.listed.pr.checks == ChecksSummary::Passed
        && node.listed.pr.merge_state == super::status::MergeState::Unstable
    {
        node.listed.pr.merge_state = super::status::MergeState::Clean;
    }
}

/// b61: under a policy hold a Light node whose rollup is `failed` and whose cached detail of the head has no contexts has
/// unknown checks: it may be only the excused Danger, so it is stored `pending` and raises no `checks_failed`.
/// Only a non-Danger failing check seen in contexts may fail a held PR.
pub fn hold_unknown_failure(
    node: &mut PollNode,
    detail: Option<&serde_json::Value>,
    excused: &[String],
) {
    use super::status::ChecksSummary;
    let held = node.labels.as_array().is_some_and(|l| {
        let labels: Vec<String> = l
            .iter()
            .filter_map(|n| n.as_str().map(str::to_owned))
            .collect();
        policy_hold(&labels, excused)
    });
    let known = detail.is_some_and(|d| !super::gh::checks_of_detail(d).is_empty());
    if node.heavy.is_none() && held && !known && node.listed.pr.checks == ChecksSummary::Failed {
        node.listed.pr.checks = ChecksSummary::Pending;
    }
}
