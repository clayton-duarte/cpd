//! The migration `20261103T2150-context-supersedes-list` (contract §4.5; pro-d4, pro-d1): DDL-free, so a store before it
//! keeps every record row and, after it, takes a row whose `supersedes` is a comma list.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261103T2150-context-supersedes-list";

const ROWS: &str = "SELECT id, supersedes FROM context_records ORDER BY seq";

fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-context-supersedes-list"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "context-supersedes-list is a built-in migration"
    );
    earlier
}

fn rows(store: &mut Store) -> Vec<(String, Option<String>)> {
    store
        .read(|c: &Connection| {
            let mut stmt = c.prepare(ROWS)?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
}

fn insert(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn before() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = crate::store::tests::open_named(&home, &earlier()).unwrap();
    insert(
        &mut store,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'C', 't', 'm');
         INSERT INTO context_records (context, id, kind, at, machine, text, supersedes) VALUES
           ('cpd', 'm-d1', 'decision', 't', 'm', 'one', NULL),
           ('cpd', 'm-d2', 'decision', 't', 'm', 'two', NULL),
           ('cpd', 'm-d3', 'decision', 't', 'm', 'three', 'm-d1');",
    );
    drop(store);
    (dir, home)
}

#[test]
fn the_migration_is_breaking_and_keeps_every_record_row() {
    let entry = NAMED
        .iter()
        .find(|n| n.name == MIGRATION)
        .unwrap_or_else(|| panic!("{MIGRATION} is a built-in migration"));
    assert!(entry.sql.starts_with("-- cpd-migration: breaking after "));
    let (_dir, home) = before();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none());
    assert_eq!(
        rows(&mut store),
        [
            ("m-d1".to_owned(), None),
            ("m-d2".to_owned(), None),
            ("m-d3".to_owned(), Some("m-d1".to_owned())),
        ]
    );
}

#[test]
fn a_migrated_store_accepts_a_multi_target_row() {
    let (_dir, home) = before();
    let mut store = Store::open(&home, 5000).unwrap();
    insert(
        &mut store,
        "INSERT INTO context_records (context, id, kind, at, machine, text, supersedes)
         VALUES ('cpd', 'm-d4', 'decision', 't', 'm', 'four', 'm-d2,m-d3')",
    );
    assert_eq!(rows(&mut store).len(), 4);
    assert_eq!(rows(&mut store)[3].1.as_deref(), Some("m-d2,m-d3"));
}
