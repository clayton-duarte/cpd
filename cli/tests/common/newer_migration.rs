//! A migration this binary lacks, applied by hand as a newer binary would have (contract §4.5): one copy, for the store
//! tests of the window and the engine's stand-down.

use std::path::Path;

use cpd_core::home::CpdHome;
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

/// What a newer binary does: runs `sql` and records the migration `name` with `compat` (cpd's own `cpd_txn()` is
/// stood in by the id of the store's newest `txns` row). `cpd_home` is the `$CPD_HOME` path.
pub fn apply_newer(cpd_home: &Path, name: &str, compat: &str, sql: &str) {
    let conn = Connection::open(CpdHome::resolve(Some(cpd_home.into()), None).unwrap().db()).unwrap();
    let txn: i64 = conn.query_row("SELECT max(id) FROM txns", [], |r| r.get(0)).unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn)).unwrap();
    conn.execute_batch(sql).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
         VALUES (?1, ?2, lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?3, 0)",
        rusqlite::params![name, compat, txn],
    )
    .unwrap();
}
