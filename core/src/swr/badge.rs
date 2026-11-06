//! `swr_badges` (docs/architecture/swr-classifier.md §2): the Lead completion badge, one row per Session.

use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
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

fn invalid(e: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::Internal,
        format!("A stored badge column is invalid: {e}."),
    )
}

fn read_row(store: &mut Store, session_id: &str) -> Result<Option<Badge>, CpdError> {
    type Raw = (String, String, Option<String>, f64, String, String, String);
    let raw: Option<Raw> = store.read(|c| {
        Ok(c.query_row(
            "SELECT entry_id, coalesce(label, ''), others_json, coalesce(score, 0), coalesce(text, ''),
                    coalesce(tone, ''), coalesce(synced_at, '')
             FROM swr_badges WHERE session_id = ?1",
            [session_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?)
    })?;
    raw.map(|(entry_id, label, others, score, text, tone, synced)| {
        Ok(Badge {
            session_id: session_id.to_owned(),
            classification: Classification {
                entry_id,
                label,
                score: score as i64,
                text,
                tone,
                others: others
                    .map(|o| serde_json::from_str(&o).map_err(invalid))
                    .transpose()?,
            },
            synced_at: OffsetDateTime::parse(&synced, &Rfc3339).map_err(invalid)?,
        })
    })
    .transpose()
}

/// The row of a Session, or `None`. Reads only.
pub fn get(store: &mut Store, session_id: &str) -> Result<Option<Badge>, CpdError> {
    read_row(store, session_id)
}

/// Store `classification`. Equal to the stored row: no write transaction, `synced_at` kept. Otherwise one
/// write replaces the row, sets `synced_at` to `now` and clears `inactive_at`.
pub fn put(
    store: &mut Store,
    now: OffsetDateTime,
    session_id: &str,
    classification: &Classification,
) -> Result<Put, CpdError> {
    if let Some(stored) = read_row(store, session_id)?
        && stored.classification == *classification
    {
        return Ok(Put {
            changed: false,
            synced_at: stored.synced_at,
        });
    }
    let others = classification
        .others
        .as_ref()
        .map(|o| o.to_string());
    store.write("swr badge put", |tx| {
        tx.execute(
            "INSERT INTO swr_badges (session_id, entry_id, label, score, text, tone, others_json, synced_at, inactive_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)
             ON CONFLICT(session_id) DO UPDATE SET entry_id = excluded.entry_id, label = excluded.label,
               score = excluded.score, text = excluded.text, tone = excluded.tone,
               others_json = excluded.others_json, synced_at = excluded.synced_at, inactive_at = NULL",
            params![
                session_id,
                classification.entry_id,
                classification.label,
                classification.score as f64,
                classification.text,
                classification.tone,
                others,
                timestamp(now)
            ],
        )?;
        Ok::<(), CpdError>(())
    })?;
    Ok(Put {
        changed: true,
        synced_at: now,
    })
}

/// True while the stored `entry_id` of the Session equals `entry_id`.
pub fn is_fresh(store: &mut Store, session_id: &str, entry_id: &str) -> Result<bool, CpdError> {
    store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM swr_badges WHERE session_id = ?1 AND entry_id = ?2)",
            params![session_id, entry_id],
            |r| r.get(0),
        )?)
    })
}

/// The Session stopped: set `inactive_at` so `gc::sweep` can delete the row after `swr.gc_days`.
pub fn mark_inactive(
    store: &mut Store,
    session_id: &str,
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    store.write("swr badge inactive", |tx| {
        tx.execute(
            "UPDATE swr_badges SET inactive_at = ?2 WHERE session_id = ?1",
            params![session_id, timestamp(now)],
        )?;
        Ok::<(), CpdError>(())
    })
}
