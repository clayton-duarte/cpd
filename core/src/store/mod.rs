//! The SQLite store (contract §4.1–§4.5): opening `$CPD_HOME/cpd.db`, read and
//! write transactions, `txns` and `cpd_txn()`, the DDL, migrations, the schema
//! check and the store's part of the tamper report.
//!
//! Every module reads through [`Store::read`] and writes through
//! [`Store::write`]; nothing else writes the database (§4.3).

#[cfg(test)]
mod clock_tests;
mod lock;
mod schema;
#[doc(hidden)] pub mod testing;
mod verdict;
#[cfg(test)]
mod version_tests;

use std::fs::{self, OpenOptions};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, ErrorCode as SqliteCode, OpenFlags, Transaction, TransactionBehavior};
use serde_json::{Value, json};

use crate::clock::{Clock, SystemClock};
use crate::context::legacy;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::spawn::Pid as SpawnPid;
use crate::ports::Ports;

use lock::{MIGRATION_SETTLE_MS, SetupLock, setup_lock};
pub use lock::MIGRATION_WAIT_CEILING;
use schema::{LEGACY, Named};
pub use schema::{SCHEMA_HASH, VERSION, canonical_hash};
use verdict::verdict_with;
pub use verdict::{Start, Verdict, refuse_user_version, verdict};

/// `PRAGMA application_id` of a cpd 2 database: `0x63706432`, "cpd2" (§4.1).
pub const APPLICATION_ID: i32 = 0x6370_6432;

/// The `history` rows whose `txn` has no `txns` row (the tamper report's
/// second part). `INDEXED BY history_txn` makes the planner scan the small
/// covering index instead of the `history` table (its rows hold every column
/// twice); the index is §4.4's, so a missing one is an error, not a silent scan.
pub const ORPHAN_HISTORY_SQL: &str =
    "SELECT h.id, h.txn FROM history h INDEXED BY history_txn WHERE NOT EXISTS (SELECT 1 FROM txns t WHERE t.id = h.txn) ORDER BY h.id";

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
    /// The clock the busy back-off of [`Store::read`] and [`Store::write`] sleeps on: the system's until [`Store::use_clock`].
    clock: Arc<dyn Clock>,
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
/// `step` is the longest single nap of the wait, so `give_up` is polled between naps.
fn settle<T>(
    clock: &dyn Clock,
    step: Duration,
    persist: &mut Option<Persist>,
    wait: &mut Wait,
    result: Result<T, CpdError>,
) -> Result<Option<T>, CpdError> {
    let Some(p) = persist else { return result.map(Some) };
    match result {
        Ok(value) => {
            if let Some(since) = wait.since.take() {
                (p.log)(&format!("the store is clear again after {:.1} s", clock.now().saturating_duration_since(since).as_secs_f64()));
                *wait = Wait::new();
            }
            Ok(Some(value))
        }
        Err(e) if is_transient(&e) && !(p.give_up)() => {
            let code = e.code().as_str().to_owned();
            wait.since.get_or_insert_with(|| clock.now());
            if !wait.codes.contains(&code) {
                (p.log)(&format!("{code}: {} Waiting and trying again until it clears.", e.message()));
                wait.codes.push(code);
            }
            let until = clock.now() + wait.delay;
            while clock.now() < until && !(p.give_up)() {
                clock.sleep(step.min(until.saturating_duration_since(clock.now())));
            }
            wait.delay = (wait.delay * 2).min(BACKOFF_MAX);
            Ok(None)
        }
        Err(e) => Err(e),
    }
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
        Store::open_with(&SystemClock, home, busy_ms, None)
    }

    /// [`Store::open`] on `clock`: every wait of the open (the busy back-off of `persist`, the retry of the setup
    /// lock) sleeps on it. With `persist`, a transient error is waited out as [`Store::open_persistent`] does,
    /// until [`lock::MIGRATION_WAIT_CEILING`] of `clock` time has passed since the first one; then the error returns.
    pub fn open_with(clock: &dyn Clock, home: &CpdHome, busy_ms: u64, persist: Option<Persist>) -> Result<Store, CpdError> {
        let mut persist = persist;
        let mut wait = Wait::new();
        loop {
            let result = Store::open_catalog(clock, home, busy_ms, &Catalog::BUILT_IN);
            if let (Err(e), Some(since)) = (&result, wait.since)
                && is_transient(e)
                && clock.now().saturating_duration_since(since) >= lock::MIGRATION_WAIT_CEILING
            {
                return result;
            }
            if let Some(mut store) = settle(clock, Duration::MAX, &mut persist, &mut wait, result)? {
                store.persist = persist;
                return Ok(store);
            }
        }
    }

    /// [`Store::open`] with the migrations and the pinned hash of `catalog`:
    /// the tests' way to run migrations this binary does not have.
    fn open_catalog(clock: &dyn Clock, home: &CpdHome, busy_ms: u64, catalog: &Catalog<'_>) -> Result<Store, CpdError> {
        let path = home.db();
        let setup = setup_lock(clock, home, busy_ms)?;
        // An opener that waited out a migration may meet its holder still finishing (closing its connection, which
        // checkpoints): its own setup waits for SQLite at least `MIGRATION_SETTLE_MS`, whatever `busy_ms` (§4.2).
        let setup_ms = if setup.waited_for_migration() { busy_ms.max(MIGRATION_SETTLE_MS) } else { busy_ms };
        let mut store = Store::connect(&path, busy_ms, setup_ms).map_err(|e| enrich(e, &path, busy_ms))?;
        store.migrate(home, catalog, &setup).map_err(|e| enrich(e, &path, busy_ms))?;
        drop(setup);
        store.conn.busy_timeout(Duration::from_millis(busy_ms)).map_err(|e| enrich(e.into(), &path, busy_ms))?;
        store.schema = schema_mismatch(&store.conn, home, catalog).map_err(|e| enrich(e, &path, busy_ms))?;
        Ok(store)
    }

    /// [`Store::open`] for a process that persists (see [`Persist`]): a
    /// transient error while opening is waited out, and the store it returns
    /// persists too.
    pub fn open_persistent(home: &CpdHome, busy_ms: u64, persist: Persist) -> Result<Store, CpdError> {
        Store::open_persistent_with(Arc::new(SystemClock), home, busy_ms, persist)
    }

    /// [`Store::open_persistent`] on `clock`: its waits, and those of the store it returns, sleep on it.
    pub fn open_persistent_with(clock: Arc<dyn Clock>, home: &CpdHome, busy_ms: u64, persist: Persist) -> Result<Store, CpdError> {
        let mut wait = Wait::new();
        let mut persist = Some(persist);
        loop {
            let opened = Store::open_with(&*clock, home, busy_ms, None);
            if let Some(mut store) = settle(&*clock, Duration::from_millis(50), &mut persist, &mut wait, opened)? {
                store.persist = persist;
                store.clock = clock;
                return Ok(store);
            }
        }
    }

    /// From now on the busy back-off of [`Store::read`] and [`Store::write`] reads and sleeps on `clock`
    /// (the engine's `Ports.clock`; a test's virtual time).
    pub fn use_clock(&mut self, clock: Arc<dyn Clock>) {
        self.clock = clock;
    }

    /// From now on, every [`Store::read`] and [`Store::write`] of this store
    /// persists (see [`Persist`]); replaces the earlier one.
    pub fn persist(&mut self, persist: Persist) {
        self.persist = Some(persist);
    }

    /// `setup_ms` is the connection's busy timeout until the caller sets it back to `busy_ms` (the setup's, see
    /// [`MIGRATION_SETTLE_MS`]).
    fn connect(path: &Path, busy_ms: u64, setup_ms: u64) -> Result<Store, CpdError> {
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
        conn.busy_timeout(Duration::from_millis(setup_ms))?;
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
        Ok(Store { conn, path: path.to_owned(), busy_ms, txn, schema: None, persist: None, clock: Arc::new(SystemClock) })
    }

    /// §4.5: runs what [`plan`] says, in one `BEGIN IMMEDIATE` transaction.
    ///
    /// The transaction runs with foreign keys off and `legacy_alter_table` on
    /// (§4.5 migration 4, step 1: both only take effect outside a
    /// transaction), so a migration can rebuild a table that others reference;
    /// `PRAGMA foreign_key_check` then holds before the commit, and so does
    /// the schema check of §4.3 (hs-n121). Both are restored afterwards,
    /// whether the migration committed or not.
    ///
    /// When the migration that imports the old Context stores (§4.5 legacy
    /// migration 0007) committed, the folders are moved to `archive/contexts/` (§1.4).
    fn migrate(&mut self, home: &CpdHome, catalog: &Catalog<'_>, setup: &SetupLock) -> Result<(), CpdError> {
        let Some(plan) = verdict_with(&self.conn, catalog)?.into_plan()? else { return Ok(()) };
        // Before the first write, and while the `flock` is held: whoever waits for it learns that it is a migration.
        let (from, to) = match plan.start {
            Start::Fresh => (0, LEGACY.len() + plan.pending.len()),
            Start::Convert(version) => (version as usize, LEGACY.len() + plan.pending.len()),
            Start::Named => {
                let applied: i64 = self.conn.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))?;
                (applied as usize, applied as usize + plan.pending.len())
            }
        };
        setup.migrating(from, to)?;
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        self.conn.pragma_update(None, "legacy_alter_table", "ON")?;
        // The commit appends the migration's pages to the WAL and nothing more. The checkpoint SQLite runs inside a
        // commit past 1000 pages (a rebuilt `events` is thousands) copies them into the database and syncs it; a
        // plain commit's checkpoint took 1.3 to 2.4 s under load (`run/store-slow.log`), and this one would hold
        // `locks/store.lock` for it (hs-n278). The next commit of any process checkpoints, outside the setup lock.
        let autocheckpoint: i64 = self.conn.pragma_query_value(None, "wal_autocheckpoint", |r| r.get(0))?;
        self.conn.pragma_update(None, "wal_autocheckpoint", 0)?;
        let migrated = self.migrate_in_txn(home, catalog);
        let restored = self
            .conn
            .pragma_update(None, "wal_autocheckpoint", autocheckpoint)
            .and_then(|()| self.conn.pragma_update(None, "legacy_alter_table", "OFF"))
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
        let mut holder = Holder::default();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        holder.record(&self.path, MIGRATE_COMMAND);
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
        // The check of §4.3, made before the commit: a schema the binary does
        // not describe is never committed (§4.5, hs-n121).
        if let Some(mismatch) = schema_mismatch(&tx, home, catalog)? {
            let mut err = CpdError::new(
                ErrorCode::MigrationFailed,
                format!(
                    "Migration of the database failed: its schema does not match this cpd's (expected {}, found {}). Nothing was migrated.",
                    mismatch.expected, mismatch.found
                ),
            )
            .with("migration", "schema_check")
            .with("expected", mismatch.expected)
            .with("actual", mismatch.found);
            if let Some(object) = mismatch.object {
                err = err.with("object", object);
            }
            return Err(err);
        }
        tx.commit()?;
        Ok(imported_old_stores)
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
    /// Nothing inside it writes: `PRAGMA query_only` is on for its duration, so
    /// DML, DDL and `TEMP` tables fail with SQLite's `attempt to write a
    /// readonly database` (store-G3), triggers or not.
    pub fn read<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T, CpdError>) -> Result<T, CpdError> {
        let (path, busy_ms) = (self.path.clone(), self.busy_ms);
        let clock = Arc::clone(&self.clock);
        let mut wait = Wait::new();
        let result = (|| {
            self.conn.pragma_update(None, "query_only", true)?;
            let read: Result<T, CpdError> = (|| {
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
                    if let Some(tx) = settle(&*clock, Duration::from_millis(50), &mut self.persist, &mut wait, begun.map_err(|e| enrich(e, &path, busy_ms)))? {
                        break tx;
                    }
                };
                let value = f(&tx)?;
                tx.commit()?;
                Ok(value)
            })();
            // Off again however the read ended: the next `write` needs it.
            let off = self.conn.pragma_update(None, "query_only", false);
            let value = read?;
            off?;
            Ok(value)
        })();
        result.map_err(|e| enrich(e, &path, busy_ms))
    }

    /// `PRAGMA data_version` on this connection: changes only when another connection commits.
    /// SQLite leaves it alone for this connection's own commits, so a caller that compares two reads learns that
    /// another process wrote, at the cost of one pragma and no query.
    pub fn data_version(&mut self) -> Result<i64, CpdError> {
        Ok(self.conn.pragma_query_value(None, "data_version", |r| r.get(0))?)
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
        let clock = Arc::clone(&self.clock);
        let _guard = TxnGuard(&self.txn);
        let mut wait = Wait::new();
        // Declared before the transaction: dropped, so the record removed, after its commit or rollback.
        let mut holder = Holder::default();
        let asked = clock.now();
        let result = (|| {
            let tx = loop {
                let begun = Transaction::new(&mut self.conn, TransactionBehavior::Immediate)
                    .map_err(|e| enrich(CpdError::from(e), &path, busy_ms));
                if let Some(tx) = settle(&*clock, Duration::from_millis(50), &mut self.persist, &mut wait, begun)? {
                    break tx;
                }
            };
            let begun = clock.now();
            holder.record(&path, command);
            // Another process may have migrated since this process opened.
            refuse_user_version(user_version(&tx)?)?;
            let id = begin_txn(&tx, &self.txn, command, &caller)?;
            let write = WriteTxn { tx, id };
            let ran = clock.now();
            let value = f(&write)?;
            let committing = clock.now();
            write.tx.commit()?;
            let held = clock.now().saturating_duration_since(begun);
            if held >= SLOW_HOLD {
                let ms = |d: Duration| d.as_millis() as u64;
                let phases = json!({
                    "waited_ms": ms(begun - asked), "begin_ms": ms(ran - begun),
                    "closure_ms": ms(committing - ran), "commit_ms": ms(clock.now().saturating_duration_since(committing)), "held_ms": ms(held),
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
            report.push(json!({"kind": "schema", "expected": m.expected, "found": m.found, "object": m.object}));
        }
        let orphans = self.read(|conn| {
            let mut stmt = conn.prepare(ORPHAN_HISTORY_SQL)?;
            let rows = stmt
                .query_map([], |r| Ok(json!({"kind": "history", "id": r.get::<_, i64>(0)?, "txn": r.get::<_, i64>(1)?})))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        report.extend(orphans);
        Ok(report)
    }
}

/// The schema check (§4.3): every generated history trigger is what the
/// generator makes, and the store has what this binary's migrations
/// created: the canonical hash is the catalog's pinned one, or, when the
/// store applied additive migrations this binary does not know (inside the
/// window, §4.5), the canonical subset of the objects this binary's
/// migrations created ([`verdict::subset_mismatch`]).
///
/// Run at every open and inside the migrating transaction, before its commit.
fn schema_mismatch(conn: &Connection, home: &CpdHome, catalog: &Catalog<'_>) -> Result<Option<SchemaMismatch>, CpdError> {
    if let Some((name, expected, found)) = schema::generated_mismatch(conn)? {
        return Ok(Some(SchemaMismatch { expected, found: found.unwrap_or_else(|| "missing".to_owned()), object: Some(name) }));
    }
    if !verdict_with(conn, catalog)?.unknown.is_empty() {
        return verdict::subset_mismatch(conn, catalog, home);
    }
    let found = canonical_hash(conn)?;
    Ok((found != catalog.schema_hash).then(|| SchemaMismatch { expected: catalog.schema_hash.to_owned(), found, object: None }))
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
        rusqlite::params![id, crate::clock::now(), command, caller.pid, caller.session, caller.cwd],
    )?;
    Ok(id)
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

/// The messages the triggers raise with `RAISE(ABORT, '…')` that are a typed
/// refusal (§4.4), with the code and the text of each. A new typed trigger
/// error is one more row here. The others (`append_only: …`, `… must be
/// cpd_txn()`) stay `store_failed` by design.
const TRIGGER_ERRORS: [(&str, ErrorCode, &str); 5] = [
    ("job_locked", ErrorCode::JobLocked, "The Job is Locked: its Spec and Dependencies cannot change."),
    ("illegal_transition", ErrorCode::IllegalTransition, "The state change is not a legal transition (§3)."),
    (
        "pr_locked",
        ErrorCode::PrLocked,
        "The PR is Locked: its repo, base and needs cannot change after the Plan's lock.",
    ),
    ("needs_other_pr", ErrorCode::NeedsOtherPr, "A Dependency can only link Jobs of the same PR."),
    (
        "spec_invalid",
        ErrorCode::SpecInvalid,
        "The Job's kind and its Spec do not agree: only a test Job has tests and test_files, only an impl Job approved tests, only a work Job is a review Job.",
    ),
];

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
                let alive = Ports::current().spawn.alive(SpawnPid(pid));
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
            "at": crate::clock::now(), "pid": std::process::id(), "command": command,
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
/// busy timeout is `store_busy`, and so is `SQLITE_LOCKED`; the triggers' `RAISE(ABORT, 'job_locked')`,
/// `'illegal_transition'`, `'pr_locked'`, `'needs_other_pr'` and `'spec_invalid'` (§4.4) are
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
            Some(f) if matches!(f.code, SqliteCode::DatabaseBusy | SqliteCode::DatabaseLocked) => CpdError::new(
                ErrorCode::StoreBusy,
                "Another cpd process held the database's write lock for longer than store_busy_ms.",
            )
            .with("sqlite_message", message),
            Some(f) if f.code == SqliteCode::ConstraintViolation => {
                match TRIGGER_ERRORS.iter().find(|(raised, ..)| *raised == message) {
                    Some(&(_, code, text)) => CpdError::new(code, text).with("sqlite_message", message),
                    None => store_failed(sqlite, message),
                }
            }
            _ => store_failed(sqlite, message),
        }
    }
}

fn store_failed(sqlite: Option<rusqlite::ffi::Error>, message: String) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database failed: {message}."))
        .with("sqlite_code", sqlite.map_or(Value::Null, |f| Value::from(f.extended_code)))
        .with("sqlite_message", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// store-G4: every message a trigger raises is a typed refusal or one of the
    /// two untyped families, so a new `RAISE` cannot fall to `store_failed` by accident.
    #[test]
    fn every_message_a_trigger_raises_is_typed_or_untyped_by_design() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/migrations");
        let mut seen = std::collections::BTreeSet::new();
        for file in fs::read_dir(dir).unwrap() {
            let sql = fs::read_to_string(file.unwrap().path()).unwrap();
            for part in sql.split("RAISE(ABORT, '").skip(1) {
                seen.insert(part.split('\'').next().unwrap().to_owned());
            }
        }
        assert!(seen.len() >= 8, "{seen:?}");
        for message in seen {
            let failure = rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER);
            let err = CpdError::from(rusqlite::Error::SqliteFailure(failure, Some(message.clone())));
            // `review_pending` guards a code path that forgot to close the Plan's reviews: a bug, so untyped by design.
            // `working_pr_invalid` is the same: the writer checks the Lead and the PR first, so it never fires in practice.
            let untyped = message.starts_with("append_only: ") || message.ends_with(" must be cpd_txn()") || message == "review_pending" || message == "working_pr_invalid";
            assert_eq!(err.code() == ErrorCode::StoreFailed, untyped, "{message}: {err:?}");
            assert_eq!(err.details()["sqlite_message"], message);
        }
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
    const A_AND_B_HASH: &str = "07016dfaefc95f920571eb82da3a3e99fcb26ebd9a2ab0a567e2e6310abfecf3";

    /// A store opened with `named` as its migrations, whose pinned hash is the one of a fresh store with them.
    pub(super) fn open_named(home: &CpdHome, named: &[Named]) -> Result<Store, CpdError> {
        let probe = tempfile::tempdir().unwrap();
        let probe_home = CpdHome::resolve(Some(probe.path().join("cpd").into()), None).unwrap();
        // Even a refusal of `home` is judged against the pinned hash of a fresh store with the same list.
        let pinned = canonical_hash(&verdict::reference_database(&Catalog { named, schema_hash: "" }, &probe_home)?)?;
        Store::open_catalog(&SystemClock, home, 5000, &Catalog { named, schema_hash: &pinned })
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
    const KIND_POKED: Named = Named {
        name: "20300202T0000-kind-session-poked",
        compat: schema::Compat::Additive,
        sql: "-- cpd-migration: additive\nINSERT INTO event_kinds (name) VALUES ('session-poked');\n",
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
        // One row added to `event_kinds` by a later binary's migration makes it insertable.
        let mut store = open_named(&home, &with_kind_tables(&[KIND_PINNED])).unwrap();
        assert!(kinds(&mut store).contains(&"plan-pinned".to_owned()));
        insert_event(&mut store, "plan-pinned").unwrap();
        // The other kinds stay, and a kind nobody added stays refused.
        insert_event(&mut store, "plan-created").unwrap();
        assert!(insert_event(&mut store, "session-poked").is_err());
        assert_eq!(applied(&mut store).last().map(|a| a.0.as_str()), Some(KIND_PINNED.name));
    }

    #[test]
    fn two_plans_kind_migrations_applied_in_either_order_keep_both_kinds() {
        let (_a, pinned_first) = tmp_home();
        let (_b, poked_first) = tmp_home();
        let (_c, together) = tmp_home();
        let both = with_kind_tables(&[KIND_PINNED, KIND_POKED]);
        drop(open_named(&pinned_first, &with_kind_tables(&[KIND_PINNED])).unwrap());
        let mut a = open_named(&pinned_first, &both).unwrap();
        drop(open_named(&poked_first, &with_kind_tables(&[KIND_POKED])).unwrap());
        // `KIND_PINNED` sorts before the applied `KIND_POKED`: allowed, it is additive (O2).
        let mut b = open_named(&poked_first, &both).unwrap();
        let mut c = open_named(&together, &both).unwrap();

        let order = |s: &mut Store| applied(s).into_iter().skip(LEGACY.len() + 1).map(|a| a.0).collect::<Vec<_>>();
        assert_eq!(order(&mut a), [KIND_PINNED.name, KIND_POKED.name]);
        assert_eq!(order(&mut b), [KIND_POKED.name, KIND_PINNED.name]);
        let expected = kinds(&mut c);
        assert!(["plan-pinned", "session-poked"].iter().all(|k| expected.contains(&(*k).to_owned())));
        for store in [&mut a, &mut b] {
            assert_eq!(kinds(store), expected, "neither migration dropped the other's kind");
            insert_event(store, "plan-pinned").unwrap();
            insert_event(store, "session-poked").unwrap();
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
        let built_in_and_a: Vec<Named> = schema::NAMED.iter().copied().chain([ADD_A]).collect();
        drop(open_named(&home, &built_in_and_a).unwrap());
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
        for version in [15, 99, 999, 1001] {
            let err = refuse_user_version(version).unwrap_err();
            assert_eq!(err.code(), ErrorCode::StoreTooNew, "{version}");
            assert_eq!(err.details()["user_version"], version);
        }
        for version in [0, 1, 6, 7, 11, 14, 1000] {
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

    // ---------- the conversion's cost (hs-n278) ----------

    /// A legacy store at the last legacy version (what the conversion reads) with `events` Events and as many
    /// `history` rows, each about 2 KB, as one `txns` row: a store of a size that makes `kind-tables` rebuild
    /// thousands of pages. Its Events and history rows have no Context, Plan or Session.
    fn big_legacy_store(home: &CpdHome, events: usize) {
        fs::create_dir_all(home.db().parent().unwrap()).unwrap();
        let conn = Connection::open(home.db()).unwrap();
        conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64)).unwrap();
        conn.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
        for legacy in LEGACY {
            (legacy.apply)(&conn).unwrap();
        }
        conn.pragma_update(None, "user_version", LEGACY.len() as i64).unwrap();
        conn.execute_batch(&format!(
            "BEGIN;
             INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '2026-01-01T00:00:00.000Z', 'x', 1, NULL, '/');
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {events})
               INSERT INTO events (txn, at, kind, text, data)
               SELECT 1, '2026-01-01T00:00:00.000Z', 'session-state', 'Event ' || i, json_object('x', hex(randomblob(1000))) FROM n;
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {events})
               INSERT INTO history (txn, tbl, pk, op, before, after)
               SELECT 1, 'jobs', json_array('p', i), 'update', json_object('x', hex(randomblob(500))), json_object('x', hex(randomblob(500))) FROM n;
             COMMIT;"
        ))
        .unwrap();
    }

    /// Converts the store of [`big_legacy_store`] as `Store::open` does, on a connection that counts SQLite's
    /// virtual machine steps (in thousands). Returns the converted store and the steps.
    fn convert_counting_steps(home: &CpdHome) -> (Store, u64) {
        let mut store = Store::connect(&home.db(), 5000, 5000).unwrap();
        let steps = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counter = Arc::clone(&steps);
        store
            .conn
            .progress_handler(1000, Some(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                false
            }))
            .unwrap();
        let setup = setup_lock(&SystemClock, home, 5000).unwrap();
        store.migrate(home, &Catalog::BUILT_IN, &setup).unwrap();
        store.conn.progress_handler(1000, None::<fn() -> bool>).unwrap();
        (store, steps.load(Ordering::SeqCst))
    }

    #[test]
    fn the_conversion_of_a_large_store_is_one_transaction_with_work_linear_in_its_size() {
        let mut steps = Vec::new();
        for events in [500, 2000] {
            let (_dir, home) = tmp_home();
            big_legacy_store(&home, events);
            let (mut store, counted) = convert_counting_steps(&home);
            let (txns, migrated, history): (i64, i64, i64) = store
                .read(|c| {
                    Ok((
                        c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?,
                        c.query_row("SELECT count(*) FROM schema_migrations WHERE txn = (SELECT max(id) FROM txns)", [], |r| r.get(0))?,
                        c.query_row("SELECT count(*) FROM history", [], |r| r.get(0))?,
                    ))
                })
                .unwrap();
            assert_eq!((txns, migrated), (2, (LEGACY.len() + schema::NAMED.len()) as i64), "{events}: one migrating transaction, which recorded every migration");
            assert_eq!(history, events as i64, "{events}: the conversion writes no history row for an Event or a history row");
            steps.push(counted);
        }
        // Four times the rows, at most five times the work (index builds sort): a per-row statement of Rust, or a
        // pass over the table per row, would be four times that or more.
        assert!(steps[1] <= steps[0] * 5, "500 Events took {} thousand steps, 2000 took {}", steps[0], steps[1]);
    }

    #[test]
    fn the_conversion_commit_leaves_the_checkpoint_to_the_next_commit_and_restores_the_threshold() {
        let (_dir, home) = tmp_home();
        big_legacy_store(&home, 2000);
        let (store, _) = convert_counting_steps(&home);
        // `NOOP`: reports the WAL without checkpointing it. Frames of the migration, none copied to the database.
        let (log, checkpointed): (i64, i64) =
            store.conn.query_row("PRAGMA wal_checkpoint(NOOP)", [], |r| Ok((r.get(1)?, r.get(2)?))).unwrap();
        assert!(log > 1000, "the fixture is smaller than the default threshold: {log} frames");
        assert_eq!(checkpointed, 0, "the migrating commit checkpointed {checkpointed} of {log} frames, under the setup lock");
        let threshold: i64 = store.conn.pragma_query_value(None, "wal_autocheckpoint", |r| r.get(0)).unwrap();
        assert_eq!(threshold, 1000, "SQLite's default, for every later commit");
    }
}
