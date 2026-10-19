//! The SQLite store (contract §4.1–§4.5): opening `$CPD_HOME/cpd.db`, read and
//! write transactions, `txns` and `cpd_txn()`, the DDL, migrations, the schema
//! check and the store's part of the tamper report.
//!
//! Every module reads through [`Store::read`] and writes through
//! [`Store::write`]; nothing else writes the database (§4.3).

mod schema;

use std::fs::{self, File, OpenOptions};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal::kill;
use nix::unistd::Pid;
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, ErrorCode as SqliteCode, OpenFlags, Transaction, TransactionBehavior};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::context::legacy;
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
    /// Set by [`Store::persist`]: wait out a transient busy instead of failing.
    persist: Option<Persist>,
}

/// How a long-lived process (an Attempt driver, the engine) meets a transient
/// store error (§3.4, §3.7, §4.2): it waits and tries again, 0.5 s doubling to
/// 5 s, for as long as the error lasts, and never gives the error to its
/// caller. Only the waiting for a lock is retried, never a closure that ran.
pub struct Persist {
    log: Box<dyn FnMut(&str)>,
    give_up: fn() -> bool,
}

impl Persist {
    /// `log` gets one line per error code and one when the store clears;
    /// `give_up` is asked while waiting: when it says so, the error is returned
    /// (the engine's shutdown).
    pub fn new(log: impl FnMut(&str) + 'static, give_up: fn() -> bool) -> Persist {
        Persist { log: Box::new(log), give_up }
    }
}

const BACKOFF_FIRST: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// One episode of waiting: from the first transient error to the next success.
struct Wait {
    delay: Duration,
    since: Option<Instant>,
    codes: Vec<String>,
}

impl Wait {
    fn new() -> Wait {
        Wait { delay: BACKOFF_FIRST, since: None, codes: Vec::new() }
    }
}

/// A transient store error: `store_busy`, or `store_failed` for SQLite's
/// `SQLITE_BUSY` or `SQLITE_LOCKED` (primary codes 5 and 6, whatever the
/// extended code; `SQLITE_LOCKED` is not `store_busy`, store-W7).
pub fn is_transient(e: &CpdError) -> bool {
    match e.code() {
        ErrorCode::StoreBusy => true,
        ErrorCode::StoreFailed => {
            e.details().get("sqlite_code").and_then(Value::as_i64).is_some_and(|c| matches!(c & 0xff, 5 | 6))
        }
        _ => false,
    }
}

/// Settles one attempt of a step that may meet a transient error. `Ok(Some)`:
/// it worked. `Ok(None)`: it met a transient error, which was logged and
/// waited out; try again. `Err`: not transient, or not persisting, or told to give up.
fn settle<T>(persist: &mut Option<Persist>, wait: &mut Wait, result: Result<T, CpdError>) -> Result<Option<T>, CpdError> {
    let Some(p) = persist else { return result.map(Some) };
    match result {
        Ok(value) => {
            if let Some(since) = wait.since.take() {
                (p.log)(&format!("the store is clear again after {:.1} s", since.elapsed().as_secs_f64()));
                *wait = Wait::new();
            }
            Ok(Some(value))
        }
        Err(e) if is_transient(&e) && !(p.give_up)() => {
            let code = e.code().as_str().to_owned();
            wait.since.get_or_insert_with(Instant::now);
            if !wait.codes.contains(&code) {
                (p.log)(&format!("{code}: {} Waiting and trying again until it clears.", e.message()));
                wait.codes.push(code);
            }
            let until = Instant::now() + wait.delay;
            while Instant::now() < until && !(p.give_up)() {
                std::thread::sleep(Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())));
            }
            wait.delay = (wait.delay * 2).min(BACKOFF_MAX);
            Ok(None)
        }
        Err(e) => Err(e),
    }
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
    ///
    /// The one-time setup (the WAL switch and the migration) runs under
    /// `locks/store.lock` (§1.4): SQLite answers concurrent switches to WAL of
    /// a new database with `SQLITE_BUSY` at once, without its busy handler.
    pub fn open(home: &CpdHome, busy_ms: u64) -> Result<Store, CpdError> {
        let path = home.db();
        let setup = setup_lock(home)?;
        let mut store = Store::connect(&path, busy_ms).map_err(|e| enrich(e, &path, busy_ms))?;
        store.migrate(home).map_err(|e| enrich(e, &path, busy_ms))?;
        drop(setup);
        store.schema = store.check_schema().map_err(|e| enrich(e, &path, busy_ms))?;
        Ok(store)
    }

    /// [`Store::open`] for a process that persists (see [`Persist`]): a
    /// transient error while opening is waited out, and the store it returns
    /// persists too.
    pub fn open_persistent(home: &CpdHome, busy_ms: u64, persist: Persist) -> Result<Store, CpdError> {
        let mut wait = Wait::new();
        let mut persist = Some(persist);
        loop {
            if let Some(mut store) = settle(&mut persist, &mut wait, Store::open(home, busy_ms))? {
                store.persist = persist;
                return Ok(store);
            }
        }
    }

    /// From now on, every [`Store::read`] and [`Store::write`] of this store
    /// persists (see [`Persist`]); replaces the earlier one.
    pub fn persist(&mut self, persist: Persist) {
        self.persist = Some(persist);
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
        conn.pragma_update(None, "synchronous", "NORMAL")?;

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
        Ok(Store { conn, path: path.to_owned(), busy_ms, txn, schema: None, persist: None })
    }

    /// §4.5: below N, migrate in one `BEGIN IMMEDIATE` transaction; above N, refuse.
    ///
    /// The transaction runs with foreign keys off and `legacy_alter_table` on
    /// (§4.5 migration 4, step 1: both only take effect outside a
    /// transaction), so a migration can rebuild a table that others reference;
    /// `PRAGMA foreign_key_check` then holds before the commit (step 5), and so
    /// does the schema hash of the version reached (§4.5, hs-n121). Both
    /// are restored afterwards, whether the migration committed or not.
    ///
    /// When the migration that imported the old Context stores (§4.5 migration
    /// 0007) committed, the folders are moved to `archive/contexts/` (§1.4).
    fn migrate(&mut self, home: &CpdHome) -> Result<(), CpdError> {
        let version = user_version(&self.conn)?;
        refuse_too_new(version)?;
        if version == VERSION {
            return Ok(());
        }
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        self.conn.pragma_update(None, "legacy_alter_table", "ON")?;
        let migrated = self.migrate_in_txn(home);
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

    /// Applies the missing migrations in one transaction. Returns whether the
    /// one that imports the old Context stores was among them.
    fn migrate_in_txn(&mut self, home: &CpdHome) -> Result<bool, CpdError> {
        let caller = Caller::current();
        let _guard = TxnGuard(&self.txn);
        let mut holder = Holder::default();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        holder.record(&self.path, MIGRATE_COMMAND);
        // Another process may have migrated since the read above.
        let version = user_version(&tx)?;
        refuse_too_new(version)?;
        if version == VERSION {
            return Ok(false);
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
        // The migration's `txns` row is inserted as soon as `txns` exists
        // (migration 1 creates it), so later migrations can name it: migration
        // 3's PRs record it as their `created_txn` and in their history.
        let mut txn_row = false;
        for (index, migration) in schema::MIGRATIONS.iter().enumerate().skip(version as usize) {
            if index > 0 && !txn_row {
                begin_txn(&tx, &self.txn, MIGRATE_COMMAND, &caller).map_err(|e| failed(index as i64 + 1, e))?;
                txn_row = true;
            }
            migration(&tx, home).map_err(|e| failed(index as i64 + 1, e))?;
        }
        if !txn_row {
            begin_txn(&tx, &self.txn, MIGRATE_COMMAND, &caller).map_err(|e| failed(VERSION, e))?;
        }
        let violations = foreign_key_violations(&tx).map_err(|e| failed(VERSION, e))?;
        if !violations.is_empty() {
            return Err(CpdError::new(
                ErrorCode::MigrationFailed,
                format!("Migration {VERSION} of the database failed: {} rows break a foreign key.", violations.len()),
            )
            .with("from", version)
            .with("to", VERSION)
            .with("foreign_key_violations", Value::from(violations)));
        }
        tx.pragma_update(None, "user_version", VERSION).map_err(|e| failed(VERSION, e))?;
        // The check of §4.3, made before the commit: a schema the binary does
        // not describe is never committed (§4.5, hs-n121).
        let expected = schema::SCHEMA_HASHES[VERSION as usize - 1];
        let actual = schema_hash(&tx).map_err(|e| failed(VERSION, e))?;
        if actual != expected {
            return Err(CpdError::new(
                ErrorCode::MigrationFailed,
                format!("Migration {VERSION} of the database failed: its schema hash is {actual}, expected {expected}."),
            )
            .with("from", version)
            .with("version", VERSION)
            .with("expected", expected)
            .with("actual", actual));
        }
        tx.commit()?;
        Ok((version as usize) < schema::IMPORTS_OLD_STORES)
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
        let mut wait = Wait::new();
        let result = (|| {
            let tx = loop {
                let begun = Transaction::new(&mut self.conn, TransactionBehavior::Deferred).map_err(CpdError::from).and_then(|tx| {
                    // A persisting reader takes its snapshot here, where a busy store is waited out.
                    if self.persist.is_some() {
                        tx.query_row("SELECT 1 FROM sqlite_schema LIMIT 1", [], |_| Ok(())).or_else(|e| match e {
                            rusqlite::Error::QueryReturnedNoRows => Ok(()),
                            e => Err(e),
                        })?;
                    }
                    Ok(tx)
                });
                if let Some(tx) = settle(&mut self.persist, &mut wait, begun.map_err(|e| enrich(e, &path, busy_ms)))? {
                    break tx;
                }
            };
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
            .with("found", mismatch.found.clone()));
        }
        let caller = Caller::current();
        let (path, busy_ms) = (self.path.clone(), self.busy_ms);
        let _guard = TxnGuard(&self.txn);
        let mut wait = Wait::new();
        // Declared before the transaction: dropped, so the record removed, after its commit or rollback.
        let mut holder = Holder::default();
        let asked = Instant::now();
        let result = (|| {
            let tx = loop {
                let begun = Transaction::new(&mut self.conn, TransactionBehavior::Immediate)
                    .map_err(|e| enrich(CpdError::from(e), &path, busy_ms));
                if let Some(tx) = settle(&mut self.persist, &mut wait, begun)? {
                    break tx;
                }
            };
            let begun = Instant::now();
            holder.record(&path, command);
            // Another process may have migrated since this process opened.
            refuse_too_new(user_version(&tx)?)?;
            let id = begin_txn(&tx, &self.txn, command, &caller)?;
            let write = WriteTxn { tx, id };
            let ran = Instant::now();
            let value = f(&write)?;
            let committing = Instant::now();
            write.tx.commit()?;
            let held = begun.elapsed();
            if held >= SLOW_HOLD {
                let ms = |d: Duration| d.as_millis() as u64;
                let phases = json!({
                    "waited_ms": ms(begun - asked), "begin_ms": ms(ran - begun),
                    "closure_ms": ms(committing - ran), "commit_ms": ms(committing.elapsed()), "held_ms": ms(held),
                });
                Holder::report_slow(&path, command, phases);
            }
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

fn refuse_too_new(version: i64) -> Result<(), CpdError> {
    if version > VERSION {
        return Err(CpdError::new(
            ErrorCode::StoreTooNew,
            format!("The database has schema version {version}, newer than this cpd's {VERSION}: upgrade cpd."),
        )
        .with("version", version)
        .with("supported", VERSION));
    }
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

/// Adds the database's path, and to `store_busy` its `busy_ms` and the
/// `holder` of the write lock (§4.2), to errors made from a `rusqlite::Error`,
/// which cannot know them.
fn enrich(e: CpdError, path: &Path, busy_ms: u64) -> CpdError {
    match e.code() {
        ErrorCode::StoreFailed | ErrorCode::StoreBusy if !e.details().contains_key("path") => {
            let e = e.with("path", path.display().to_string());
            if e.code() != ErrorCode::StoreBusy {
                return e;
            }
            let holder = Holder::find(path);
            let message = match &holder {
                Some(h) => format!("{} Held by {}.", e.message(), h.describe()),
                None => format!("{} Its holder is unknown: no process has recorded that it holds the lock.", e.message()),
            };
            let mut busy = CpdError::new(ErrorCode::StoreBusy, message);
            for (key, value) in e.details() {
                busy = busy.with(key, value.clone());
            }
            busy.with("busy_ms", busy_ms).with("holder", holder.map_or(Value::Null, |h| h.to_json()))
        }
        _ => e,
    }
}

/// Who holds the write lock (§4.2). A process that holds it records itself in
/// `locks/store-writer-<pid>.json` (one file per process, so none removes
/// another's): written right after `BEGIN IMMEDIATE`, replaced atomically,
/// removed when the transaction ends. A `store_busy` reads it, to name the
/// holder. The record is evidence, not state: a write that cannot be made is
/// skipped, never a reason to fail the transaction.
#[derive(Default)]
struct Holder {
    file: Option<PathBuf>,
}

/// A record read back: the process that held the write lock, and since when.
struct Holding {
    pid: u32,
    command: String,
    argv: String,
    session: Option<String>,
    attempt: Option<String>,
    since_ms: u64,
}

impl Holding {
    fn describe(&self) -> String {
        let who = self.session.as_deref().map(|s| format!(", session {s}")).unwrap_or_default();
        let which = self.attempt.as_deref().map(|a| format!(", attempt {a}")).unwrap_or_default();
        format!(
            "pid {} (`{}`; {}{who}{which}) for {:.1} s",
            self.pid,
            self.command,
            self.argv,
            self.since_ms as f64 / 1000.0
        )
    }

    fn to_json(&self) -> Value {
        json!({
            "pid": self.pid, "command": self.command, "argv": self.argv,
            "session": self.session, "attempt": self.attempt, "since_ms": self.since_ms,
        })
    }
}

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn locks_of(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join("locks")
}

impl Holder {
    fn record(&mut self, db: &Path, command: &str) {
        let pid = std::process::id();
        let dir = locks_of(db);
        let session = ["CPD_SESSION", "PI_SESSION_ID"].iter().find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()));
        let record = json!({
            "pid": pid,
            "command": command,
            "argv": std::env::args().collect::<Vec<_>>().join(" "),
            "session": session,
            "attempt": std::env::var("CPD_ATTEMPT").ok().filter(|v| !v.is_empty()),
            "since_unix_ms": unix_ms(),
        });
        let (tmp, file) = (dir.join(format!("store-writer-{pid}.tmp")), dir.join(format!("store-writer-{pid}.json")));
        if fs::write(&tmp, record.to_string()).and_then(|()| fs::rename(&tmp, &file)).is_ok() {
            self.file = Some(file);
        }
    }

    /// The newest record of a process that is still alive: a killed holder
    /// leaves its file behind, and SQLite released its lock with it.
    fn find(db: &Path) -> Option<Holding> {
        let entries = fs::read_dir(locks_of(db)).ok()?;
        let now = unix_ms();
        entries
            .filter_map(Result::ok)
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("store-writer-") && name.ends_with(".json")
            })
            .filter_map(|e| serde_json::from_str::<Value>(&fs::read_to_string(e.path()).ok()?).ok())
            .filter_map(|v| {
                let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
                let alive = match kill(Pid::from_raw(i32::try_from(pid).ok()?), None) {
                    Ok(()) | Err(Errno::EPERM) => true,
                    Err(_) => false,
                };
                let since = v["since_unix_ms"].as_u64()?;
                let text = |k: &str| v[k].as_str().map(str::to_owned);
                let command = text("command")?;
                alive.then(|| Holding {
                    pid,
                    command,
                    argv: text("argv").unwrap_or_default(),
                    session: text("session"),
                    attempt: text("attempt"),
                    since_ms: now.saturating_sub(since),
                })
            })
            .min_by_key(|h| h.since_ms)
    }
}

/// A write transaction that held the lock this long is written to `run/store-slow.log`.
const SLOW_HOLD: Duration = Duration::from_millis(1000);

impl Holder {
    /// One JSON line in `run/store-slow.log`: who held the write lock for [`SLOW_HOLD`] or more, and where
    /// the time went (`begin_ms`: the `txns` row; `closure_ms`: the work; `commit_ms`: the commit, which syncs).
    /// Evidence for §4.2's `store_busy`; best effort like the record.
    fn report_slow(db: &Path, command: &str, phases: Value) {
        use std::io::Write;
        let dir = db.parent().unwrap_or(Path::new(".")).join("run");
        let line = json!({
            "at": now(), "pid": std::process::id(), "command": command,
            "argv": std::env::args().collect::<Vec<_>>().join(" "), "phases": phases,
        });
        let _ = fs::create_dir_all(&dir).and_then(|()| {
            let mut file = OpenOptions::new().create(true).append(true).open(dir.join("store-slow.log"))?;
            writeln!(file, "{line}")
        });
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = fs::remove_file(file);
        }
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
        let err = store.migrate(&home).unwrap_err();
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
}
