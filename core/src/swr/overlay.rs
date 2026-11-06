//! The overlay (swr.md §9). Stub until the implementing Job.

use super::outbox::Mutation;
use crate::error::CpdError;
use crate::store::Store;

/// The `pending`, `in_flight` and `revalidating` mutations of the key, in `submitted_at` order.
pub fn active(
    _store: &mut Store,
    _system: &str,
    _target_key: &str,
) -> Result<Vec<Mutation>, CpdError> {
    todo!("swr overlay active")
}
