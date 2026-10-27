//! The hold E3 puts on a Job whose latest Attempt ended `provider_error` (decision pro-d102).

use cpd_core::clock::Clock;
use cpd_core::error::CpdError;
use cpd_core::ids::{JobKey, PlanId};

/// Whether Job `key`'s latest Attempt ended failed with cause provider_error less than 30 s before the time `clock`
/// reads (`Clock::utc`). Stub: never holds.
pub(super) fn holding_with(
    _conn: &rusqlite::Connection,
    _plan: &PlanId,
    _key: &JobKey,
    _clock: &dyn Clock,
) -> Result<bool, CpdError> {
    Ok(false)
}
