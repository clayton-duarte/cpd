//! The one seam that records a call's result as endpoint health (swr-classifier.md §4.2).

use time::OffsetDateTime;

use crate::error::CpdError;
use crate::home::CpdHome;

/// Record `result` on `endpoint`: an answer or a `*_refused` is ok, a `*_unreachable` a transient failure, any
/// other error no record. Stub.
pub fn record_result<T>(
    _home: &CpdHome,
    _endpoint: &str,
    _now: OffsetDateTime,
    _result: &Result<T, CpdError>,
) {
    todo!("record_result")
}
