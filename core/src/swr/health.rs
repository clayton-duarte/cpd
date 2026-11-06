//! The one seam that records a call's result as endpoint health (swr-classifier.md §4.2).

use time::OffsetDateTime;

use super::endpoint::{self, Kind};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::store::Store;

/// Record `result` on `endpoint`: an answer or a `*_refused` is ok, a `*_unreachable` a transient failure, any
/// other error no record. A failed record is swallowed: health never fails the call.
pub fn record_result<T>(
    home: &CpdHome,
    endpoint: &str,
    now: OffsetDateTime,
    result: &Result<T, CpdError>,
) {
    let _ = (|| -> Result<(), CpdError> {
        match result {
            Ok(_) => endpoint::record_ok(&mut Store::open(home, 5000)?, endpoint, now),
            Err(e) => match e.code() {
                ErrorCode::ClassifierRefused
                | ErrorCode::JudgeRefused
                | ErrorCode::EmbeddingRefused => {
                    endpoint::record_ok(&mut Store::open(home, 5000)?, endpoint, now)
                }
                ErrorCode::ClassifierUnreachable
                | ErrorCode::JudgeUnreachable
                | ErrorCode::EmbeddingUnreachable => endpoint::record_failure(
                    &mut Store::open(home, 5000)?,
                    endpoint,
                    now,
                    e.message(),
                    Kind::Transient,
                ),
                _ => Ok(()),
            },
        }
    })();
}
