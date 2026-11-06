//! `swr_endpoints` (swr.md §7): endpoint health.

use time::OffsetDateTime;

use crate::error::CpdError;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Transient,
    Permanent,
}

/// A row of `swr_endpoints` (swr.md §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub endpoint: String,
    pub status: Option<String>,
    pub fetch_status: String,
    pub paused_until: Option<OffsetDateTime>,
    pub failing_since: Option<OffsetDateTime>,
    pub reported_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub error_count: i64,
    pub last_ok_at: Option<OffsetDateTime>,
    pub rate_remaining: Option<i64>,
    pub rate_reset_at: Option<OffsetDateTime>,
}

/// The row of `endpoint`, if any.
pub fn get(_store: &mut Store, _endpoint: &str) -> Result<Option<Endpoint>, CpdError> {
    todo!()
}

pub fn record_ok(
    _store: &mut Store,
    _endpoint: &str,
    _now: OffsetDateTime,
) -> Result<(), CpdError> {
    todo!()
}

pub fn record_failure(
    _store: &mut Store,
    _endpoint: &str,
    _now: OffsetDateTime,
    _error: &str,
    _kind: Kind,
) -> Result<(), CpdError> {
    todo!()
}

pub fn due_report(
    _store: &mut Store,
    _endpoint: &str,
    _now: OffsetDateTime,
) -> Result<bool, CpdError> {
    todo!()
}

pub fn record_rate(
    _store: &mut Store,
    _endpoint: &str,
    _remaining: i64,
    _reset_at: OffsetDateTime,
) -> Result<(), CpdError> {
    todo!()
}

pub fn pause_until(
    _store: &mut Store,
    _endpoint: &str,
    _reset_at: OffsetDateTime,
) -> Result<(), CpdError> {
    todo!()
}

pub fn is_paused(_row: &Endpoint, _now: OffsetDateTime) -> bool {
    todo!()
}
