//! `swr_queries` (swr.md §6): commit, fail, invalidate, is_fresh, mark_inactive, touch, settle.

use time::OffsetDateTime;

use crate::error::CpdError;
use crate::store::{Store, WriteTxn};

/// What `commit` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Committed {
    Changed,
    Unchanged,
}

/// A row of `swr_queries` (swr.md §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub key: String,
    pub system: String,
    pub status: String,
    pub data_hash: Option<String>,
    pub fetched_at: Option<OffsetDateTime>,
    pub invalidated: bool,
    /// `None` means never stale (Infinity).
    pub stale_after: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub error_count: i64,
    pub inactive_at: Option<OffsetDateTime>,
}

/// The row of `key`, if any.
pub fn get(_store: &mut Store, _key: &str) -> Result<Option<Query>, CpdError> {
    todo!()
}

pub fn commit(
    _store: &mut Store,
    _key: &str,
    _new_hash: &str,
    _now: OffsetDateTime,
    _poll_started_at: OffsetDateTime,
    _apply: impl FnOnce(&WriteTxn<'_>) -> Result<(), CpdError>,
) -> Result<Committed, CpdError> {
    todo!()
}

pub fn fail(
    _store: &mut Store,
    _key: &str,
    _now: OffsetDateTime,
    _error: &str,
) -> Result<(), CpdError> {
    todo!()
}

pub fn invalidate(_store: &mut Store, _key: &str) -> Result<(), CpdError> {
    todo!()
}

pub fn is_fresh(_row: &Query, _now: OffsetDateTime) -> bool {
    todo!()
}

pub fn mark_inactive(_store: &mut Store, _key: &str, _now: OffsetDateTime) -> Result<(), CpdError> {
    todo!()
}

pub fn touch(_store: &mut Store, _key: &str) -> Result<(), CpdError> {
    todo!()
}

pub fn settle(
    _store: &mut Store,
    _key: &str,
    _now: OffsetDateTime,
    _poll_started_at: OffsetDateTime,
) -> Result<(), CpdError> {
    todo!()
}
