//! The in-memory FTS5 search seam: ranks `Doc`s for a typed query and drops the weak tail.

use rusqlite::{Connection, params};

use super::query;
use crate::error::{CpdError, ErrorCode};

/// One searchable record: its id and the curated topic and short ("" when uncurated) and the text.
pub(super) struct Doc {
    pub id: String,
    pub topic: String,
    pub short: String,
    pub text: String,
}

/// A ranked hit: higher `score` is better.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Scored {
    pub id: String,
    pub score: f64,
}

/// A hit scoring below `FLOOR` times the top score is dropped.
pub(super) const FLOOR: f64 = 0.35;

/// Ranks `docs` for `typed`, best first: bm25 over an in-memory FTS5 table, the id column weighing most.
/// A doc whose id is the query comes first.
pub(super) fn rank(docs: &[Doc], typed: &str) -> Result<Vec<Scored>, CpdError> {
    if docs.is_empty() {
        return Ok(Vec::new());
    }
    let mut conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE VIRTUAL TABLE fts USING fts5(id, topic, short, text, tokenize = 'porter unicode61')",
    )?;
    let tx = conn.transaction()?;
    {
        let mut insert =
            tx.prepare("INSERT INTO fts (id, topic, short, text) VALUES (?1, ?2, ?3, ?4)")?;
        for doc in docs {
            insert.execute(params![doc.id, doc.topic, doc.short, doc.text])?;
        }
    }
    tx.commit()?;
    let usage = |e: rusqlite::Error| match e {
        rusqlite::Error::SqliteFailure(_, Some(message)) => CpdError::new(
            ErrorCode::Usage,
            format!("The search query is malformed: {message}"),
        )
        .with("query", typed)
        .with("message", message),
        other => other.into(),
    };
    let mut stmt = conn.prepare(
        "SELECT id, -bm25(fts, 10.0, 2.0, 2.0, 1.0) FROM fts WHERE fts MATCH ?1 \
         ORDER BY bm25(fts, 10.0, 2.0, 2.0, 1.0), rowid",
    )?;
    let mut ranked = stmt
        .query_map([query::sanitise(typed)], |r| {
            Ok(Scored {
                id: r.get(0)?,
                score: r.get(1)?,
            })
        })
        .map_err(usage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(usage)?;
    let wanted = typed.trim();
    if let Some(doc) = docs.iter().find(|d| d.id.eq_ignore_ascii_case(wanted)) {
        let top = ranked.iter().map(|s| s.score).fold(0.0_f64, f64::max);
        let own = ranked
            .iter()
            .position(|s| s.id == doc.id)
            .map(|i| ranked.remove(i).score)
            .unwrap_or(0.0);
        ranked.insert(
            0,
            Scored {
                id: doc.id.clone(),
                score: own.max(top),
            },
        );
    }
    Ok(ranked)
}

/// Drops the weak tail of `ranked` (below `FLOOR` times the top score), keeping the `exempt` id.
pub(super) fn floor(ranked: Vec<Scored>, exempt: Option<&str>) -> Vec<Scored> {
    let Some(top) = ranked.first().map(|s| s.score).filter(|top| *top > 0.0) else {
        return ranked;
    };
    ranked
        .into_iter()
        .filter(|s| s.score >= FLOOR * top || exempt == Some(s.id.as_str()))
        .collect()
}
