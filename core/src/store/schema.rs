//! The schema: migrations, the generated history triggers and the schema hash
//! (contract §4.3–§4.5).

use rusqlite::{Connection, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// A migration: applied once, in order, inside the migrating transaction.
pub(super) type Migration = fn(&Connection) -> Result<()>;

/// Migrations `1..=N`, in order; `MIGRATIONS[i]` takes the schema to version `i + 1`.
pub(super) const MIGRATIONS: &[Migration] = &[migration_1, migration_2, migration_3];

/// The schema hash of each version (§4.3): `SCHEMA_HASHES[i]` is version `i + 1`'s.
/// A test asserts that migrating an empty database gives each of them.
pub(super) const SCHEMA_HASHES: &[&str] = &[
    "0c73adb58e9a2027348333015d1aaf952d3bc3b0923ea27bcd41cac1079dcced",
    "37449f2445f3d621405857abab17a5f3760382270a1b6a55f46a116792998646",
    "c4529a6f3cd787333b8115bade42de35504eb4902773945cfbde80ba93acfc5f",
];

const _: () = assert!(SCHEMA_HASHES.len() == MIGRATIONS.len(), "one schema hash per migration");

/// The schema version this binary writes: `N`.
pub const VERSION: i64 = MIGRATIONS.len() as i64;

/// The tables whose rows are copied into `history` by generated triggers (§4.4).
/// `dependencies` gets no generated `_no_delete`: migration 1 writes its own
/// (a draft Job's Dependency may be deleted) and its `_hist_del`.
pub const HISTORY_TABLES: &[&str] = &[
    "contexts",
    "context_records",
    "plans",
    "plan_reviews",
    "jobs",
    "dependencies",
    "attempts",
    "escalations",
    "sessions",
    "tier_catalog",
];

fn migration_1(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0001.sql"))?;
    for table in HISTORY_TABLES {
        create_history_triggers(conn, table, *table != "dependencies")?;
    }
    Ok(())
}

/// Cockpit parity (§4.5): `plans` gains `pr_title`, `pr_review_decision` and
/// `pr_checks`, so its history triggers are dropped and generated again to list them.
fn migration_2(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0002.sql"))?;
    conn.execute_batch("DROP TRIGGER plans_hist_ins; DROP TRIGGER plans_hist_upd; DROP TRIGGER plans_no_delete;")?;
    create_history_triggers(conn, "plans", true)
}

/// Archive and rename (§4.5): `archived_at` on `contexts`, `plans` and `sessions`
/// and `events` rebuilt with five more kinds, so the three tables' history
/// triggers are dropped and generated again to list the new column.
fn migration_3(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0003.sql"))?;
    for table in ["contexts", "plans", "sessions"] {
        conn.execute_batch(&format!(
            "DROP TRIGGER {table}_hist_ins; DROP TRIGGER {table}_hist_upd; DROP TRIGGER {table}_no_delete;"
        ))?;
        create_history_triggers(conn, table, true)?;
    }
    Ok(())
}

/// Creates `<table>_hist_ins` and `<table>_hist_upd`, and `<table>_no_delete`
/// when `no_delete`, from the table's current columns and primary key, in the
/// shape of §4.4's `escalations` example. A later migration that adds or
/// renames columns drops these and calls this again (§4.5).
fn create_history_triggers(conn: &Connection, table: &str, no_delete: bool) -> Result<()> {
    conn.execute_batch(&history_triggers(conn, table, no_delete)?)
}

/// The SQL of [`create_history_triggers`].
fn history_triggers(conn: &Connection, table: &str, no_delete: bool) -> Result<String> {
    let mut stmt = conn.prepare("SELECT name, pk FROM pragma_table_info(?1) ORDER BY cid")?;
    let columns: Vec<(String, i64)> =
        stmt.query_map([table], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_>>()?;
    if columns.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(format!("no table {table}")));
    }
    let mut key: Vec<&(String, i64)> = columns.iter().filter(|(_, n)| *n > 0).collect();
    key.sort_by_key(|(_, n)| *n);
    let pk = |row: &str| -> String {
        let cols: Vec<String> = key.iter().map(|(name, _)| format!("{row}.{name}")).collect();
        format!("json_array({})", cols.join(", "))
    };
    let object = |row: &str| -> String {
        let pairs: Vec<String> = columns.iter().map(|(name, _)| format!("'{name}', {row}.{name}")).collect();
        format!("json_object({})", pairs.join(", "))
    };
    let mut sql = format!(
        "CREATE TRIGGER {table}_hist_ins AFTER INSERT ON {table} BEGIN\n  \
         INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), '{table}',\n    \
         {}, 'insert', NULL,\n    {});\nEND;\n\
         CREATE TRIGGER {table}_hist_upd AFTER UPDATE ON {table} BEGIN\n  \
         INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), '{table}',\n    \
         {}, 'update',\n    {},\n    {});\nEND;\n",
        pk("NEW"),
        object("NEW"),
        pk("NEW"),
        object("OLD"),
        object("NEW"),
    );
    if no_delete {
        sql.push_str(&format!(
            "CREATE TRIGGER {table}_no_delete BEFORE DELETE ON {table} BEGIN\n  \
             SELECT RAISE(ABORT, 'append_only: {table}');\nEND;\n"
        ));
    }
    Ok(sql)
}

/// The schema hash (§4.3): sha256 (hex) of the JSON array of
/// `[type, name, sql]` rows of `sqlite_schema`, in the contract's order.
pub fn schema_hash(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT type, name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(Value::from(vec![
                Value::from(row.get::<_, String>(0)?),
                Value::from(row.get::<_, String>(1)?),
                row.get::<_, Option<String>>(2)?.map_or(Value::Null, Value::from),
            ]))
        })?
        .collect::<Result<_>>()?;
    let text = serde_json::to_string(&rows).expect("JSON values always serialise");
    Ok(format!("{:x}", Sha256::digest(text.as_bytes())))
}

#[cfg(test)]
mod tests {
    use rusqlite::functions::FunctionFlags;

    use super::*;
    use crate::error::CpdError;
    use crate::home::CpdHome;
    use crate::store::{APPLICATION_ID, Store};

    /// A connection with a stand-in `cpd_txn()`, which the history triggers call.
    fn connection(path: &std::path::Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64)).unwrap();
        conn
    }

    /// The schema after the first `n` migrations only.
    fn at_version(conn: &Connection, n: usize) {
        for migration in &MIGRATIONS[..n] {
            migration(conn).unwrap();
        }
        conn.pragma_update(None, "user_version", n as i64).unwrap();
    }

    #[test]
    fn migrating_an_empty_database_gives_every_versions_schema_hash() {
        let dir = tempfile::tempdir().unwrap();
        for (n, expected) in SCHEMA_HASHES.iter().enumerate() {
            let conn = connection(&dir.path().join(format!("v{}.db", n + 1)));
            at_version(&conn, n + 1);
            assert_eq!(schema_hash(&conn).unwrap(), *expected, "version {}", n + 1);
        }
    }

    const PLAN: &str = "INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                                           pr_number, pr_url, pr_opened_at, created_at, locked_at)
         VALUES ('pay', 'ctx', 'Pay', '/repo', 'main', 'cpd/pay', '/wt', 'pr', 'l-pay', 7, 'https://example.test/pull/7',
                 '2026-01-05T00:00:00.000Z', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');";

    /// A version 1 database holding a Plan with a PR, as a real `cpd.db` does.
    fn version_1(home: &CpdHome) {
        std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
        let conn = connection(&home.db());
        conn.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
        at_version(&conn, 1);
        conn.execute_batch(
            "BEGIN;
             INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '2026-01-01T00:00:00.000Z', 'migrate', 1, NULL, '/');
             INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/repo', 'O3', 'idle', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');",
        )
        .unwrap();
        conn.execute_batch(PLAN).unwrap();
        conn.execute_batch("COMMIT").unwrap();
    }

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
    }

    #[test]
    fn a_version_1_database_migrates_to_the_current_version_and_keeps_its_plan() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_1(&home);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        let (version, row, migrations): (i64, (i64, Option<String>, Option<String>, Option<String>), i64) = store
            .read(|c| {
                Ok((
                    c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                    c.query_row("SELECT pr_number, pr_title, pr_review_decision, pr_checks FROM plans WHERE id = 'pay'", [], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                    })?,
                    c.query_row("SELECT count(*) FROM txns WHERE command = 'migrate'", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(version, VERSION);
        assert_eq!(row, (7, None, None, None), "a Plan's PR keeps the three columns null until its next poll");
        assert_eq!(migrations, 2, "the old migration's row and the new one");
    }

    #[test]
    fn after_migration_2_plans_history_lists_the_new_columns_and_the_checks_hold() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_1(&home);
        let mut store = Store::open(&home, 5000).unwrap();

        store
            .write("test", |tx| {
                tx.execute(
                    "UPDATE plans SET pr_title = 'T', pr_review_decision = 'approved', pr_checks = 'passed' WHERE id = 'pay'",
                    [],
                )?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
        let after: String = store
            .read(|c| Ok(c.query_row("SELECT after FROM history WHERE tbl = 'plans' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))?))
            .unwrap();
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["pr_title"], "T");
        assert_eq!(after["pr_review_decision"], "approved");
        assert_eq!(after["pr_checks"], "passed");

        for bad in ["pr_title = ''", "pr_review_decision = 'maybe'", "pr_checks = 'green'"] {
            let sql = format!("UPDATE plans SET {bad} WHERE id = 'pay'");
            let err = store.write("test", |tx| Ok::<(), CpdError>({ tx.execute(&sql, [])?; })).unwrap_err();
            assert!(err.to_string().contains("CHECK") || format!("{err:?}").contains("CHECK"), "{bad}: {err:?}");
        }
    }

    /// A version 2 database with Events, as a real `cpd.db` is; the Event ids are 1, 2 and 5
    /// (not contiguous), so the next one must be above 5.
    fn version_2(home: &CpdHome) {
        std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
        let conn = connection(&home.db());
        conn.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
        at_version(&conn, 2);
        conn.execute_batch(
            "BEGIN;
             INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '2026-01-01T00:00:00.000Z', 'migrate', 1, NULL, '/');
             INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/repo', 'O3', 'idle', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');",
        )
        .unwrap();
        conn.execute_batch(PLAN).unwrap();
        for id in [1, 2, 5] {
            conn.execute(
                "INSERT INTO events (id, txn, at, kind, context, plan, text, data)
                 VALUES (?1, 1, '2026-01-01T00:00:00.000Z', 'plan-created', 'ctx', 'pay', ?2, '{}')",
                rusqlite::params![id, format!("event {id}")],
            )
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
    }

    const NEW_KINDS: [&str; 5] =
        ["context-renamed", "plan-renamed", "context-archived", "plan-archived", "session-archived"];

    #[test]
    fn a_version_2_database_migrates_to_3_keeping_every_row_and_event_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_2(&home);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        let (version, events, archived, triggers): (i64, Vec<(i64, String)>, i64, i64) = store
            .read(|c| {
                let mut stmt = c.prepare("SELECT id, text FROM events ORDER BY id")?;
                let events = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>>>()?;
                Ok((
                    c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                    events,
                    c.query_row(
                        "SELECT (SELECT count(*) FROM contexts WHERE id = 'ctx' AND archived_at IS NULL)
                              + (SELECT count(*) FROM plans WHERE id = 'pay' AND archived_at IS NULL)
                              + (SELECT count(*) FROM sessions WHERE id = 'l-pay' AND archived_at IS NULL)",
                        [],
                        |r| r.get(0),
                    )?,
                    c.query_row(
                        "SELECT count(*) FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'events'",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(version, 3);
        assert_eq!(events, [(1, "event 1".to_owned()), (2, "event 2".to_owned()), (5, "event 5".to_owned())]);
        assert_eq!(archived, 3, "every row kept, every archived_at null");
        assert_eq!(triggers, 3, "events_own, events_no_update, events_no_delete");

        let id = store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO events (txn, at, kind, context, text, data)
                     VALUES (cpd_txn(), '2026-01-02T00:00:00.000Z', 'context-archived', 'ctx', 't', '{\"archived\":true}')",
                    [],
                )?;
                Ok::<i64, CpdError>(tx.last_insert_rowid())
            })
            .unwrap();
        assert!(id > 5, "the next Event id {id} is above the old maximum");
    }

    #[test]
    fn after_migration_3_the_sequence_never_goes_back_and_the_history_lists_archived_at() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_2(&home);
        // An old sequence above the highest id (rows that no longer exist) is kept.
        Connection::open(home.db()).unwrap().execute("UPDATE sqlite_sequence SET seq = 40 WHERE name = 'events'", []).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let (seq, after): (i64, String) = store
            .write("test", |tx| {
                tx.execute("UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'pay'", [])?;
                Ok::<_, CpdError>((
                    tx.query_row("SELECT seq FROM sqlite_sequence WHERE name = 'events'", [], |r| r.get(0))?,
                    tx.query_row("SELECT after FROM history WHERE tbl = 'plans' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(seq, 40);
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["archived_at"], "2026-02-01T00:00:00.000Z");
    }

    #[test]
    fn the_events_check_allows_the_five_new_kinds_and_still_refuses_an_unknown_one() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_2(&home);
        let mut store = Store::open(&home, 5000).unwrap();
        let insert = |tx: &Connection, kind: &str| {
            tx.execute(
                "INSERT INTO events (txn, at, kind, context, text, data)
                 VALUES (cpd_txn(), '2026-01-02T00:00:00.000Z', ?1, 'ctx', 't', '{}')",
                [kind],
            )
        };
        for kind in NEW_KINDS {
            store.write("test", |tx| Ok::<_, CpdError>(insert(tx, kind)?)).unwrap_or_else(|e| panic!("{kind}: {e:?}"));
        }
        let err = store.write("test", |tx| Ok::<_, CpdError>(insert(tx, "plan-exploded")?)).unwrap_err();
        assert!(format!("{err:?}").contains("CHECK"), "{err:?}");
    }
}
