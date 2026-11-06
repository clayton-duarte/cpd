//! `swr_queries` (swr.md §6): commit, fail, invalidate, is_fresh, mark_inactive, touch, settle.

use std::time::Duration;
use time::OffsetDateTime;

use rusqlite::{OptionalExtension, Row, params};

use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
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

const COLUMNS: &str = "key, system, status, data_hash, fetched_at, invalidated, stale_after, last_error, error_count, inactive_at";

fn parse(text: Option<String>) -> Result<Option<OffsetDateTime>, rusqlite::Error> {
    text.map(|t| {
        OffsetDateTime::parse(&t, &time::format_description::well_known::Rfc3339).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

fn query(r: &Row<'_>) -> Result<Query, rusqlite::Error> {
    Ok(Query {
        key: r.get(0)?,
        system: r.get(1)?,
        status: r.get(2)?,
        data_hash: r.get(3)?,
        fetched_at: parse(r.get(4)?)?,
        invalidated: r.get::<_, i64>(5)? == 1,
        stale_after: parse(r.get(6)?)?,
        last_error: r.get(7)?,
        error_count: r.get(8)?,
        inactive_at: parse(r.get(9)?)?,
    })
}

/// The row of `key`, if any.
pub fn get(store: &mut Store, key: &str) -> Result<Option<Query>, CpdError> {
    store.read(|c| {
        Ok(c.query_row(
            &format!("SELECT {COLUMNS} FROM swr_queries WHERE key = ?1"),
            [key],
            query,
        )
        .optional()?)
    })
}

/// The system of a key `<system>:<kind>:<id>`.
fn system(key: &str) -> Result<&str, CpdError> {
    key.split(':')
        .next()
        .filter(|s| !s.is_empty() && key.matches(':').count() >= 2)
        .ok_or_else(|| {
            CpdError::new(
                ErrorCode::Internal,
                "A query key is `<system>:<kind>:<id>`.",
            )
            .with("key", key.to_string())
        })
}

/// The staleTime of a key (swr.md §4); `None` is never stale. An unknown kind fails loud.
fn stale_time(key: &str) -> Result<Option<Duration>, CpdError> {
    let mut parts = key.splitn(3, ':');
    let (system, kind) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    const HOUR: u64 = 3600;
    let secs = match (system, kind) {
        ("github", "pr") => 5,
        ("github", "repo" | "viewer") | ("jira", "atlassian") => 24 * HOUR,
        ("slack", "channel") => 30 * 60,
        ("models", _) => 7 * 24 * HOUR,
        _ => {
            return Err(
                CpdError::new(ErrorCode::Internal, "A query key has no staleTime.")
                    .with("key", key.to_string()),
            );
        }
    };
    Ok(Some(Duration::from_secs(secs)))
}

const SETTLE: &str = "UPDATE swr_mutations SET status = 'done'
     WHERE system = ?1 AND target_key = ?2 AND status = 'revalidating' AND completed_at <= ?3";

pub fn commit(
    store: &mut Store,
    key: &str,
    new_hash: &str,
    now: OffsetDateTime,
    poll_started_at: OffsetDateTime,
    apply: impl FnOnce(&WriteTxn<'_>) -> Result<(), CpdError>,
) -> Result<Committed, CpdError> {
    let system = system(key)?.to_string();
    let stale_after = stale_time(key)?.map(|d| timestamp(now + d));
    let started = timestamp(poll_started_at);
    let row = get(store, key)?;
    if let Some(row) = row.filter(|r| r.data_hash.as_deref() == Some(new_hash)) {
        let settles = store.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS (SELECT 1 FROM swr_mutations
                   WHERE system = ?1 AND target_key = ?2 AND status = 'revalidating' AND completed_at <= ?3)",
                params![system, key, started],
                |r| r.get::<_, bool>(0),
            )?)
        })?;
        let recovers = row.status == "error";
        if recovers || settles {
            store.write("swr commit", |tx| {
                if recovers {
                    tx.execute(
                        "UPDATE swr_queries SET status = 'success', last_error = NULL WHERE key = ?1",
                        [key],
                    )?;
                }
                tx.execute(SETTLE, params![system, key, started])?;
                Ok::<(), CpdError>(())
            })?;
        }
        return Ok(Committed::Unchanged);
    }
    store.write("swr commit", |tx| {
        apply(tx)?;
        tx.execute(
            "INSERT INTO swr_queries (key, system, status, data_hash, fetched_at, stale_after)
             VALUES (?1, ?2, 'success', ?3, ?4, ?5)
             ON CONFLICT (key) DO UPDATE SET status = 'success', data_hash = excluded.data_hash,
               fetched_at = excluded.fetched_at, stale_after = excluded.stale_after,
               invalidated = 0, last_error = NULL",
            params![key, system, new_hash, timestamp(now), stale_after],
        )?;
        tx.execute(SETTLE, params![system, key, started])?;
        Ok::<(), CpdError>(())
    })?;
    Ok(Committed::Changed)
}

pub fn fail(
    store: &mut Store,
    key: &str,
    _now: OffsetDateTime,
    error: &str,
) -> Result<(), CpdError> {
    let system = system(key)?.to_string();
    let row = get(store, key)?;
    if row.is_some_and(|r| r.status == "error" && r.last_error.as_deref() == Some(error)) {
        return Ok(());
    }
    store.write("swr fail", |tx| {
        tx.execute(
            "INSERT INTO swr_queries (key, system, status, last_error, error_count)
             VALUES (?1, ?2, 'error', ?3, 1)
             ON CONFLICT (key) DO UPDATE SET status = 'error', last_error = excluded.last_error,
               error_count = error_count + 1",
            params![key, system, error],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn invalidate(store: &mut Store, key: &str) -> Result<(), CpdError> {
    let system = system(key)?.to_string();
    store.write("swr invalidate", |tx| {
        tx.execute(
            "INSERT INTO swr_queries (key, system, invalidated) VALUES (?1, ?2, 1)
             ON CONFLICT (key) DO UPDATE SET invalidated = 1",
            params![key, system],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn is_fresh(row: &Query, now: OffsetDateTime) -> bool {
    row.stale_after.is_none_or(|at| now < at)
}

pub fn mark_inactive(store: &mut Store, key: &str, now: OffsetDateTime) -> Result<(), CpdError> {
    store.write("swr mark-inactive", |tx| {
        tx.execute(
            "UPDATE swr_queries SET inactive_at = ?2 WHERE key = ?1",
            params![key, timestamp(now)],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn touch(store: &mut Store, key: &str) -> Result<(), CpdError> {
    store.write("swr touch", |tx| {
        tx.execute(
            "UPDATE swr_queries SET inactive_at = NULL WHERE key = ?1",
            [key],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn settle(
    store: &mut Store,
    key: &str,
    _now: OffsetDateTime,
    poll_started_at: OffsetDateTime,
) -> Result<(), CpdError> {
    let system = system(key)?.to_string();
    let started = timestamp(poll_started_at);
    store.write("swr settle", |tx| {
        tx.execute(SETTLE, params![system, key, started])?;
        Ok::<(), CpdError>(())
    })
}
