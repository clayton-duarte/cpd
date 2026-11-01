//! The migration `tier-catalog-source` (contract §4.4, §4.5; pro-d26, Plan bifrost, PR catfp): `tier_catalog` is rebuilt empty
//! with `models_sha` in its primary key, `(pi_version, models_sha, letter, model)`. It is a cache: the rows are recomputed by the
//! next `cpd tiers`, so the rebuild drops them.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

/// A store as every migration before `tier-catalog-source` leaves it, with cached rows. The test fails here while the migration
/// is missing.
fn before_tier_catalog_source() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED.iter().copied().take_while(|n| !n.name.ends_with("-tier-catalog-source")).collect();
    assert!(earlier.len() < NAMED.len(), "tier-catalog-source is a built-in migration");
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut store,
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
           ('1.3.0', 'O', 'bifrost/vertex/claude-opus-5', '[0,1,2,3,4,5]', 't1'),
           ('1.3.0', 'S', 'bifrost/vertex/claude-sonnet-5-5', '[0,1,2,3]', 't1'),
           ('1.3.0', 'H', 'bifrost/vertex/claude-haiku-5', '[0,1,2,3]', 't1');",
    )
    .unwrap();
    drop(store);
    (dir, home, earlier)
}

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn count(store: &mut Store, sql: &'static str) -> i64 {
    store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// A store with the migration applied over a cache that held rows.
fn migrated() -> (tempfile::TempDir, CpdHome, Store) {
    let (dir, home, _earlier) = before_tier_catalog_source();
    let store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    (dir, home, store)
}

const INSERT: &str = "INSERT INTO tier_catalog (pi_version, models_sha, letter, model, efforts, read_at) VALUES";

#[test]
fn tier_catalog_migration_puts_models_sha_in_the_primary_key_in_the_documented_order() {
    let (_dir, _home, mut store) = migrated();
    let columns: Vec<(String, i64)> = store.read(|c| Ok(table_columns(c, "tier_catalog")?)).unwrap();
    let names: Vec<&str> = columns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["pi_version", "models_sha", "letter", "model", "efforts", "read_at"]);
    let key: Vec<&str> = {
        let mut keyed: Vec<&(String, i64)> = columns.iter().filter(|(_, pk)| *pk > 0).collect();
        keyed.sort_by_key(|(_, pk)| *pk);
        keyed.into_iter().map(|(n, _)| n.as_str()).collect()
    };
    assert_eq!(key, ["pi_version", "models_sha", "letter", "model"]);
    let not_null: i64 = count(&mut store, "SELECT \"notnull\" FROM pragma_table_info('tier_catalog') WHERE name = 'models_sha'");
    assert_eq!(not_null, 1, "models_sha is NOT NULL");
    let default: String = store
        .read(|c| Ok(c.query_row("SELECT dflt_value FROM pragma_table_info('tier_catalog') WHERE name = 'models_sha'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(default, "''", "models_sha DEFAULT '' means exactly: no models.json");
}

#[test]
fn tier_catalog_migration_rebuilds_the_table_empty_because_it_is_a_cache() {
    let (_dir, home, _earlier) = before_tier_catalog_source();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    assert_eq!(count(&mut store, "SELECT count(*) FROM tier_catalog"), 0, "the rows are dropped: the next `cpd tiers` recomputes them");
}

#[test]
fn tier_catalog_migration_keys_rows_by_models_sha_and_still_refuses_a_duplicate_key_and_a_bad_letter() {
    let (_dir, _home, mut store) = migrated();
    write(&mut store, &format!("{INSERT} ('1.3.0', '', 'S', 'm', '[0,1]', 't')")).unwrap();
    write(&mut store, &format!("{INSERT} ('1.3.0', 'abc', 'S', 'm', '[0,1,2]', 't')")).unwrap();
    let again = write(&mut store, &format!("{INSERT} ('1.3.0', 'abc', 'S', 'm', '[0]', 't')"));
    assert!(format!("{again:?}").contains("UNIQUE constraint failed"), "{again:?}");
    let letter = write(&mut store, &format!("{INSERT} ('1.3.0', 'abc', 'G', 'm', '[0]', 't')"));
    assert!(format!("{letter:?}").contains("CHECK constraint failed"), "{letter:?}");
    let null_sha = write(&mut store, "INSERT INTO tier_catalog (pi_version, models_sha, letter, model, efforts, read_at) VALUES ('1.3.0', NULL, 'O', 'm', '[0]', 't')");
    assert!(null_sha.is_err(), "models_sha is NOT NULL");
}

#[test]
fn tier_catalog_migration_keeps_the_history_triggers_and_the_registry_of_the_other_rebuilds() {
    let (_dir, _home, mut store) = migrated();
    let triggers: Vec<String> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'tier_catalog' ORDER BY name")?;
            Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    assert_eq!(triggers, ["tier_catalog_hist_del", "tier_catalog_hist_ins", "tier_catalog_hist_upd", "tier_catalog_no_delete"]);
    assert_eq!(count(&mut store, "SELECT no_delete FROM history_tables WHERE name = 'tier_catalog'"), 1, "registered once, append-only");
    assert_eq!(count(&mut store, "SELECT count(*) FROM history_tables WHERE name = 'tier_catalog'"), 1);

    // The triggers work on the rebuilt table: an insert is a history row that lists models_sha; a delete is refused.
    let before = count(&mut store, "SELECT count(*) FROM history WHERE tbl = 'tier_catalog'");
    write(&mut store, &format!("{INSERT} ('1.3.0', 'abc', 'S', 'm', '[0,1]', 't')")).unwrap();
    assert_eq!(count(&mut store, "SELECT count(*) FROM history WHERE tbl = 'tier_catalog'"), before + 1);
    let with_sha = count(&mut store, "SELECT count(*) FROM history WHERE tbl = 'tier_catalog' AND instr(after, 'models_sha') > 0");
    assert_eq!(with_sha, 1, "the history row carries the new column");
    let refused = write(&mut store, "DELETE FROM tier_catalog");
    assert!(format!("{refused:?}").contains("append_only"), "{refused:?}");
}

#[test]
fn tier_catalog_migration_is_recorded_once_and_a_second_open_changes_nothing() {
    let (_dir, home, mut first) = migrated();
    write(&mut first, &format!("{INSERT} ('1.3.0', 'abc', 'S', 'm', '[0,1]', 't')")).unwrap();
    let applied = crate::store::tests::applied(&mut first);
    assert_eq!(applied.iter().filter(|a| format!("{a:?}").contains("tier-catalog-source")).count(), 1, "{applied:?}");
    let hash = first.read(|c| Ok(canonical_hash(c)?)).unwrap();
    drop(first);

    let mut second = Store::open(&home, 5000).unwrap();
    assert_eq!(crate::store::tests::applied(&mut second), applied);
    assert_eq!(count(&mut second, "SELECT count(*) FROM tier_catalog"), 1, "a second open does not rebuild the table again");
    assert_eq!(second.read(|c| Ok(canonical_hash(c)?)).unwrap(), hash);
}
