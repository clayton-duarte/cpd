//! The hold E3 puts on a Job whose latest Attempt ended `provider_error` (decision pro-d102).

use cpd_core::attempt::ledger::PROVIDER_RETRY_SECONDS;
use cpd_core::clock::Clock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ids::{JobKey, PlanId};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Whether Job `key`'s latest Attempt ended failed with cause provider_error less than 30 s before the time `clock`
/// reads (`Clock::utc`).
pub(super) fn holding_with(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &JobKey,
    clock: &dyn Clock,
) -> Result<bool, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT ended_at FROM attempts WHERE plan = ?1 AND job = ?2 AND status = 'ended'
            AND outcome = 'failed' AND cause = 'provider_error'
            AND number = (SELECT max(number) FROM attempts WHERE plan = ?1 AND job = ?2)",
    )?;
    let mut rows = stmt.query([plan.as_str(), key.as_str()])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    let ended_at: String = row.get(0)?;
    let ended = OffsetDateTime::parse(&ended_at, &Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The Ledger holds an invalid value: ended_at {ended_at:?} of an Attempt of Job {key}: {e}."))
    })?;
    let wait = time::Duration::seconds(i64::try_from(PROVIDER_RETRY_SECONDS).expect("30 fits"));
    Ok(clock.utc() < ended + wait)
}
