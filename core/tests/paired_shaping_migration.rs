//! The paired-shaping migration preserves the prior database and its guards.

use cpd_core::home::CpdHome;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::{Connection, types::Value};

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let columns = stmt.column_count();
    stmt.query_map([], |r| (0..columns).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn paired_shaping_migration_preserves_rows_history_and_constraints() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let migrations = named_migrations();
    let index = migrations
        .iter()
        .position(|(name, _, _)| *name == "20261114T1101-paired-shaping")
        .unwrap();
    let conn = prefix_database(index, &home).unwrap();
    conn.execute_batch(
        "INSERT INTO txns (id, at, command, pid, cwd) VALUES (1, 't', 'fixture', 1, '/');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, done_at)
           VALUES ('legacy', 'ctx', 'Legacy', 'done', NULL, 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('oldshape', 'ctx', 'Old shaping', 'shaping', NULL, 't');
         INSERT INTO jobs (plan, workflow, key, title, status, appended, created_txn, created_at, kind)
           VALUES ('legacy', NULL, 'scout', 'Scout', 'draft', 1, 1, 't', 'scout');
         INSERT INTO events (txn, at, kind, context, plan, text, data)
           VALUES (1, 't', 'plan-created', 'ctx', 'legacy', 'Historical', '{}');
         INSERT INTO context_records (context, id, kind, at, machine, by_plan, text)
           VALUES ('ctx', 'm1-n1', 'note', 't', 'm1', 'legacy', 'Historical');",
    )
    .unwrap();
    let plans = rows(&conn, "SELECT * FROM plans ORDER BY id");
    let jobs = rows(&conn, "SELECT * FROM jobs ORDER BY seq");
    let events = rows(&conn, "SELECT * FROM events ORDER BY id");
    let records = rows(&conn, "SELECT * FROM context_records ORDER BY seq");
    let history = rows(&conn, "SELECT * FROM history ORDER BY id");
    let plan_objects = rows(&conn, "SELECT type, name, sql FROM sqlite_schema WHERE tbl_name = 'plans' AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY type, name");

    // The runner applies the SQL with these settings, then reconciles generated triggers.
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
    conn.execute_batch(migrations[index].2).unwrap();
    assert_eq!(rows(&conn, "SELECT * FROM plans ORDER BY id"), plans);
    assert_eq!(rows(&conn, "SELECT * FROM jobs ORDER BY seq"), jobs);
    assert_eq!(rows(&conn, "SELECT * FROM events ORDER BY id"), events);
    assert_eq!(rows(&conn, "SELECT * FROM context_records ORDER BY seq"), records);
    assert_eq!(rows(&conn, "SELECT * FROM history ORDER BY id"), history);
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());

    // The reference database after the migration uses the same runner reconciliation.
    let after = prefix_database(index + 1, &home).unwrap();
    assert_eq!(
        rows(&after, "SELECT type, name, sql FROM sqlite_schema WHERE tbl_name = 'plans' AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY type, name"),
        plan_objects,
    );
    after.execute_batch(
        "INSERT INTO txns (id, at, command, pid, cwd) VALUES (1, 't', 'fixture', 1, '/');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead', 'lead', 'ctx', 'pair', 'lead ctx:pair', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pair', 'ctx', 'Paired', 'shaping', 'lead', 't');",
    ).unwrap();
    assert_eq!(rows(&after, "SELECT id FROM plans WHERE id = 'pair' AND phase = 'shaping' AND lead_session = 'lead'").len(), 1);
    assert_eq!(rows(&after, "SELECT id FROM history WHERE tbl = 'plans' AND op = 'insert'").len(), 1);
    assert!(after.execute_batch("UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pair'").is_err());
    assert!(after.execute_batch("DELETE FROM plans WHERE id = 'pair'").is_err());
    assert!(after.execute_batch("INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('badlock', 'ctx', 'Bad', 'shaping', 'lead', 't', 't')").is_err());
    assert!(after.execute_batch("INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('badlead', 'ctx', 'Bad', 'grounding', NULL, 't')").is_err());
    assert!(rows(&after, "PRAGMA foreign_key_check").is_empty());
}
