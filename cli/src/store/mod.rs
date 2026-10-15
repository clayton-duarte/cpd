//! The SQLite store (contract §4.1–§4.5): opening `$CPD_HOME/cpd.db`, read and
//! write transactions, `txns` and `cpd_txn()`, the DDL, migrations, the schema
//! check and the store's part of the tamper report.
//!
//! Every module reads through [`Store::read`] and writes through
//! [`Store::write`]; nothing else writes the database (§4.3).

mod schema;

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, ErrorCode as SqliteCode, Transaction, TransactionBehavior};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

pub use schema::{HISTORY_TABLES, VERSION, schema_hash};

/// `PRAGMA application_id` of a cpd 2 database: `0x63706432`, "cpd2" (§4.1).
pub const APPLICATION_ID: i32 = 0x6370_6432;

/// The `txns.command` of the transaction that migrates (§4.5).
pub const MIGRATE_COMMAND: &str = "migrate";

/// An open database: one connection, set up as §4.1 says.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    busy_ms: u64,
    /// What `cpd_txn()` returns: the open write transaction's id, 0 when none.
    txn: Arc<AtomicI64>,
    schema: Option<SchemaMismatch>,
}

/// The schema check failed (§4.3): `sqlite_schema` does not hash to what this
/// binary embeds for its version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaMismatch {
    pub expected: String,
    pub found: String,
}

/// The open write transaction handed to [`Store::write`]'s closure. It derefs
/// to the connection; the store commits or rolls it back.
pub struct WriteTxn<'a> {
    tx: Transaction<'a>,
    id: i64,
}

impl WriteTxn<'_> {
    /// This transaction's `txns.id`, also what `cpd_txn()` returns.
    pub fn id(&self) -> i64 {
        self.id
    }
}

impl Deref for WriteTxn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.tx
    }
}

/// Clears `cpd_txn()` when a write transaction ends, however it ends.
struct TxnGuard<'a>(&'a AtomicI64);

impl Drop for TxnGuard<'_> {
    fn drop(&mut self) {
        self.0.store(0, Ordering::SeqCst);
    }
}

impl Store {
    /// Opens `$CPD_HOME/cpd.db`, creating it and `$CPD_HOME` on first use, sets
    /// up the connection (§4.1), migrates (§4.5) and runs the schema check
    /// (§4.3). `busy_ms` is the config's `store_busy_ms`.
    ///
    /// A failed schema check does not fail the open: [`Store::write`] refuses
    /// with `store_tampered` and [`Store::tamper`] reports it.
    pub fn open(home: &CpdHome, busy_ms: u64) -> Result<Store, CpdError> {
        let path = home.db();
        fs::create_dir_all(home.root()).map_err(|e| {
            CpdError::new(ErrorCode::StoreFailed, format!("Cannot create {}: {e}.", home.root().display()))
                .with("path", home.root().display().to_string())
        })?;
        let mut store = Store::connect(&path, busy_ms).map_err(|e| enrich(e, &path, busy_ms))?;
        store.migrate().map_err(|e| enrich(e, &path, busy_ms))?;
        store.schema = store.check_schema().map_err(|e| enrich(e, &path, busy_ms))?;
        Ok(store)
    }

    fn connect(path: &Path, busy_ms: u64) -> Result<Store, CpdError> {
        let conn = Connection::open(path)?;
        let txn = Arc::new(AtomicI64::new(0));
        let current = Arc::clone(&txn);
        // Not SQLITE_DIRECTONLY: the triggers call it (§4.1).
        conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| {
            match current.load(Ordering::SeqCst) {
                0 => Err(rusqlite::Error::UserFunctionError(
                    "cpd_txn() called outside a cpd write transaction".into(),
                )),
                id => Ok(id),
            }
        })?;
        conn.busy_timeout(Duration::from_millis(busy_ms))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "FULL")?;

        let application_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if application_id != 0 && application_id != APPLICATION_ID {
            return Err(CpdError::new(
                ErrorCode::StoreFailed,
                format!("{} is not a cpd 2 database: its application_id is {application_id}.", path.display()),
            )
            .with("application_id", application_id)
            .with("expected", APPLICATION_ID));
        }
        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(CpdError::new(
                    ErrorCode::StoreFailed,
                    format!("Cannot switch {} to WAL: journal_mode stayed {mode}.", path.display()),
                )
                .with("journal_mode", mode));
            }
        }
        Ok(Store { conn, path: path.to_owned(), busy_ms, txn, schema: None })
    }

    /// §4.5: below N, migrate in one `BEGIN IMMEDIATE` transaction; above N, refuse.
    fn migrate(&mut self) -> Result<(), CpdError> {
        let version = user_version(&self.conn)?;
        refuse_too_new(version)?;
        if version == VERSION {
            return Ok(());
        }
        let caller = Caller::current()?;
        let _guard = TxnGuard(&self.txn);
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Another process may have migrated since the read above.
        let version = user_version(&tx)?;
        refuse_too_new(version)?;
        if version == VERSION {
            return Ok(());
        }
        let failed = |to: i64, e: rusqlite::Error| {
            CpdError::new(ErrorCode::MigrationFailed, format!("Migration {to} of the database failed: {e}."))
                .with("from", version)
                .with("to", to)
                .with("sqlite_message", e.to_string())
        };
        if version == 0 {
            tx.pragma_update(None, "application_id", APPLICATION_ID).map_err(|e| failed(1, e))?;
        }
        for (index, migration) in schema::MIGRATIONS.iter().enumerate().skip(version as usize) {
            migration(&tx).map_err(|e| failed(index as i64 + 1, e))?;
        }
        begin_txn(&tx, &self.txn, MIGRATE_COMMAND, &caller).map_err(|e| failed(VERSION, e))?;
        tx.pragma_update(None, "user_version", VERSION).map_err(|e| failed(VERSION, e))?;
        tx.commit()?;
        Ok(())
    }

    fn check_schema(&self) -> Result<Option<SchemaMismatch>, CpdError> {
        let expected = schema::SCHEMA_HASHES[VERSION as usize - 1];
        let found = schema_hash(&self.conn)?;
        Ok((found != expected).then(|| SchemaMismatch { expected: expected.to_owned(), found }))
    }

    /// The path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The schema check's result at open (§4.3); `None` when it passed.
    pub fn schema_mismatch(&self) -> Option<&SchemaMismatch> {
        self.schema.as_ref()
    }

    /// Runs `f` in a deferred read transaction on the WAL snapshot (§4.2): it
    /// never waits for a writer. Keep it short: one query or one command's reads.
    /// Writes inside it fail: `cpd_txn()` refuses outside [`Store::write`].
    pub fn read<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T, CpdError>) -> Result<T, CpdError> {
        let (path, busy_ms) = (self.path.clone(), self.busy_ms);
        let result = (|| {
            let tx = self.conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
            let value = f(&tx)?;
            tx.commit()?;
            Ok(value)
        })();
        result.map_err(|e| enrich(e, &path, busy_ms))
    }

    /// Runs `f` in one write transaction (§4.2, §4.3): `BEGIN IMMEDIATE`
    /// (`store_busy` after `store_busy_ms`), then the `txns` row for `command`
    /// with `cpd_txn()` set to its id, then `f`. Commits when `f` returns `Ok`,
    /// rolls everything back when it returns `Err`. Refuses with
    /// `store_tampered` when the schema check failed.
    ///
    /// No external work (git, tmux, pi, `gh`) belongs inside `f`.
    pub fn write<T>(
        &mut self,
        command: &str,
        f: impl FnOnce(&WriteTxn<'_>) -> Result<T, CpdError>,
    ) -> Result<T, CpdError> {
        if let Some(mismatch) = &self.schema {
            return Err(CpdError::new(
                ErrorCode::StoreTampered,
                "The database schema does not match this cpd2's: a table or trigger was changed outside cpd.",
            )
            .with("path", self.path.display().to_string())
            .with("expected", mismatch.expected.clone())
            .with("found", mismatch.found.clone()));
        }
        let caller = Caller::current()?;
        let (path, busy_ms) = (self.path.clone(), self.busy_ms);
        let _guard = TxnGuard(&self.txn);
        let result = (|| {
            let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let id = begin_txn(&tx, &self.txn, command, &caller)?;
            let write = WriteTxn { tx, id };
            let value = f(&write)?;
            write.tx.commit()?;
            Ok(value)
        })();
        result.map_err(|e| enrich(e, &path, busy_ms))
    }

    /// The store's part of the tamper report (§4.3): the schema check, and every
    /// `history` row whose `txn` has no `txns` row. Each entry is an object
    /// with `kind` (`"schema"` or `"history"`). The Locked-Job fingerprints
    /// (§2.3) are checked by the Job area.
    pub fn tamper(&mut self) -> Result<Vec<Value>, CpdError> {
        let mut report = Vec::new();
        if let Some(m) = &self.schema {
            report.push(json!({"kind": "schema", "expected": m.expected, "found": m.found}));
        }
        let orphans = self.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT h.id, h.txn FROM history h WHERE NOT EXISTS (SELECT 1 FROM txns t WHERE t.id = h.txn) ORDER BY h.id",
            )?;
            let rows = stmt
                .query_map([], |r| Ok(json!({"kind": "history", "id": r.get::<_, i64>(0)?, "txn": r.get::<_, i64>(1)?})))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        report.extend(orphans);
        Ok(report)
    }
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
}

fn refuse_too_new(version: i64) -> Result<(), CpdError> {
    if version > VERSION {
        return Err(CpdError::new(
            ErrorCode::StoreTooNew,
            format!("The database has schema version {version}, newer than this cpd2's {VERSION}: upgrade cpd2."),
        )
        .with("version", version)
        .with("supported", VERSION));
    }
    Ok(())
}

/// Who opens a write transaction: `txns.pid`, `session` and `cwd` (§4.4).
struct Caller {
    pid: u32,
    session: Option<String>,
    cwd: String,
}

impl Caller {
    fn current() -> Result<Caller, CpdError> {
        let session = ["CPD_SESSION", "PI_SESSION_ID"]
            .iter()
            .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
            .map(|v| v.to_string_lossy().into_owned());
        let cwd = std::env::current_dir().map_err(|e| {
            CpdError::new(ErrorCode::StoreFailed, format!("The current directory is unreadable: {e}."))
        })?;
        Ok(Caller { pid: std::process::id(), session, cwd: cwd.to_string_lossy().into_owned() })
    }
}

/// Inserts this transaction's `txns` row with the id `max(id) + 1`, after
/// setting `cpd_txn()` to it (§4.3). Returns the id.
fn begin_txn(conn: &Connection, current: &AtomicI64, command: &str, caller: &Caller) -> rusqlite::Result<i64> {
    let id: i64 = conn.query_row("SELECT coalesce(max(id), 0) + 1 FROM txns", [], |r| r.get(0))?;
    current.store(id, Ordering::SeqCst);
    conn.execute(
        "INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, now(), command, caller.pid, caller.session, caller.cwd],
    )?;
    Ok(id)
}

/// Now, as §1.3 writes times: RFC 3339 UTC with milliseconds.
fn now() -> String {
    OffsetDateTime::now_utc()
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .expect("a UTC time always formats")
}

/// Adds the database's path, and `busy_ms` to `store_busy`, to errors made
/// from a `rusqlite::Error`, which cannot know them.
fn enrich(e: CpdError, path: &Path, busy_ms: u64) -> CpdError {
    match e.code() {
        ErrorCode::StoreFailed | ErrorCode::StoreBusy if !e.details().contains_key("path") => {
            let e = e.with("path", path.display().to_string());
            if e.code() == ErrorCode::StoreBusy { e.with("busy_ms", busy_ms) } else { e }
        }
        _ => e,
    }
}

/// A SQLite failure, as Escalation `rs-store-a1-q1` decided: busy after the
/// busy timeout is `store_busy`; the triggers' `RAISE(ABORT, 'job_locked')`
/// and `'illegal_transition'` are those refusals; anything else is
/// `store_failed` with `sqlite_code` and `sqlite_message`.
impl From<rusqlite::Error> for CpdError {
    fn from(e: rusqlite::Error) -> CpdError {
        let message = e.to_string();
        let sqlite = match &e {
            rusqlite::Error::SqliteFailure(failure, _) => Some(*failure),
            _ => None,
        };
        match sqlite {
            Some(f) if f.code == SqliteCode::DatabaseBusy => CpdError::new(
                ErrorCode::StoreBusy,
                "Another cpd2 process held the database's write lock for longer than store_busy_ms.",
            )
            .with("sqlite_message", message),
            Some(f) if f.code == SqliteCode::ConstraintViolation && message == "job_locked" => {
                CpdError::new(ErrorCode::JobLocked, "The Job is Locked: its Spec and Dependencies cannot change.")
                    .with("sqlite_message", message)
            }
            Some(f) if f.code == SqliteCode::ConstraintViolation && message == "illegal_transition" => {
                CpdError::new(ErrorCode::IllegalTransition, "The state change is not a legal transition (§3).")
                    .with("sqlite_message", message)
            }
            _ => CpdError::new(ErrorCode::StoreFailed, format!("The database failed: {message}."))
                .with("sqlite_code", sqlite.map_or(Value::Null, |f| Value::from(f.extended_code)))
                .with("sqlite_message", message),
        }
    }
}
