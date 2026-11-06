//! `swr_badges` (docs/architecture/swr-classifier.md §2): the Lead completion badge, one row per Session.

use serde_json::Value;
use time::OffsetDateTime;

use crate::error::CpdError;
use crate::store::Store;

/// What the classifier said about a Session at `entry_id` (the Session's last transcript item id).
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub entry_id: String,
    pub label: String,
    /// An integer from 0 to 100.
    pub score: i64,
    pub text: String,
    pub tone: String,
    /// The `[{"label","score"}, …]` array; `None` is stored as NULL.
    pub others: Option<Value>,
}

/// A stored row.
#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    pub session_id: String,
    pub classification: Classification,
    pub synced_at: OffsetDateTime,
}

/// What `put` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Put {
    pub changed: bool,
    /// The stored `synced_at`: `now` when changed, the kept one when not.
    pub synced_at: OffsetDateTime,
}

/// The row of a Session, or `None`. Reads only.
pub fn get(_store: &mut Store, _session_id: &str) -> Result<Option<Badge>, CpdError> {
    todo!("swr badge get")
}

/// Store `classification`. Equal to the stored row: no write transaction, `synced_at` kept. Otherwise one
/// write replaces the row, sets `synced_at` to `now` and clears `inactive_at`.
pub fn put(
    _store: &mut Store,
    _now: OffsetDateTime,
    _session_id: &str,
    _classification: &Classification,
) -> Result<Put, CpdError> {
    todo!("swr badge put")
}

/// True while the stored `entry_id` of the Session equals `entry_id`.
pub fn is_fresh(_store: &mut Store, _session_id: &str, _entry_id: &str) -> Result<bool, CpdError> {
    todo!("swr badge is_fresh")
}

/// The Session stopped: set `inactive_at` so `gc::sweep` can delete the row after `swr.gc_days`.
pub fn mark_inactive(
    _store: &mut Store,
    _session_id: &str,
    _now: OffsetDateTime,
) -> Result<(), CpdError> {
    todo!("swr badge mark_inactive")
}
