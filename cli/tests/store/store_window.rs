//! The compatibility window (contract §4.5 "The window and its one verdict"):
//! a binary opens a store that applied additive migrations it does not know and
//! refuses one whose newest breaking migration it lacks. The migrations this
//! binary lacks are applied by hand, as a newer binary would have.

use crate::common;

use std::fs;

use crate::home::{HasCpdHome, Home};
use common::{TestEnv, newer_migration};
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, verdict};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;

const NEWER: &str = "29991231T2358-newer-table";
const NEWEST: &str = "29991231T2359-newest-column";

/// A machine with a config file, which the commands need.
fn machine() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\n",
    )
    .unwrap();
    env
}

fn home(env: &impl HasCpdHome) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn apply_newer(env: &impl HasCpdHome, name: &str, compat: &str, sql: &str) {
    newer_migration::apply_newer(&env.cpd_home(), name, compat, sql);
}

/// A store with a new table and a new column of an additive migration this binary lacks.
fn store_with_additive_migrations(env: &impl HasCpdHome) {
    drop(Store::open(&home(env), 5000).unwrap());
    apply_newer(
        env,
        NEWER,
        "additive",
        "CREATE TABLE newer_things (id INTEGER PRIMARY KEY, label TEXT NOT NULL) STRICT; CREATE INDEX newer_things_label ON newer_things (label);",
    );
    apply_newer(
        env,
        NEWEST,
        "additive",
        "ALTER TABLE events ADD COLUMN newest TEXT;",
    );
}

fn read_only_verdict(env: &impl HasCpdHome) -> cpd_core::store::Verdict<'static> {
    let conn =
        Connection::open_with_flags(home(env).db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    verdict(&conn).unwrap()
}

#[test]
fn a_binary_opens_a_store_with_additive_migrations_it_does_not_know() {
    let env = Home::new();
    store_with_additive_migrations(&env);

    let v = read_only_verdict(&env);
    assert!(v.opens(), "{v:?}");
    assert_eq!(v.unknown, [NEWER, NEWEST]);
    assert!(v.pending_names().is_empty());

    let store = Store::open(&home(&env), 5000).unwrap();
    assert_eq!(
        store.schema_mismatch(),
        None,
        "the schema check covers only what this binary created"
    );
}

#[test]
fn it_reads_and_writes_such_a_store() {
    let env = machine();
    store_with_additive_migrations(&env);
    let mut store = Store::open(&home(&env), 5000).unwrap();
    store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let titles: Vec<String> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT title FROM contexts")?;
            let rows = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(titles, ["Ctx"]);

    // And through the binary itself, which appends Events (the table with the unknown column).
    let run = env.cpd(&[
        "context", "new", "ctx2", "--title", "Two", "--owner", "acme",
    ]);
    assert_eq!(run.exit, 0, "{run:?}");
}

#[test]
fn a_store_whose_newest_breaking_migration_this_binary_lacks_is_store_too_new() {
    let env = machine();
    store_with_additive_migrations(&env);
    apply_newer(
        &env,
        "29991231T2400-rebuild",
        "breaking",
        "CREATE TABLE rebuilt (id INTEGER PRIMARY KEY) STRICT;",
    );

    let v = read_only_verdict(&env);
    assert!(!v.opens());
    let err = v.refusal().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew);
    assert_eq!(err.details()["floor"], "29991231T2400-rebuild");
    assert_eq!(
        err.details()["unknown"],
        json!([NEWER, NEWEST, "29991231T2400-rebuild"])
    );
    assert_eq!(
        v.unknown,
        [NEWER, NEWEST, "29991231T2400-rebuild"],
        "filled whether or not it opens"
    );

    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
    assert_eq!(err.exit().code(), 1);

    let run = env.cpd(&["context", "new", "ctx", "--title", "Ctx", "--owner", "acme"]);
    assert_eq!(run.error()["code"], "store_too_new", "{run:?}");
    assert_eq!(run.exit, 1);
}

#[test]
fn inside_the_window_a_dropped_object_of_this_binary_is_still_store_tampered() {
    let env = Home::new();
    store_with_additive_migrations(&env);
    let conn = Connection::open(home(&env).db()).unwrap();
    let index: String = conn
        .query_row("SELECT name FROM sqlite_schema WHERE type = 'index' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'newer_%' ORDER BY name LIMIT 1", [], |r| r.get(0))
        .unwrap();
    conn.execute_batch(&format!("DROP INDEX {index}")).unwrap();
    drop(conn);
    let mut store = Store::open(&home(&env), 5000).unwrap();
    assert_eq!(
        store.schema_mismatch().and_then(|m| m.object.clone()),
        Some(index)
    );
    assert_eq!(
        store.write("test", |_| Ok(())).unwrap_err().code(),
        ErrorCode::StoreTampered
    );
}
