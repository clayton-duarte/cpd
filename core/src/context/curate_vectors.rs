//! The vector cache `$CPD_HOME/vectors.db` of write-time curation (contract §4.6): a derived cache, not the
//! source of truth. No history, no `cpd_txn()`, no foreign keys.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::store::{cache_setup_lock, configure};

/// `PRAGMA application_id` of `vectors.db` (§4.6): "cpvc".
const APPLICATION_ID: i32 = 1_668_703_587;
/// `PRAGMA user_version` of `vectors.db`; a file with a higher one is refused.
const VERSION: i32 = 1;
/// How long the first open waits for `locks/store.lock` and for SQLite.
const BUSY_MS: u64 = 5000;

const DDL: &str = "CREATE TABLE IF NOT EXISTS context_embeddings (context TEXT NOT NULL CHECK (length(context) BETWEEN 2 AND 40), model TEXT NOT NULL CHECK (length(model) BETWEEN 1 AND 128), record TEXT NOT NULL CHECK (length(record) BETWEEN 1 AND 64), dims INTEGER NOT NULL CHECK (dims > 0), vector BLOB NOT NULL CHECK (length(vector) = dims * 4), at TEXT NOT NULL, PRIMARY KEY (context, model, record)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS context_checked (context TEXT NOT NULL, record TEXT NOT NULL, at TEXT NOT NULL, PRIMARY KEY (context, record)) STRICT, WITHOUT ROWID;";

/// The vector cache file.
pub struct VectorStore {
    home: CpdHome,
    path: PathBuf,
}

fn failed(path: &Path, what: &str, detail: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("Cannot {what} {}: {detail}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// Adds the path to a store error of the shared setup.
fn at_path(path: &Path, e: CpdError) -> CpdError {
    if e.code() == ErrorCode::StoreFailed {
        let shown = path.display().to_string();
        let message = if e.message().contains(&shown) {
            e.message().to_owned()
        } else {
            format!("{} ({shown})", e.message())
        };
        let mut out = CpdError::new(ErrorCode::StoreFailed, message);
        for (k, v) in e.details() {
            out = out.with(k, v.clone());
        }
        out.with("path", shown)
    } else {
        e
    }
}

fn check_version(conn: &Connection, path: &Path) -> Result<i32, CpdError> {
    let version: i32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| failed(path, "read", e))?;
    if version > VERSION {
        return Err(failed(
            path,
            "use",
            format!("its user_version is {version}, this cpd reads {VERSION}"),
        )
        .with("user_version", version));
    }
    Ok(version)
}

fn check_application_id(conn: &Connection, path: &Path) -> Result<(), CpdError> {
    let id: i32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(|e| failed(path, "read", e))?;
    if id != 0 && id != APPLICATION_ID {
        return Err(failed(
            path,
            "use",
            format!("its application_id is {id}, not a cpd vector cache"),
        )
        .with("application_id", id));
    }
    Ok(())
}

fn blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn unblob(path: &Path, dims: i64, bytes: &[u8]) -> Result<Vec<f32>, CpdError> {
    if dims < 0 || bytes.len() as i64 != dims * 4 {
        return Err(failed(path, "read", "a vector has the wrong length"));
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}

impl VectorStore {
    pub fn at(home: &CpdHome) -> Self {
        VectorStore {
            home: home.clone(),
            path: home.vectors_db(),
        }
    }

    /// A connection for a read, or `None` when there is no file (nothing is created) or it is still empty.
    fn reader(&self) -> Result<Option<Connection>, CpdError> {
        let path = &self.path;
        if !path.exists() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| failed(path, "open", e))?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_MS))
            .map_err(|e| failed(path, "open", e))?;
        check_application_id(&conn, path)?;
        Ok((check_version(&conn, path)? != 0).then_some(conn))
    }

    /// The connection for a write: the first open sets it up under `locks/store.lock`.
    fn writer(&self) -> Result<Connection, CpdError> {
        let path = &self.path;
        let _lock = cache_setup_lock(&self.home, BUSY_MS)?;
        let conn = Connection::open(path).map_err(|e| failed(path, "open", e))?;
        check_application_id(&conn, path)?;
        check_version(&conn, path)?;
        configure(&conn, path, BUSY_MS, APPLICATION_ID, "a cpd vector cache")
            .map_err(|e| at_path(path, e))?;
        // Always the idempotent DDL, so a file that stopped between its pragmas and its tables is completed.
        conn.execute_batch(DDL)
            .map_err(|e| failed(path, "create the tables of", e))?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|e| failed(path, "set application_id of", e))?;
        conn.pragma_update(None, "user_version", VERSION)
            .map_err(|e| failed(path, "set user_version of", e))?;
        Ok(conn)
    }

    pub fn put(
        &self,
        context: &ContextId,
        model: &str,
        record: &RecordId,
        vector: &[f32],
        at: &str,
    ) -> Result<(), CpdError> {
        if vector.is_empty() || vector.iter().any(|f| !f.is_finite()) {
            return Err(CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "Refused the vector of {record}: it is empty or has a value that is not finite."
                ),
            ));
        }
        let conn = self.writer()?;
        conn.execute(
            "INSERT OR REPLACE INTO context_embeddings (context, model, record, dims, vector, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![context.as_str(), model, record.to_string(), vector.len() as i64, blob(vector), at],
        )
        .map_err(|e| failed(&self.path, "write", e))?;
        Ok(())
    }

    pub fn get(
        &self,
        context: &ContextId,
        model: &str,
        ids: &[RecordId],
    ) -> Result<Vec<(RecordId, Vec<f32>)>, CpdError> {
        let path = &self.path;
        let Some(conn) = self.reader()? else {
            return Ok(Vec::new());
        };
        let mut stmt = conn
            .prepare("SELECT dims, vector FROM context_embeddings WHERE context = ?1 AND model = ?2 AND record = ?3")
            .map_err(|e| failed(path, "read", e))?;
        let mut out = Vec::new();
        for id in ids {
            let row = stmt.query_row(params![context.as_str(), model, id.to_string()], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            });
            match row {
                Ok((dims, bytes)) => out.push((id.clone(), unblob(path, dims, &bytes)?)),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => return Err(failed(path, "read", e)),
            }
        }
        Ok(out)
    }

    pub fn embedded(
        &self,
        context: &ContextId,
        model: &str,
    ) -> Result<HashSet<RecordId>, CpdError> {
        self.ids(
            "SELECT record FROM context_embeddings WHERE context = ?1 AND model = ?2",
            params![context.as_str(), model],
        )
    }

    pub fn mark_checked(
        &self,
        context: &ContextId,
        record: &RecordId,
        at: &str,
    ) -> Result<(), CpdError> {
        let conn = self.writer()?;
        conn.execute(
            "INSERT OR IGNORE INTO context_checked (context, record, at) VALUES (?1, ?2, ?3)",
            params![context.as_str(), record.to_string(), at],
        )
        .map_err(|e| failed(&self.path, "write", e))?;
        Ok(())
    }

    pub fn checked(&self, context: &ContextId) -> Result<HashSet<RecordId>, CpdError> {
        self.ids(
            "SELECT record FROM context_checked WHERE context = ?1",
            params![context.as_str()],
        )
    }

    fn ids(&self, sql: &str, args: impl rusqlite::Params) -> Result<HashSet<RecordId>, CpdError> {
        let path = &self.path;
        let Some(conn) = self.reader()? else {
            return Ok(HashSet::new());
        };
        let mut stmt = conn.prepare(sql).map_err(|e| failed(path, "read", e))?;
        let texts = stmt
            .query_map(args, |r| r.get::<_, String>(0))
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            .map_err(|e| failed(path, "read", e))?;
        texts
            .iter()
            .map(|t| RecordId::parse(t).map_err(|e| failed(path, "read", e.message())))
            .collect()
    }
}
