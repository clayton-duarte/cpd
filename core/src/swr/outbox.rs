//! `swr_mutations` (swr.md §8): the mutation outbox. Stubs until the implementing Job.

use std::time::Duration;

use serde_json::Value;
use time::OffsetDateTime;

use rusqlite::{OptionalExtension, Row, params};

use super::cache::{parse, system};
use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
use crate::store::{Store, WriteTxn};

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

const COLUMNS: &str = "id, system, target_key, op_kind, vars_json, patch_json, status, submitted_at, started_at, completed_at, next_attempt_at, attempt_count, error, context_json";

pub(super) fn mutation(r: &Row<'_>) -> Result<Mutation, rusqlite::Error> {
    Ok(Mutation {
        id: r.get(0)?,
        system: r.get(1)?,
        target_key: r.get(2)?,
        op_kind: r.get(3)?,
        vars_json: r.get(4)?,
        patch_json: r.get(5)?,
        status: r.get(6)?,
        submitted_at: parse(r.get(7)?)?.ok_or(rusqlite::Error::InvalidQuery)?,
        started_at: parse(r.get(8)?)?,
        completed_at: parse(r.get(9)?)?,
        next_attempt_at: parse(r.get(10)?)?,
        attempt_count: r.get(11)?,
        error: r.get(12)?,
        context_json: r.get(13)?,
    })
}

/// The row `id`, if any.
pub fn get(store: &mut Store, id: i64) -> Result<Option<Mutation>, CpdError> {
    store.read(|c| {
        Ok(c.query_row(
            &format!("SELECT {COLUMNS} FROM swr_mutations WHERE id = ?1"),
            [id],
            mutation,
        )
        .optional()?)
    })
}

/// The ranks of a `Forward` mutation live in `context_json` under `_rank`.
fn context_with_rank(
    context: Option<Value>,
    rank: Option<i64>,
) -> Result<Option<String>, CpdError> {
    let Some(rank) = rank else {
        return Ok(context.map(|c| c.to_string()));
    };
    let mut object = match context {
        None => serde_json::Map::new(),
        Some(Value::Object(m)) => m,
        Some(_) => {
            return Err(CpdError::new(
                ErrorCode::Internal,
                "A forward-coalesced mutation needs an object context.",
            ));
        }
    };
    object.insert("_rank".into(), Value::from(rank));
    Ok(Some(Value::Object(object).to_string()))
}

fn rank_of(context_json: Option<&str>) -> i64 {
    context_json
        .and_then(|c| serde_json::from_str::<Value>(c).ok())
        .and_then(|v| v.get("_rank").and_then(Value::as_i64))
        .unwrap_or(i64::MIN)
}

/// Insert a `pending` row, apply `coalesce`, return the id (the existing id for `Dedupe`).
pub fn enqueue(
    store: &mut Store,
    now: OffsetDateTime,
    op: Op,
    coalesce: Coalesce,
) -> Result<i64, CpdError> {
    store.write("swr enqueue", |tx| enqueue_tx(tx, now, op, coalesce))
}

/// [`enqueue`] inside the caller's transaction.
pub fn enqueue_tx(
    tx: &WriteTxn<'_>,
    now: OffsetDateTime,
    op: Op,
    coalesce: Coalesce,
) -> Result<i64, CpdError> {
    let rank = match &coalesce {
        Coalesce::Forward(r) => Some(*r),
        _ => None,
    };
    let context = context_with_rank(op.context, rank)?;
    let vars = op.vars.to_string();
    let patch = op.patch.map(|p| p.to_string());
    let (system, key, kind) = (op.system, op.target_key, op.op_kind);
    let same = "system = ?1 AND target_key = ?2 AND op_kind = ?3";
    let mut status = "pending";
    match &coalesce {
        Coalesce::LatestWins => {
            tx.execute(
                &format!("UPDATE swr_mutations SET status = 'superseded' WHERE {same} AND status = 'pending'"),
                params![system, key, kind],
            )?;
        }
        Coalesce::Forward(rank) => {
            let mut stmt = tx.prepare(&format!(
                "SELECT id, context_json FROM swr_mutations WHERE {same} AND status = 'pending'"
            ))?;
            let pending = stmt
                .query_map(params![system, key, kind], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            if pending.iter().any(|(_, c)| rank_of(c.as_deref()) >= *rank) {
                status = "superseded";
            } else {
                for (id, _) in pending {
                    tx.execute(
                        "UPDATE swr_mutations SET status = 'superseded' WHERE id = ?1",
                        [id],
                    )?;
                }
            }
        }
        Coalesce::Dedupe(canonical) => {
            let existing = tx
                .query_row(
                    &format!("SELECT id FROM swr_mutations WHERE {same} AND vars_json = ?4
                       AND status IN ('pending', 'in_flight', 'revalidating') ORDER BY id LIMIT 1"),
                    params![system, key, kind, canonical],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(id) = existing {
                return Ok(id);
            }
        }
    }
    tx.execute(
        "INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, patch_json, status, submitted_at, context_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![system, key, kind, vars, patch, status, timestamp(now), context],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Take the oldest due `pending` row of `system`: `in_flight`, `started_at`, `attempt_count` + 1.
/// A row waits while another of its key is `in_flight`.
pub fn claim_next(
    store: &mut Store,
    now: OffsetDateTime,
    system: &str,
) -> Result<Option<Mutation>, CpdError> {
    let at = timestamp(now);
    store.write("swr claim", |tx| {
        let id = tx
            .query_row(
                "SELECT m.id FROM swr_mutations m
                 WHERE m.system = ?1 AND m.status = 'pending' AND m.target_key IS NOT NULL
                   AND (m.next_attempt_at IS NULL OR m.next_attempt_at <= ?2)
                   AND NOT EXISTS (SELECT 1 FROM swr_mutations f WHERE f.system = m.system
                     AND f.target_key = m.target_key AND f.op_kind = m.op_kind AND f.status = 'in_flight')
                 ORDER BY m.submitted_at, m.id LIMIT 1",
                params![system, at],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(None) };
        tx.execute(
            "UPDATE swr_mutations SET status = 'in_flight', started_at = ?2, next_attempt_at = NULL,
               attempt_count = attempt_count + 1 WHERE id = ?1",
            params![id, at],
        )?;
        Ok::<_, CpdError>(Some(tx.query_row(
            &format!("SELECT {COLUMNS} FROM swr_mutations WHERE id = ?1"),
            [id],
            mutation,
        )?))
    })
}

/// `revalidating`, `completed_at` = `now`, and invalidate the target key.
pub fn complete(store: &mut Store, id: i64, now: OffsetDateTime) -> Result<(), CpdError> {
    let m = get(store, id)?
        .ok_or_else(|| CpdError::new(ErrorCode::Internal, "No such mutation.").with("id", id))?;
    let system = system(&m.target_key)?.to_string();
    store.write("swr complete", |tx| {
        tx.execute(
            "UPDATE swr_mutations SET status = 'revalidating', completed_at = ?2 WHERE id = ?1",
            params![id, timestamp(now)],
        )?;
        tx.execute(
            "INSERT INTO swr_queries (key, system, invalidated) VALUES (?1, ?2, 1)
             ON CONFLICT (key) DO UPDATE SET invalidated = 1",
            params![m.target_key, system],
        )?;
        Ok::<(), CpdError>(())
    })
}

/// `After(d)`: back to `pending` at `now + d`. `Never`: `failed`.
pub fn fail(
    store: &mut Store,
    id: i64,
    now: OffsetDateTime,
    error: &str,
    retry: Retry,
) -> Result<(), CpdError> {
    store.write("swr fail mutation", |tx| {
        match retry {
            Retry::After(d) => tx.execute(
                "UPDATE swr_mutations SET status = 'pending', error = ?2, next_attempt_at = ?3 WHERE id = ?1",
                params![id, error, timestamp(now + d)],
            )?,
            Retry::Never => tx.execute(
                "UPDATE swr_mutations SET status = 'failed', error = ?2, completed_at = ?3 WHERE id = ?1",
                params![id, error, timestamp(now)],
            )?,
        };
        Ok::<(), CpdError>(())
    })
}
