//! `swr_endpoints` (swr.md §7): endpoint health.

use std::time::Duration;

use time::OffsetDateTime;

use rusqlite::{OptionalExtension, Row, params};

use crate::clock::timestamp;
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

const COLUMNS: &str = "endpoint, status, fetch_status, paused_until, failing_since, reported_at, last_error, error_count, last_ok_at, rate_remaining, rate_reset_at";

/// Report an outage this long after its first transient failure.
const OUTAGE: Duration = Duration::from_secs(600);

fn parse(text: Option<String>) -> Result<Option<OffsetDateTime>, rusqlite::Error> {
    text.map(|t| {
        OffsetDateTime::parse(&t, &time::format_description::well_known::Rfc3339).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

fn endpoint(r: &Row<'_>) -> Result<Endpoint, rusqlite::Error> {
    Ok(Endpoint {
        endpoint: r.get(0)?,
        status: r.get(1)?,
        fetch_status: r.get(2)?,
        paused_until: parse(r.get(3)?)?,
        failing_since: parse(r.get(4)?)?,
        reported_at: parse(r.get(5)?)?,
        last_error: r.get(6)?,
        error_count: r.get(7)?,
        last_ok_at: parse(r.get(8)?)?,
        rate_remaining: r.get(9)?,
        rate_reset_at: parse(r.get(10)?)?,
    })
}

/// The row of `endpoint`, if any.
pub fn get(store: &mut Store, name: &str) -> Result<Option<Endpoint>, CpdError> {
    store.read(|c| {
        Ok(c.query_row(
            &format!("SELECT {COLUMNS} FROM swr_endpoints WHERE endpoint = ?1"),
            [name],
            endpoint,
        )
        .optional()?)
    })
}

const ENSURE: &str = "INSERT OR IGNORE INTO swr_endpoints (endpoint) VALUES (?1)";

pub fn record_ok(store: &mut Store, name: &str, now: OffsetDateTime) -> Result<(), CpdError> {
    let now = timestamp(now);
    store.write("swr endpoint ok", |tx| {
        tx.execute(ENSURE, [name])?;
        tx.execute(
            "UPDATE swr_endpoints SET failing_since = NULL, reported_at = NULL, last_error = NULL, error_count = 0,
               last_ok_at = ?2, status = 'success',
               fetch_status = CASE WHEN fetch_status = 'paused' AND paused_until > ?2 THEN 'paused' ELSE 'idle' END,
               paused_until = CASE WHEN fetch_status = 'paused' AND paused_until > ?2 THEN paused_until ELSE NULL END
             WHERE endpoint = ?1",
            params![name, now],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn record_failure(
    store: &mut Store,
    name: &str,
    now: OffsetDateTime,
    error: &str,
    kind: Kind,
) -> Result<(), CpdError> {
    // A permanent failure is due at once: its outage is dated as old as a report needs, since the table has no
    // column for the kind. The earliest instant wins, so a transient outage that turns permanent is due too.
    let since = timestamp(match kind {
        Kind::Transient => now,
        Kind::Permanent => now - OUTAGE,
    });
    store.write("swr endpoint failure", |tx| {
        tx.execute(ENSURE, [name])?;
        tx.execute(
            "UPDATE swr_endpoints SET status = 'error', last_error = ?2, error_count = error_count + 1,
               failing_since = CASE WHEN failing_since IS NULL OR failing_since > ?3 THEN ?3 ELSE failing_since END
             WHERE endpoint = ?1",
            params![name, error, since],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn due_report(store: &mut Store, name: &str, now: OffsetDateTime) -> Result<bool, CpdError> {
    let due = match get(store, name)? {
        Some(r) if r.reported_at.is_none() => r.failing_since.is_some_and(|f| now - f >= OUTAGE),
        _ => false,
    };
    if due {
        let at = timestamp(now);
        store.write("swr endpoint report", |tx| {
            tx.execute(
                "UPDATE swr_endpoints SET reported_at = ?2 WHERE endpoint = ?1",
                params![name, at],
            )?;
            Ok::<(), CpdError>(())
        })?;
    }
    Ok(due)
}

pub fn pause_until(
    store: &mut Store,
    name: &str,
    reset_at: OffsetDateTime,
) -> Result<(), CpdError> {
    let reset = timestamp(reset_at);
    store.write("swr endpoint pause", |tx| {
        tx.execute(ENSURE, [name])?;
        tx.execute(
            "UPDATE swr_endpoints SET fetch_status = 'paused', paused_until = ?2 WHERE endpoint = ?1",
            params![name, reset],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn record_rate(
    store: &mut Store,
    name: &str,
    remaining: i64,
    reset_at: OffsetDateTime,
) -> Result<(), CpdError> {
    let reset = timestamp(reset_at);
    store.write("swr endpoint rate", |tx| {
        tx.execute(ENSURE, [name])?;
        tx.execute(
            "UPDATE swr_endpoints SET rate_remaining = ?2, rate_reset_at = ?3,
               fetch_status = CASE WHEN ?2 = 0 THEN 'paused' ELSE fetch_status END,
               paused_until = CASE WHEN ?2 = 0 THEN ?3 ELSE paused_until END
             WHERE endpoint = ?1",
            params![name, remaining, reset],
        )?;
        Ok::<(), CpdError>(())
    })
}

pub fn is_paused(row: &Endpoint, now: OffsetDateTime) -> bool {
    row.fetch_status == "paused" && row.paused_until.is_some_and(|until| now < until)
}
