//! The engine's retry of a failed `open` or `update` Job when its fix merges (contract §3.2; Context decision pro-d89).

use crate::error::CpdError;
use crate::ids::{JobKey, PlanId};
use crate::store::WriteTxn;

/// Requeues each failed `open` or `update` Job of `merged`'s PR that `merged` fixes; returns the keys it requeued.
/// Call it after the row of `merged` is `merged`, in the same transaction.
pub(crate) fn retry_after_fix(tx: &WriteTxn<'_>, plan: &PlanId, merged: &JobKey) -> Result<Vec<JobKey>, CpdError> {
    let _ = (tx, plan, merged);
    unimplemented!("retry_after_fix")
}
