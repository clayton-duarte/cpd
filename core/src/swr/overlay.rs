//! The overlay (swr.md §9): the writes the cache does not yet show.

use super::outbox::{Mutation, mutation};
use crate::error::CpdError;
use crate::store::Store;

/// The `pending`, `in_flight` and `revalidating` mutations of the key, in `submitted_at` order.
pub fn active(
    store: &mut Store,
    system: &str,
    target_key: &str,
) -> Result<Vec<Mutation>, CpdError> {
    store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT id, system, target_key, op_kind, vars_json, patch_json, status, submitted_at, started_at,
                    completed_at, next_attempt_at, attempt_count, error, context_json
             FROM swr_mutations
             WHERE system = ?1 AND target_key = ?2 AND status IN ('pending', 'in_flight', 'revalidating')
             ORDER BY submitted_at, id",
        )?;
        Ok(stmt
            .query_map([system, target_key], mutation)?
            .collect::<Result<Vec<_>, _>>()?)
    })
}
