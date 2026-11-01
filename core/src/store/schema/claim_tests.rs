//! The migration `inbox-claim` (pro-d58, C2): `inbox_items.claimed_at` and the Event kind `inbox-claimed`.

use serde_json::Value;

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

/// A store as every migration before `inbox-claim` leaves it.
fn before_claim() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-inbox-claim"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "inbox-claim is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut store,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES
           ('review-requested', 'blue', 'pr:acme/pay#1', 'review pay#1', 't1'),
           ('dialog', 'red', 'session:a', 'a', 't3');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at, dismissed_at) VALUES
           ('review-requested', 'blue', 'pr:acme/pay#2', 'review pay#2', 't0', 't1', 't0b');
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

/// Every row of `inbox_items`, the given columns, by id.
fn items(store: &mut Store, columns: &[&str]) -> Vec<Value> {
    let pairs: Vec<String> = columns
        .iter()
        .map(|name| format!("'{name}', {name}"))
        .collect();
    store
        .read(|c| {
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

fn kinds(store: &mut Store) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT name FROM event_kinds WHERE name = 'inbox-claimed'")?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

const OLD: [&str; 14] = [
    "id",
    "kind",
    "urgency",
    "subject",
    "label",
    "url",
    "context",
    "plan",
    "job",
    "session",
    "record",
    "event",
    "opened_at",
    "closed_at",
];

#[test]
fn inbox_claim_migration_adds_a_nullable_claimed_at_and_keeps_every_row() {
    let (_dir, home, earlier) = before_claim();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    let mut columns = OLD.to_vec();
    columns.extend(["snoozed_until", "dismissed_at"]);
    let before = items(&mut old, &columns);
    drop(old);
    assert_eq!(before.len(), 4);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        items(&mut store, &columns),
        before,
        "every row is kept, column for column"
    );
    columns.push("claimed_at");
    let after = items(&mut store, &columns);
    assert_eq!(after.len(), 4);
    assert!(after.iter().all(|r| r["claimed_at"].is_null()), "{after:?}");
}

#[test]
fn inbox_claim_migration_lets_only_a_review_requested_item_be_claimed() {
    let (_dir, home, _earlier) = before_claim();
    let mut store = Store::open(&home, 5000).unwrap();
    write(
        &mut store,
        "UPDATE inbox_items SET claimed_at = 't9' WHERE subject = 'pr:acme/pay#1'",
    )
    .unwrap();
    let rows = items(&mut store, &["subject", "claimed_at"]);
    assert_eq!(rows[0]["claimed_at"], "t9", "{rows:?}");

    let update = write(
        &mut store,
        "UPDATE inbox_items SET claimed_at = 't9' WHERE kind = 'dialog'",
    );
    assert!(
        format!("{update:?}").contains("CHECK constraint failed"),
        "{update:?}"
    );
    let insert = write(
        &mut store,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, claimed_at) VALUES ('dialog', 'red', 'session:z', 'z', 't', 't9')",
    );
    assert!(
        format!("{insert:?}").contains("CHECK constraint failed"),
        "{insert:?}"
    );
}

#[test]
fn inbox_claim_migration_adds_the_inbox_claimed_event_kind() {
    let (_dir, home, earlier) = before_claim();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    assert!(kinds(&mut old).is_empty(), "not before the migration");
    drop(old);
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(kinds(&mut store), ["inbox-claimed"]);
}
