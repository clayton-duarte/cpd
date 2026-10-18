//! The SQLite store (contract §4.1–§4.5): opening `$CPD_HOME/cpd.db`, read and
//! write transactions, `txns` and `cpd_txn()`, the DDL, migrations, the schema
//! check and the store's part of the tamper report.
//!
//! Every module reads through [`Store::read`] and writes through
//! [`Store::write`]; nothing else writes the database (§4.3).

mod schema;
#[doc(hidden)] pub mod testing;
mod verdict;

use std::fs::{self, File, OpenOptions};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, ErrorCode as SqliteCode, OpenFlags, Transaction, TransactionBehavior};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::context::legacy;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

use schema::{LEGACY, Named};
pub use schema::{SCHEMA_HASH, VERSION, canonical_hash};
use verdict::verdict_with;
pub use verdict::{Start, Verdict, refuse_user_version, verdict};

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

/// The schema check failed (§4.3): the canonical hash of `sqlite_schema` is not
/// the one this binary pins, or a generated history trigger is not what the
/// generator makes. For a hash `expected` and `found` are hashes and `object` is
/// `None`; for a trigger `object` is its name and they are its SQL (`found` is
/// `"missing"` when the store lacks it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaMismatch {
    pub expected: String,
    pub found: String,
    pub object: Option<String>,
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
    /// with `store_tampered` and [`Store::tamper`] reports it. A legacy store
    /// that fails its legacy schema check is not converted: the open fails.
    ///
    /// The one-time setup (the WAL switch and the migration) runs under
    /// `locks/store.lock` (§1.4): SQLite answers concurrent switches to WAL of
    /// a new database with `SQLITE_BUSY` at once, without its busy handler.
    pub fn open(home: &CpdHome, busy_ms: u64) -> Result<Store, CpdError> {
        Store::open_with(home, busy_ms, &Catalog::BUILT_IN)
    }

    /// [`Store::open`] with the migrations and the pinned hash of `catalog`:
    /// the tests' way to run migrations this binary does not have.
    fn open_with(home: &CpdHome, busy_ms: u64, catalog: &Catalog<'_>) -> Result<Store, CpdError> {
        let path = home.db();
        let setup = setup_lock(home)?;
        let mut store = Store::connect(&path, busy_ms).map_err(|e| enrich(e, &path, busy_ms))?;
        store.migrate(home, catalog).map_err(|e| enrich(e, &path, busy_ms))?;
        drop(setup);
        store.schema = store.check_schema(home, catalog).map_err(|e| enrich(e, &path, busy_ms))?;
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

    /// §4.5: runs what [`plan`] says, in one `BEGIN IMMEDIATE` transaction.
    ///
    /// The transaction runs with foreign keys off and `legacy_alter_table` on
    /// (§4.5 migration 4, step 1: both only take effect outside a
    /// transaction), so a migration can rebuild a table that others reference;
    /// `PRAGMA foreign_key_check` then holds before the commit. Both are
    /// restored afterwards, whether the migration committed or not.
    ///
    /// When the migration that imports the old Context stores (§4.5 legacy
    /// migration 0007) committed, the folders are moved to `archive/contexts/` (§1.4).
    fn migrate(&mut self, home: &CpdHome, catalog: &Catalog<'_>) -> Result<(), CpdError> {
        if verdict_with(&self.conn, catalog)?.into_plan()?.is_none() {
            return Ok(());
        }
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        self.conn.pragma_update(None, "legacy_alter_table", "ON")?;
        let migrated = self.migrate_in_txn(home, catalog);
        let restored = self
            .conn
            .pragma_update(None, "legacy_alter_table", "OFF")
            .and_then(|()| self.conn.pragma_update(None, "foreign_keys", "ON"));
        let imported_old_stores = migrated?;
        restored?;
        if imported_old_stores {
            archive_old_stores(home)?;
        }
        Ok(())
    }

    /// Applies what the verdict plans in one transaction. Returns whether the
    /// legacy migration that imports the old Context stores was among them.
    fn migrate_in_txn(&mut self, home: &CpdHome, catalog: &Catalog<'_>) -> Result<bool, CpdError> {
        let caller = Caller::current();
        let path = self.path.display().to_string();
        let _guard = TxnGuard(&self.txn);
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Another process may have migrated since the read in `migrate`.
        let Some(plan) = verdict_with(&tx, catalog)?.into_plan()? else { return Ok(false) };
        let failed = |name: &str, e: &dyn std::fmt::Display| {
            CpdError::new(ErrorCode::MigrationFailed, format!("Migration {name} of the database failed: {e}."))
                .with("migration", name)
                .with("sqlite_message", e.to_string())
        };
        // The migration's `txns` row is inserted as soon as `txns` exists
        // (migration 1 creates it), so later migrations can name it: migration
        // 4's PRs record it as their `created_txn` and in their history.
        let mut txn = None;
        let mut seq = 0_i64;
        let mut imported_old_stores = false;
        if let Start::Fresh | Start::Convert(_) = plan.start {
            let from = match plan.start {
                Start::Convert(version) => version as usize,
                _ => 0,
            };
            if from > 0 {
                // C2 step 2: a tampered legacy store must not become the base of the new scheme.
                let expected = schema::SCHEMA_HASHES[from - 1];
                let found = schema::legacy_schema_hash(&tx)?;
                if found != expected {
                    return Err(CpdError::new(
                        ErrorCode::StoreTampered,
                        "The database schema does not match any this cpd can convert: a table or trigger was changed outside cpd. Nothing was converted.",
                    )
                    .with("path", path)
                    .with("version", from)
                    .with("expected", expected)
                    .with("found", found));
                }
            } else {
                tx.pragma_update(None, "application_id", APPLICATION_ID).map_err(|e| failed(LEGACY[0].name, &e))?;
            }
            for (index, legacy) in LEGACY.iter().enumerate().skip(from) {
                if index > 0 && txn.is_none() {
                    txn = Some(begin_txn(&tx, &self.txn, MIGRATE_COMMAND, &caller).map_err(|e| failed(legacy.name, &e))?);
                }
                (legacy.apply)(&tx).map_err(|e| failed(legacy.name, &e))?;
                if index + 1 == schema::IMPORTS_OLD_STORES {
                    schema::import_old_stores(&tx, &home.old_contexts_dir()).map_err(|e| failed(legacy.name, &e))?;
                }
            }
            imported_old_stores = from < schema::IMPORTS_OLD_STORES;
            let id = match txn {
                Some(id) => id,
                None => begin_txn(&tx, &self.txn, MIGRATE_COMMAND, &caller).map_err(|e| failed("schema_migrations", &e))?,
            };
            txn = Some(id);
            tx.execute_batch(schema::RUNNER_DDL).map_err(|e| failed("schema_migrations", &e))?;
            for legacy in LEGACY {
                seq += 1;
                record(&tx, legacy.name, legacy.compat, legacy.sql, seq, id, from > 0).map_err(|e| failed(legacy.name, &e))?;
            }
        } else {
            let id = begin_txn(&tx, &self.txn, MIGRATE_COMMAND, &caller).map_err(|e| failed("schema_migrations", &e))?;
            txn = Some(id);
            seq = tx
                .query_row("SELECT coalesce(max(seq), 0) FROM schema_migrations", [], |r| r.get(0))
                .map_err(|e| failed("schema_migrations", &e))?;
        }
        let txn = txn.expect("every path above inserts the migrating txns row");
        for named in &plan.pending {
            tx.execute_batch(named.sql).map_err(|e| failed(named.name, &e))?;
            if let Some(run) = named.run {
                run(&tx, home).map_err(|e| failed(named.name, &e.message()))?;
            }
            seq += 1;
            record(&tx, named.name, named.compat, named.sql, seq, txn, false).map_err(|e| failed(named.name, &e))?;
        }
        schema::reconcile(&tx).map_err(|e| failed("reconcile", &e))?;
        let violations = foreign_key_violations(&tx).map_err(|e| failed("foreign_key_check", &e))?;
        if !violations.is_empty() {
            return Err(CpdError::new(
                ErrorCode::MigrationFailed,
                format!("Migration of the database failed: {} rows break a foreign key.", violations.len()),
            )
            .with("migration", "foreign_key_check")
            .with("foreign_key_violations", Value::from(violations)));
        }
        if plan.start != Start::Named {
            tx.pragma_update(None, "user_version", VERSION).map_err(|e| failed("user_version", &e))?;
        }
        tx.commit()?;
        Ok(imported_old_stores)
    }

    /// The schema check (§4.3): every generated history trigger is what the
    /// generator makes, and the store has what this binary's migrations
    /// created: the canonical hash is the catalog's pinned one, or, when the
    /// store applied additive migrations this binary does not know (inside the
    /// window, §4.5), the canonical subset of the objects this binary's
    /// migrations created ([`verdict::subset_mismatch`]).
    fn check_schema(&self, home: &CpdHome, catalog: &Catalog<'_>) -> Result<Option<SchemaMismatch>, CpdError> {
        if let Some((name, expected, found)) = schema::generated_mismatch(&self.conn)? {
            return Ok(Some(SchemaMismatch { expected, found: found.unwrap_or_else(|| "missing".to_owned()), object: Some(name) }));
        }
        if !verdict_with(&self.conn, catalog)?.unknown.is_empty() {
            return verdict::subset_mismatch(&self.conn, catalog, home);
        }
        let found = canonical_hash(&self.conn)?;
        Ok((found != catalog.schema_hash)
            .then(|| SchemaMismatch { expected: catalog.schema_hash.to_owned(), found, object: None }))
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
    /// `store_tampered` when the schema check failed and with `store_too_new`
    /// when another process migrated the database past this cpd's schema.
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
                "The database schema does not match this cpd's: a table or trigger was changed outside cpd.",
            )
            .with("path", self.path.display().to_string())
            .with("expected", mismatch.expected.clone())
            .with("found", mismatch.found.clone())
            .with("object", mismatch.object.clone()));
        }
        let caller = Caller::current();
        let (path, busy_ms) = (self.path.clone(), self.busy_ms);
        let _guard = TxnGuard(&self.txn);
        let result = (|| {
            let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Another process may have migrated since this process opened.
            refuse_user_version(user_version(&tx)?)?;
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
            report.push(json!({"kind": "schema", "expected": m.expected, "found": m.found, "object": m.object}));
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

/// Moves each `contexts/<id>/` folder migration 0007 imported to
/// `archive/contexts/<id>/`, creating the parents (§4.5, §1.4). Nothing is
/// deleted, and an existing destination is never overwritten: that folder
/// stays, the others are still moved, and the failure is reported.
fn archive_old_stores(home: &CpdHome) -> Result<(), CpdError> {
    let failed = |message: String| CpdError::new(ErrorCode::StoreFailed, message);
    let source = home.old_contexts_dir();
    let destination = home.archived_contexts_dir();
    let folders = legacy::folders(&source).map_err(&failed)?;
    if folders.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(&destination)
        .map_err(|e| failed(format!("Cannot create {}: {e}.", destination.display())))?;
    let mut problems: Vec<String> = Vec::new();
    for folder in folders {
        let target = destination.join(folder.file_name().expect("a folder has a name"));
        if target.symlink_metadata().is_ok() {
            problems.push(format!("{} stays: {} exists", folder.display(), target.display()));
        } else if let Err(e) = fs::rename(&folder, &target) {
            problems.push(format!("{} stays: cannot move it to {}: {e}", folder.display(), target.display()));
        }
    }
    if problems.is_empty() {
        // Succeeds only when `contexts/` is empty now.
        let _ = fs::remove_dir(&source);
        return Ok(());
    }
    Err(failed(format!(
        "The database migrated and holds every Context record, but the old Context stores were not all moved to {}: {}.",
        destination.display(),
        problems.join("; ")
    ))
    .with("problems", problems))
}

/// Takes `locks/store.lock` (§1.4), creating `$CPD_HOME` and `locks/` on first
/// use, and waits for it without a limit: a holder is another cpd process
/// doing the open's bounded setup. Dropping the result releases it.
fn setup_lock(home: &CpdHome) -> Result<Flock<File>, CpdError> {
    let failed = |what: &str, path: &Path, e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot {what} {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    };
    let dir = home.locks_dir();
    fs::create_dir_all(&dir).map_err(|e| failed("create", &dir, e))?;
    // `home.rs` has no method for this lock; it is the store's alone.
    let path = dir.join("store.lock");
    let mut file =
        OpenOptions::new().create(true).append(true).open(&path).map_err(|e| failed("open", &path, e))?;
    loop {
        match Flock::lock(file, FlockArg::LockExclusive) {
            Ok(lock) => return Ok(lock),
            Err((f, Errno::EINTR)) => file = f,
            Err((_, errno)) => return Err(failed("lock", &path, errno.into())),
        }
    }
}

/// The database's `PRAGMA user_version`, or `None` when the file does not
/// exist. Opens it read-only: no migration, no schema check, no write.
pub fn current_schema(home: &CpdHome) -> Result<Option<i64>, CpdError> {
    let path = home.db();
    if !path.try_exists().map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot read {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    })? {
        return Ok(None);
    }
    let read = || -> Result<i64, CpdError> {
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(user_version(&conn)?)
    };
    read().map(Some).map_err(|e| enrich(e, &path, 0))
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
}

/// `PRAGMA foreign_key_check`: one `{table, rowid, parent}` per row that
/// breaks a foreign key.
fn foreign_key_violations(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    stmt.query_map([], |r| {
        Ok(json!({"table": r.get::<_, String>(0)?, "rowid": r.get::<_, Option<i64>>(1)?, "parent": r.get::<_, String>(2)?}))
    })?
    .collect()
}

/// The migrations a binary knows and the canonical hash it pins for them.
struct Catalog<'a> {
    /// Named migrations, sorted by name.
    named: &'a [Named],
    schema_hash: &'a str,
}

impl Catalog<'_> {
    const BUILT_IN: Catalog<'static> = Catalog { named: schema::NAMED, schema_hash: SCHEMA_HASH };
}

/// Records an applied migration in `schema_migrations` (§4.4).
fn record(
    conn: &Connection,
    name: &str,
    compat: schema::Compat,
    sql: &str,
    seq: i64,
    txn: i64,
    converted: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![name, compat.as_str(), schema::sha256_hex(sql), seq, txn, converted],
    )?;
    Ok(())
}

/// What `txns.cwd` holds when the process's cwd cannot be read (§4.4).
const UNREADABLE_CWD: &str = "(unreadable)";

/// Who opens a write transaction: `txns.pid`, `session` and `cwd` (§4.4).
struct Caller {
    pid: u32,
    session: Option<String>,
    cwd: String,
}

impl Caller {
    fn current() -> Caller {
        let session = ["CPD_SESSION", "PI_SESSION_ID"]
            .iter()
            .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
            .map(|v| v.to_string_lossy().into_owned());
        // The column is information, not a key: a deleted cwd must not stop a write.
        let cwd = std::env::current_dir().map_or_else(|_| UNREADABLE_CWD.to_owned(), |d| d.to_string_lossy().into_owned());
        Caller { pid: std::process::id(), session, cwd }
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
/// busy timeout is `store_busy`; the triggers' `RAISE(ABORT, 'job_locked')`,
/// `'illegal_transition'`, `'pr_locked'` and `'needs_other_pr'` (§4.4) are
/// those refusals; anything else is `store_failed` with `sqlite_code` and
/// `sqlite_message`.
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
                "Another cpd process held the database's write lock for longer than store_busy_ms.",
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
            Some(f) if f.code == SqliteCode::ConstraintViolation && message == "pr_locked" => CpdError::new(
                ErrorCode::PrLocked,
                "The PR is Locked: its repo, base and needs cannot change after the Plan's lock.",
            )
            .with("sqlite_message", message),
            Some(f) if f.code == SqliteCode::ConstraintViolation && message == "needs_other_pr" => {
                CpdError::new(ErrorCode::NeedsOtherPr, "A Dependency can only link Jobs of the same PR.")
                    .with("sqlite_message", message)
            }
            _ => CpdError::new(ErrorCode::StoreFailed, format!("The database failed: {message}."))
                .with("sqlite_code", sqlite.map_or(Value::Null, |f| Value::from(f.extended_code)))
                .with("sqlite_message", message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_migration_leaves_foreign_keys_on_and_legacy_alter_table_off() {
        let dir = tempfile::tempdir().unwrap();
        // Migration 1 cannot apply: a table `history` already exists.
        Connection::open(dir.path().join("cpd.db")).unwrap().execute_batch("CREATE TABLE history (x)").unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::connect(&dir.path().join("cpd.db"), 5000).unwrap();
        let err = store.migrate(&home, &Catalog::BUILT_IN).unwrap_err();
        assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
        let get = |name: &str| store.conn.pragma_query_value(None, name, |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((get("foreign_keys"), get("legacy_alter_table")), (1, 0));
    }

    #[test]
    fn current_schema_is_none_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        assert_eq!(current_schema(&home).unwrap(), None);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn current_schema_reads_user_version_and_leaves_the_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let conn = Connection::open(home.db()).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        let before = fs::read(home.db()).unwrap();
        assert_eq!(current_schema(&home).unwrap(), Some(99));
        assert_eq!(fs::read(home.db()).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    // ---------- named migrations (§4.5) ----------

    pub(super) fn tmp_home() -> (tempfile::TempDir, CpdHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        (dir, home)
    }

    pub(super) const ADD_A: Named = Named {
        name: "20300101T0000-add-a",
        compat: schema::Compat::Additive,
        sql: "-- cpd-migration: additive\nALTER TABLE plans ADD COLUMN note_a TEXT;\nALTER TABLE jobs ADD COLUMN extra TEXT;\n",
        run: None,
    };
    pub(super) const ADD_B: Named = Named {
        name: "20300102T0000-add-b",
        compat: schema::Compat::Additive,
        sql: "-- cpd-migration: additive\nALTER TABLE plans ADD COLUMN note_b TEXT CHECK (note_b IS NULL OR length(note_b) >= 1);\n\
              CREATE TABLE b_things (id INTEGER PRIMARY KEY, label TEXT NOT NULL) STRICT;\nCREATE INDEX b_things_label ON b_things (label);\n",
        run: None,
    };

    /// The canonical hash two stores that applied `ADD_A` and `ADD_B` share, pinned: a change of the
    /// canonical form (§4.3) or of the SQLite bundle changes it.
    const A_AND_B_HASH: &str = "28c7632ec8b4aeeff452b75e26f28ba696405e460eefe9a02c476a3bbec90651";

    /// A store opened with `named` as its migrations, whose pinned hash is the one of a fresh store with them.
    pub(super) fn open_named(home: &CpdHome, named: &[Named]) -> Result<Store, CpdError> {
        let probe = tempfile::tempdir().unwrap();
        let probe_home = CpdHome::resolve(Some(probe.path().join("cpd").into()), None).unwrap();
        let pinned = {
            // Even a refusal of `home` is judged against the pinned hash of a fresh store with the same list.
            let catalog = Catalog { named, schema_hash: "" };
            let fresh = Store::open_with(&probe_home, 5000, &catalog)?;
            canonical_hash(&fresh.conn)?
        };
        Store::open_with(home, 5000, &Catalog { named, schema_hash: &pinned })
    }

    pub(super) fn applied(store: &mut Store) -> Vec<(String, i64, i64)> {
        store
            .read(|c| {
                let mut stmt = c.prepare("SELECT name, seq, converted FROM schema_migrations ORDER BY seq")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    pub(super) fn plan_columns(store: &mut Store) -> Vec<String> {
        store
            .read(|c| {
                let mut stmt = c.prepare("SELECT name FROM pragma_table_info('plans') ORDER BY cid")?;
                let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    #[test]
    fn two_additive_migrations_give_the_same_canonical_hash_in_both_orders() {
        let (_a, a_first) = tmp_home();
        let (_b, b_first) = tmp_home();
        let (_c, together) = tmp_home();
        // One store applied `a` and later `b`; the other `b` and later `a`: a migration that sorts
        // before an applied one is allowed when it is additive (O2).
        drop(open_named(&a_first, &[ADD_A]).unwrap());
        let mut a_then_b = open_named(&a_first, &[ADD_A, ADD_B]).unwrap();
        drop(open_named(&b_first, &[ADD_B]).unwrap());
        let mut b_then_a = open_named(&b_first, &[ADD_A, ADD_B]).unwrap();
        let mut both = open_named(&together, &[ADD_A, ADD_B]).unwrap();

        let n = LEGACY.len() as i64;
        assert_eq!(applied(&mut a_then_b).into_iter().skip(LEGACY.len()).collect::<Vec<_>>(), [(ADD_A.name.into(), n + 1, 0), (ADD_B.name.into(), n + 2, 0)]);
        assert_eq!(applied(&mut b_then_a).into_iter().skip(LEGACY.len()).collect::<Vec<_>>(), [(ADD_B.name.into(), n + 1, 0), (ADD_A.name.into(), n + 2, 0)]);

        // The stores differ in the order of their columns and in their `sqlite_schema` text ...
        let (cols_ab, cols_ba) = (plan_columns(&mut a_then_b), plan_columns(&mut b_then_a));
        assert_ne!(cols_ab, cols_ba);
        let legacy = |s: &mut Store| s.read(|c| Ok(schema::legacy_schema_hash(c)?)).unwrap();
        assert_ne!(legacy(&mut a_then_b), legacy(&mut b_then_a), "the plain hash depends on the order");
        // ... and have the same canonical hash, which is pinned.
        let canonical = |s: &mut Store| s.read(|c| Ok(canonical_hash(c)?)).unwrap();
        let hash = canonical(&mut a_then_b);
        assert_eq!(canonical(&mut b_then_a), hash);
        assert_eq!(canonical(&mut both), hash);
        assert_eq!(hash, A_AND_B_HASH, "the pinned canonical hash of a store with a and b");
        for store in [&a_then_b, &b_then_a, &both] {
            assert_eq!(store.schema_mismatch(), None);
        }
        // Each store's generated triggers list its own column order, and every column.
        for store in [&mut a_then_b, &mut b_then_a] {
            let sql: String = store
                .read(|c| Ok(c.query_row("SELECT sql FROM sqlite_schema WHERE name = 'plans_hist_ins'", [], |r| r.get(0))?))
                .unwrap();
            assert!(sql.contains("'note_a', NEW.note_a") && sql.contains("'note_b', NEW.note_b"), "{sql}");
        }
        a_then_b.write("test", |tx| Ok(tx.execute_batch("UPDATE plans SET note_a = 'x' WHERE 0")?)).unwrap();
    }

    /// `kind-tables`, the first named migration, then `more`: the list of a binary with extra additive migrations.
    pub(super) fn with_kind_tables(more: &[Named]) -> Vec<Named> {
        std::iter::once(schema::NAMED[0]).chain(more.iter().copied()).collect()
    }

    /// What two Plans each write to add an Event kind (§4.5 K2): one insert, additive.
    const KIND_PINNED: Named = Named {
        name: "20300201T0000-kind-plan-pinned",
        compat: schema::Compat::Additive,
        sql: "-- cpd-migration: additive\nINSERT INTO event_kinds (name) VALUES ('plan-pinned');\n",
        run: None,
    };
    const KIND_MESSAGE: Named = Named {
        name: "20300202T0000-kind-session-message",
        compat: schema::Compat::Additive,
        sql: "-- cpd-migration: additive\nINSERT INTO event_kinds (name) VALUES ('session-message');\n",
        run: None,
    };

    fn kinds(store: &mut Store) -> Vec<String> {
        store
            .read(|c| {
                let mut stmt = c.prepare("SELECT name FROM event_kinds ORDER BY name")?;
                let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// Inserts an Event of `kind`, as cpd would.
    fn insert_event(store: &mut Store, kind: &str) -> Result<(), CpdError> {
        store.write("test", |tx| {
            tx.execute(
                "INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', ?1, NULL, 'x', '{}')",
                [kind],
            )?;
            Ok(())
        })
    }

    #[test]
    fn kind_tables_is_the_first_named_migration_and_breaking_after_the_legacy_set() {
        let first = schema::NAMED[0];
        assert!(first.name.ends_with("-kind-tables"), "{}", first.name);
        assert_eq!(first.compat, schema::Compat::Breaking);
        assert_eq!(first.sql.lines().next(), Some(format!("-- cpd-migration: breaking after {}", LEGACY.len()).as_str()));
    }

    #[test]
    fn an_event_kind_added_only_by_a_later_additive_insert_can_be_inserted() {
        let (_dir, home) = tmp_home();
        // Without the migration the kind does not exist: the foreign key refuses it.
        let mut store = open_named(&home, &with_kind_tables(&[])).unwrap();
        let err = insert_event(&mut store, "plan-pinned").unwrap_err();
        assert!(format!("{err:?}").contains("FOREIGN KEY"), "{err:?}");
        drop(store);
        // One `INSERT INTO event_kinds`, applied by a later binary, makes it insertable.
        let mut store = open_named(&home, &with_kind_tables(&[KIND_PINNED])).unwrap();
        assert!(kinds(&mut store).contains(&"plan-pinned".to_owned()));
        insert_event(&mut store, "plan-pinned").unwrap();
        // The other kinds stay, and a kind nobody added stays refused.
        insert_event(&mut store, "plan-created").unwrap();
        assert!(insert_event(&mut store, "session-message").is_err());
        assert_eq!(applied(&mut store).last().map(|a| a.0.as_str()), Some(KIND_PINNED.name));
    }

    #[test]
    fn two_plans_kind_migrations_applied_in_either_order_keep_both_kinds() {
        let (_a, pinned_first) = tmp_home();
        let (_b, message_first) = tmp_home();
        let (_c, together) = tmp_home();
        let both = with_kind_tables(&[KIND_PINNED, KIND_MESSAGE]);
        drop(open_named(&pinned_first, &with_kind_tables(&[KIND_PINNED])).unwrap());
        let mut a = open_named(&pinned_first, &both).unwrap();
        drop(open_named(&message_first, &with_kind_tables(&[KIND_MESSAGE])).unwrap());
        // `KIND_PINNED` sorts before the applied `KIND_MESSAGE`: allowed, it is additive (O2).
        let mut b = open_named(&message_first, &both).unwrap();
        let mut c = open_named(&together, &both).unwrap();

        let order = |s: &mut Store| applied(s).into_iter().skip(7).map(|a| a.0).collect::<Vec<_>>();
        assert_eq!(order(&mut a), [KIND_PINNED.name, KIND_MESSAGE.name]);
        assert_eq!(order(&mut b), [KIND_MESSAGE.name, KIND_PINNED.name]);
        let expected = kinds(&mut c);
        assert!(["plan-pinned", "session-message"].iter().all(|k| expected.contains(&(*k).to_owned())));
        for store in [&mut a, &mut b] {
            assert_eq!(kinds(store), expected, "neither migration dropped the other's kind");
            insert_event(store, "plan-pinned").unwrap();
            insert_event(store, "session-message").unwrap();
            assert_eq!(store.schema_mismatch(), None);
        }
        let canonical = |s: &mut Store| s.read(|c| Ok(canonical_hash(c)?)).unwrap();
        assert_eq!(canonical(&mut a), canonical(&mut b));
        assert_eq!(canonical(&mut a), canonical(&mut c));
    }

    #[test]
    fn a_store_whose_floor_this_binary_lacks_is_store_too_new() {
        let (_dir, home) = tmp_home();
        let breaking = Named { compat: schema::Compat::Breaking, ..ADD_B };
        drop(open_named(&home, &[ADD_A, breaking]).unwrap());
        let err = Store::open(&home, 5000).err().unwrap();
        assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
        assert_eq!(err.details()["user_version"], VERSION);
        assert_eq!(err.details()["floor"], ADD_B.name);
        assert_eq!(err.details()["unknown"], serde_json::json!([ADD_A.name, ADD_B.name]));
        // An additive one is inside the window (§4.5): the binary opens it (verdict.rs tests).
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &with_kind_tables(&[ADD_A])).unwrap());
        assert!(Store::open(&home, 5000).is_ok());
    }

    #[test]
    fn a_migration_whose_text_changed_is_migration_changed() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A]).unwrap());
        let edited = Named { sql: "-- cpd-migration: additive\nALTER TABLE plans ADD COLUMN note_a TEXT NOT NULL DEFAULT '';\n", ..ADD_A };
        let err = open_named(&home, &[edited]).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationChanged, "{err:?}");
        assert_eq!(err.exit().code(), 1);
        assert_eq!(err.details()["name"], ADD_A.name);
        assert_eq!(err.details()["recorded"], schema::sha256_hex(ADD_A.sql));
        assert_eq!(err.details()["embedded"], schema::sha256_hex(edited.sql));
    }

    #[test]
    fn a_legacy_migration_whose_recorded_text_changed_is_migration_changed() {
        let (_dir, home) = tmp_home();
        drop(Store::open(&home, 5000).unwrap());
        // `schema_migrations` is append-only; a repair by hand goes through a connection with `cpd_txn()`.
        let conn = Connection::open(home.db()).unwrap();
        conn.execute_batch("DROP TRIGGER schema_migrations_no_update; UPDATE schema_migrations SET checksum = lower(hex(zeroblob(32))) WHERE seq = 3").unwrap();
        drop(conn);
        let err = Store::open(&home, 5000).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationChanged, "{err:?}");
        assert_eq!(err.details()["name"], LEGACY[2].name);
    }

    #[test]
    fn a_failed_named_migration_rolls_the_whole_transaction_back() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A]).unwrap());
        let broken = Named { name: "20300103T0000-broken", sql: "-- cpd-migration: additive\nALTER TABLE plans ADD COLUMN ok TEXT;\nALTER TABLE nope ADD COLUMN x TEXT;\n", ..ADD_B };
        let err = open_named(&home, &[ADD_A, broken]).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
        assert_eq!(err.details()["migration"], broken.name);
        let mut store = open_named(&home, &[ADD_A]).unwrap();
        assert_eq!(applied(&mut store).len(), LEGACY.len() + 1, "the legacy migrations and a");
        assert!(!plan_columns(&mut store).contains(&"ok".to_owned()));
    }

    fn make_hook_table(conn: &Connection, home: &CpdHome) -> Result<(), CpdError> {
        conn.execute("CREATE TABLE hooked (home TEXT NOT NULL) STRICT", [])?;
        conn.execute("INSERT INTO hooked (home) VALUES (?1)", [home.db().display().to_string()])?;
        Ok(())
    }

    #[test]
    fn a_migration_with_rust_runs_it_after_its_sql_with_the_home() {
        let (_dir, home) = tmp_home();
        let hooked = Named { run: Some(make_hook_table), ..ADD_A };
        let mut store = open_named(&home, &[hooked]).unwrap();
        let at: String = store.read(|c| Ok(c.query_row("SELECT home FROM hooked", [], |r| r.get(0))?)).unwrap();
        assert_eq!(at, home.db().display().to_string());
        assert!(plan_columns(&mut store).contains(&"note_a".to_owned()));
    }

    #[test]
    fn user_versions_other_than_a_legacy_one_or_the_named_scheme_are_store_too_new() {
        for version in [12, 99, 999, 1001] {
            let err = refuse_user_version(version).unwrap_err();
            assert_eq!(err.code(), ErrorCode::StoreTooNew, "{version}");
            assert_eq!(err.details()["user_version"], version);
        }
        for version in [0, 1, 6, 7, 11, 1000] {
            refuse_user_version(version).unwrap();
        }
    }

    #[test]
    fn a_named_store_without_schema_migrations_is_store_failed() {
        let (_dir, home) = tmp_home();
        std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
        let conn = Connection::open(home.db()).unwrap();
        conn.pragma_update(None, "user_version", VERSION).unwrap();
        drop(conn);
        let err = Store::open(&home, 5000).err().unwrap();
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    }

    #[test]
    fn the_migrations_directory_is_the_legacy_set_and_the_named_list() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/migrations");
        let mut numbered = Vec::new();
        let mut named = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let file = entry.unwrap().file_name().into_string().unwrap();
            if file.ends_with(".rs") {
                continue; // a named migration's Rust: build.rs refuses one without its .sql
            }
            let stem = file.strip_suffix(".sql").unwrap_or_else(|| panic!("{file}: only migrations here"));
            if stem.as_bytes()[0] == b'0' { numbered.push(stem.to_owned()) } else { named.push(stem.to_owned()) }
        }
        numbered.sort();
        named.sort();
        assert_eq!(numbered, LEGACY.iter().map(|l| l.name.to_owned()).collect::<Vec<_>>(), "a numbered migration is never added");
        assert_eq!(named, schema::NAMED.iter().map(|n| n.name.to_owned()).collect::<Vec<_>>());
        assert!(schema::NAMED.windows(2).all(|w| w[0].name < w[1].name), "sorted by name");
        for n in schema::NAMED {
            assert_eq!(n.sql.lines().next().map(|l| l.starts_with("-- cpd-migration: ")), Some(true), "{}", n.name);
            assert!(n.name > LEGACY[LEGACY.len() - 1].name, "{}: names sort after the legacy set", n.name);
        }
        assert!(schema::NAMED[0].name.ends_with("-kind-tables"), "no named migration sorts before kind-tables (§4.5)");
    }
}
