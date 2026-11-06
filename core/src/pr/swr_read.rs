//! What `swr::source::github` uses of this module's private parts: the one read document and the header write.

pub use super::gh::{
    PollNode, PollPlan, PollRead, RepoMeta, build_poll_query, head_ref_oid_on, poll_once,
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
    Ok(())
}
