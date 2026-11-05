//! The migration `models-changed` (contract §4.5; pro-d13, pro-d16): the Inbox kind `models-changed` joins the `kind` CHECK of
//! `inbox_items`, which SQLite can only change by rebuilding the table. The rebuild is made from the newest shape on `main`
//! (every column, the `blue` tier) and loses no row.

use rusqlite::Connection;
use serde_json::Value;

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

/// Every kind a store can hold before the migration, with the tier its rule gives it.
const KINDS: &[(&str, &str)] = &[
    ("dialog", "red"),
    ("plan-review", "magenta"),
    ("unread", "white"),
    ("job-stuck", "red"),
    ("engine-error", "red"),
    ("question", "magenta"),
    ("pr-failing", "yellow"),
    ("pr-changes", "yellow"),
    ("pr-ready", "yellow"),
    ("review-requested", "blue"),
    ("curate-retire", "cyan"),
    ("curate-docs", "cyan"),
    ("loop-guard", "red"),
    ("deploy", "yellow"),
];

/// A store as every migration before `models-changed` leaves it, with one open row of every kind, a closed row and a dismissed
/// one. The test fails here while the migration is missing.
fn before_models_changed() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-models-changed"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "models-changed is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    let rows: Vec<String> = KINDS
        .iter()
        .enumerate()
        .map(|(i, (kind, urgency))| {
            format!("('{kind}', '{urgency}', 'subject:{kind}', 'label of {kind}', 't{i}')")
        })
        .collect();
    write(
        &mut store,
        &format!(
            "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES {};
             INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at, dismissed_at, snoozed_until, url) VALUES
               ('review-requested', 'blue', 'pr:acme/pay#0', 'review pay#0', 't0', 't1', 't0b', 't0c', 'https://x.io/r'),
               ('curate-docs', 'cyan', 'subject:curate-docs', 'closed twin', 't0', 't9', NULL, NULL, NULL);",
            rows.join(", ")
        ),
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
            let columns: Vec<_> = table_columns(c, "inbox_items")?
                .into_iter()
                .filter(|(n, _)| n != "head_oid" && n != "reviewed_oid") // `codereview-followup` (a later migration)
                .collect();
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

fn columns(store: &mut Store) -> Vec<String> {
    store
        .read(|c: &Connection| {
            Ok(table_columns(c, "inbox_items")?
                .into_iter()
                .map(|(name, _)| name)
                .filter(|n| n != "head_oid" && n != "reviewed_oid") // `codereview-followup` (a later migration)
                .collect())
        })
        .unwrap()
}

/// A store with `models-changed` applied over the rows of [`before_models_changed`].
fn migrated() -> (tempfile::TempDir, CpdHome, Store) {
    let (dir, home, _earlier) = before_models_changed();
    let store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    (dir, home, store)
}

const INSERT: &str =
    "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at)";

#[test]
fn models_changed_migration_keeps_every_row_of_every_existing_kind_column_for_column() {
    let (_dir, home, earlier) = before_models_changed();
    let mut before_store = crate::store::tests::open_named(&home, &earlier).unwrap();
    let before = items(&mut before_store);
    let before_columns = columns(&mut before_store);
    drop(before_store);
    assert_eq!(
        before.len(),
        KINDS.len() + 2,
        "the fixture: a row of every kind, a closed and a dismissed one"
    );

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        items(&mut store),
        before,
        "every row, every column, every id"
    );
    assert_eq!(
        columns(&mut store),
        before_columns,
        "no column is lost: the rebuild starts from the newest shape"
    );
}

#[test]
fn models_changed_migration_is_declared_breaking() {
    let named = NAMED
        .iter()
        .find(|n| n.name.ends_with("-models-changed"))
        .expect("a migration named models-changed");
    assert_eq!(
        named.compat,
        Compat::Breaking,
        "it rebuilds inbox_items to widen a CHECK"
    );
}

#[test]
fn models_changed_migration_accepts_the_kind_and_still_refuses_an_unknown_kind() {
    let (_dir, _home, mut store) = migrated();
    write(&mut store, &format!("{INSERT} VALUES ('models-changed', 'cyan', 'models:bifrost/2026-10-26T12:00:00Z', 'bifrost: 2 new', 't', NULL)")).unwrap();
    let refusal = write(
        &mut store,
        &format!("{INSERT} VALUES ('models-gone', 'cyan', 'models:x/1', 'l', 't', NULL)"),
    );
    assert!(
        format!("{refusal:?}").contains("CHECK constraint failed"),
        "{refusal:?}"
    );
}

#[test]
fn models_changed_migration_keeps_the_other_checks_the_blue_tier_and_the_cyan_tier() {
    let (_dir, _home, mut store) = migrated();
    write(
        &mut store,
        &format!("{INSERT} VALUES ('review-requested', 'blue', 'pr:x/y#1', 'l', 't', NULL)"),
    )
    .unwrap();
    write(
        &mut store,
        &format!(
            "{INSERT} VALUES ('models-changed', 'cyan', 'models:github-copilot/1', 'l', 't', NULL)"
        ),
    )
    .unwrap();
    let refusal = write(
        &mut store,
        &format!(
            "{INSERT} VALUES ('models-changed', 'purple', 'models:github-copilot/2', 'l', 't', NULL)"
        ),
    );
    assert!(
        format!("{refusal:?}").contains("CHECK constraint failed"),
        "{refusal:?}"
    );
}

#[test]
fn models_changed_migration_keeps_the_open_item_identity_index() {
    let (_dir, _home, mut store) = migrated();
    let insert = |closed: &str| {
        format!(
            "{INSERT} VALUES ('models-changed', 'cyan', 'models:bifrost/1', 'l', 't', {closed})"
        )
    };
    write(&mut store, &insert("NULL")).unwrap();
    assert!(
        write(&mut store, &insert("NULL")).is_err(),
        "a second open row of the same (kind, subject, urgency) is refused"
    );
    write(
        &mut store,
        "UPDATE inbox_items SET closed_at = 't2' WHERE kind = 'models-changed'",
    )
    .unwrap();
    write(&mut store, &insert("NULL")).unwrap();
    let indexes: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name IN ('inbox_items_open', 'inbox_items_closed')", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(indexes, 2, "both indexes are recreated");
}

#[test]
fn models_changed_migration_keeps_the_no_delete_trigger() {
    let (_dir, _home, mut store) = migrated();
    let refusal = write(&mut store, "DELETE FROM inbox_items");
    assert!(refusal.is_err(), "an item is closed, never deleted");
}

#[test]
fn opening_the_migrated_store_again_changes_nothing() {
    let (_dir, home, mut store) = migrated();
    write(
        &mut store,
        &format!("{INSERT} VALUES ('models-changed', 'cyan', 'models:bifrost/1', 'l', 't', NULL)"),
    )
    .unwrap();
    let once = items(&mut store);
    let applied = |s: &mut Store| -> i64 {
        s.read(|c| Ok(c.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))?))
            .unwrap()
    };
    let migrations = applied(&mut store);
    drop(store);

    let mut again = Store::open(&home, 5000).unwrap();
    assert!(
        again.schema_mismatch().is_none(),
        "{:?}",
        again.schema_mismatch()
    );
    assert_eq!(items(&mut again), once);
    assert_eq!(
        applied(&mut again),
        migrations,
        "no migration is applied twice"
    );
}
