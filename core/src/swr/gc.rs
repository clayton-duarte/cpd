//! Eviction (swr.md §4): rows inactive past gcTime with no observer. Stub until the implementing Job.

use std::time::Duration;

use time::OffsetDateTime;

use crate::error::CpdError;
use crate::store::Store;

/// Delete the `swr_queries` rows whose `inactive_at` is older than `ttl` (`swr.gc_days`) and that have
/// no observer: an active mutation on the key, an open or waiting Workflow whose pull request is the
/// key `github:pr:<repo>#<number>`, or an open Inbox item whose `subject` is the key. Returns the
/// number of rows deleted.
pub fn sweep(_store: &mut Store, _now: OffsetDateTime, _ttl: Duration) -> Result<usize, CpdError> {
    todo!("swr gc sweep")
}
