//! The migration `blue-tier` (contract §4.5; pro-d58): every `review-requested` row is stored as `blue`.

use rusqlite::Connection;
use serde_json::Value;

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

/// A store as every migration before `blue-tier` (20261030T0900) leaves it. The test fails here while the migration is missing.
fn before_blue_tier() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED.iter().copied().take_while(|n| !n.name.ends_with("20261030T0900-blue-tier")).collect();
    assert!(earlier.len() < NAMED.len(), "blue-tier is a built-in migration");
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut store,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES
           ('review-requested', 'yellow', 'pr:acme/pay#1', 'review pay#1', 't1'),
           ('dialog', 'red', 'session:a', 'a', 't2'),
           ('pr-ready', 'yellow', 'pr:acme/pay#2', 'pr 2', 't3'),
           ('plan-review', 'magenta', 'plan:p', 'p', 't4'),
           ('curate-docs', 'cyan', 'curate-docs:ctx', 'c', 't5'),
           ('unread', 'white', 'session:b', 'b', 't6');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at, dismissed_at) VALUES
           ('review-requested', 'yellow', 'pr:acme/pay#0', 'review pay#0', 't0', 't1', 't0b');",
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
            let pairs: Vec<String> = columns.iter().map(|(name, _)| format!("'{name}', {name}")).collect();
            let mut stmt = c.prepare(&format!("SELECT json_object({}) FROM inbox_items ORDER BY id", pairs.join(", ")))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|r| serde_json::from_str(&r).unwrap()).collect())
        })
        .unwrap()
}

/// A store with `blue-tier` applied over an empty inbox.
fn migrated() -> (tempfile::TempDir, Store) {
    let (dir, home, _earlier) = before_blue_tier();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    write(&mut store, "DELETE FROM inbox_items").unwrap();
    (dir, store)
}

#[test]
fn blue_tier_migration_copies_every_review_requested_row_as_blue_and_every_other_row_unchanged() {
    let (_dir, home, earlier) = before_blue_tier();
    let mut before_store = crate::store::tests::open_named(&home, &earlier).unwrap();
    let before = items(&mut before_store);
    drop(before_store);
    assert_eq!(before.iter().filter(|r| r["kind"] == "review-requested").count(), 2, "the fixture: one open and one closed row");

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    let want: Vec<Value> = before
        .into_iter()
        .map(|mut r| {
            if r["kind"] == "review-requested" {
                r["urgency"] = Value::from("blue");
            }
            r
        })
        .collect();
    assert_eq!(items(&mut store), want, "every row, column for column; only `review-requested` is now `blue`");
}

#[test]
fn blue_tier_migration_accepts_blue_and_still_refuses_an_unknown_tier() {
    let (_dir, mut store) = migrated();
    write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('review-requested', 'blue', 'pr:x/y#1', 'l', 't')").unwrap();
    let refusal = write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('review-requested', 'purple', 'pr:x/y#2', 'l', 't')");
    assert!(format!("{refusal:?}").contains("CHECK constraint failed"), "{refusal:?}");
}

#[test]
fn blue_tier_migration_keeps_the_open_item_identity_index() {
    let (_dir, mut store) = migrated();
    let insert = |closed: &str| {
        format!("INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at) VALUES ('review-requested', 'blue', 'pr:x/y#1', 'l', 't', {closed})")
    };
    write(&mut store, &insert("NULL")).unwrap();
    assert!(write(&mut store, &insert("NULL")).is_err(), "a second open row of the same (kind, subject, urgency) is refused");
    write(&mut store, "UPDATE inbox_items SET closed_at = 't2'").unwrap();
    write(&mut store, &insert("NULL")).unwrap();
    let closed_index: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'inbox_items_closed'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(closed_index, 1);
}
