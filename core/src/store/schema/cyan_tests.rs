//! The migration `cyan-tier` (contract §4.5; pro-d16): the Inbox tier stored as `blue` is stored as `cyan`.

use rusqlite::Connection;
use serde_json::Value;

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

/// A store as every migration before `cyan-tier` leaves it. The test fails here while the migration is missing.
fn before_cyan_tier() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-cyan-tier"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "cyan-tier is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut store,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES
           ('curate-docs', 'blue', 'curate-docs:ctx', 'ctx: 2 doc candidates', 't1'),
           ('curate-retire', 'blue', 'curate-retire:ctx', 'ctx: 3 to retire', 't2');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at, dismissed_at) VALUES
           ('curate-docs', 'blue', 'curate-docs:ctx', 'ctx: 1 doc candidates', 't0', 't1', 't0b');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('dialog', 'red', 'session:a', 'a', 't3');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at) VALUES ('unread', 'white', 'session:b', 'b', 't4', 't5');",
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

/// Every row of `inbox_items`, every column, by id.
fn items(store: &mut Store) -> Vec<Value> {
    store
        .read(|c: &Connection| {
            let columns = table_columns(c, "inbox_items")?;
            let pairs: Vec<String> = columns
                .iter()
                .map(|(name, _)| format!("'{name}', {name}"))
                .collect();
            let mut stmt = c.prepare(&format!(
                "SELECT json_object({}) FROM inbox_items ORDER BY id",
                pairs.join(", ")
            ))?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .map(|r| serde_json::from_str(&r).unwrap())
                .collect())
        })
        .unwrap()
}

fn history(store: &mut Store) -> i64 {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM history WHERE tbl = 'inbox_items'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn every_blue_row_reads_cyan_and_nothing_else_changes_and_the_check_and_the_open_index_follow() {
    let (_dir, home, earlier) = before_cyan_tier();
    let mut before_store = crate::store::tests::open_named(&home, &earlier).unwrap();
    let before = items(&mut before_store);
    let history_before = history(&mut before_store);
    drop(before_store);
    assert_eq!(
        before.iter().filter(|r| r["urgency"] == "blue").count(),
        3,
        "the fixture: two open and one closed blue row"
    );

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    let want: Vec<Value> = before
        .into_iter()
        .map(|mut r| {
            r["claimed_at"] = Value::Null; // the column `inbox-claim` adds
            r["head_oid"] = Value::Null; // the columns `codereview-followup` adds
            r["reviewed_oid"] = Value::Null;
            if r["urgency"] == "blue" {
                r["urgency"] = Value::from("cyan");
            }
            r
        })
        .collect();
    let after = items(&mut store);
    assert_eq!(
        after, want,
        "every row, column for column; only `blue` is now `cyan`"
    );
    assert!(after.iter().all(|r| r["urgency"] != "blue"));
    assert_eq!(
        history(&mut store),
        history_before,
        "the copy writes no history row"
    );

    // the CHECK
    let refusal = write(
        &mut store,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('curate-docs', 'teal', 'curate-docs:other', 'l', 't')",
    );
    assert!(
        format!("{refusal:?}").contains("CHECK constraint failed"),
        "{refusal:?}"
    );
    write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('curate-docs', 'cyan', 'curate-docs:other', 'l', 't')").unwrap();
    // the open-item unique index
    let twice = write(
        &mut store,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('curate-docs', 'cyan', 'curate-docs:ctx', 'l', 't')",
    );
    assert!(
        twice.is_err(),
        "a second open row of the same (kind, subject, urgency) is refused"
    );
}

#[test]
fn running_the_migration_set_twice_changes_nothing() {
    let (_dir, home, _earlier) = before_cyan_tier();
    let mut first = Store::open(&home, 5000).unwrap();
    let rows = items(&mut first);
    assert!(
        rows.iter().all(|r| r["urgency"] != "blue") && rows.iter().any(|r| r["urgency"] == "cyan"),
        "{rows:?}"
    );
    let applied = crate::store::tests::applied(&mut first);
    let (history_once, hash) = (
        history(&mut first),
        first.read(|c| Ok(canonical_hash(c)?)).unwrap(),
    );
    drop(first);

    let mut second = Store::open(&home, 5000).unwrap();
    assert_eq!(items(&mut second), rows);
    assert_eq!(crate::store::tests::applied(&mut second), applied);
    assert_eq!(history(&mut second), history_once);
    assert_eq!(second.read(|c| Ok(canonical_hash(c)?)).unwrap(), hash);
}
