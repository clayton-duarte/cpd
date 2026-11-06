//! `swr_mutations` (swr.md §8): the mutation outbox. Stubs until the implementing Job.

use std::time::Duration;

use serde_json::Value;
use time::OffsetDateTime;

use crate::error::CpdError;
use crate::store::Store;

/// A write to enqueue (swr.md §8).
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    pub system: String,
    pub target_key: String,
    pub op_kind: String,
    pub vars: Value,
    pub patch: Option<Value>,
    pub context: Option<Value>,
}

/// How `enqueue` coalesces with the active mutations of the same key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coalesce {
    LatestWins,
    /// A higher rank is further along; the furthest target wins.
    Forward(i64),
    /// The canonical `vars_json` of the op; an active equal one is returned instead of a new row.
    Dedupe(String),
}

/// What `fail` does with the mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    After(Duration),
    Never,
}

/// A row of `swr_mutations` (swr.md §3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutation {
    pub id: i64,
    pub system: String,
    pub target_key: String,
    pub op_kind: String,
    pub vars_json: String,
    pub patch_json: Option<String>,
    pub status: String,
    pub submitted_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub next_attempt_at: Option<OffsetDateTime>,
    pub attempt_count: i64,
    pub error: Option<String>,
    pub context_json: Option<String>,
}

/// The row `id`, if any.
pub fn get(_store: &mut Store, _id: i64) -> Result<Option<Mutation>, CpdError> {
    todo!("swr outbox get")
}

/// Insert a `pending` row, apply `coalesce`, return the id (the existing id for `Dedupe`).
pub fn enqueue(
    _store: &mut Store,
    _now: OffsetDateTime,
    _op: Op,
    _coalesce: Coalesce,
) -> Result<i64, CpdError> {
    todo!("swr outbox enqueue")
}

/// Take the oldest due `pending` row of `system`: `in_flight`, `started_at`, `attempt_count` + 1.
pub fn claim_next(
    _store: &mut Store,
    _now: OffsetDateTime,
    _system: &str,
) -> Result<Option<Mutation>, CpdError> {
    todo!("swr outbox claim_next")
}

/// `revalidating`, `completed_at` = `now`, and invalidate the target key.
pub fn complete(_store: &mut Store, _id: i64, _now: OffsetDateTime) -> Result<(), CpdError> {
    todo!("swr outbox complete")
}

/// `After(d)`: back to `pending` at `now + d`. `Never`: `failed`.
pub fn fail(
    _store: &mut Store,
    _id: i64,
    _now: OffsetDateTime,
    _error: &str,
    _retry: Retry,
) -> Result<(), CpdError> {
    todo!("swr outbox fail")
}
