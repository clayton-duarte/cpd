//! The schema of the Lead-less shaping Plan (air-d19, air-d22; contract section 2.2 Plan,
//! section 3.1, section 4.4 and the section 4.5 migration `<YYYYMMDDTHHMM>-shaping`):
//! plans.lead_session nullable, the phase shaping, the lead_session/phase CHECKs and the
//! two transitions. Red until that migration lands: the CHECK refuses the row, and there
//! is no such migration.

use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::Connection;
use rusqlite::types::Value;

fn home(dir: &tempfile::TempDir) -> CpdHome {
    CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
}

/// A store opened through the repo's usual fixture: a temp CPD_HOME migrated to the
/// latest schema of this binary, as every cpd process opens it.
fn store(dir: &tempfile::TempDir) -> Store {
    let store = Store::open(&home(dir), 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    store
}

/// Runs sql in its own write transaction.
fn write(store: &mut Store, sql: &str) -> Result<(), cpd_core::error::CpdError> {
    store.write("test", |tx| Ok(tx.execute_batch(sql)?))
}

/// The Context every fixture Plan belongs to.
const CONTEXT: &str =
    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');";

/// The rows sql returns, for a whole-table comparison.
fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let columns = stmt.column_count();
    stmt.query_map([], |r| {
        (0..columns)
            .map(|i| r.get::<_, Value>(i))
            .collect::<rusqlite::Result<Vec<_>>>()
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn shaping_schema_lead_session_is_nullable() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let not_null: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT \"notnull\" FROM pragma_table_info('plans') WHERE name = 'lead_session'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(
        not_null, 0,
        "plans.lead_session must be nullable for a Lead-less shaping Plan"
    );
}

#[test]
fn shaping_schema_accepts_shaping_phase_with_null_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    write(
        &mut store,
        &format!(
            "{CONTEXT}
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('shape1', 'ctx', 'Shape', 'shaping', NULL, 't');"
        ),
    )
    .expect("a shaping Plan with a null lead_session is accepted");
    let (phase, lead): (String, Option<String>) = store
        .read(|conn| {
            conn.query_row(
                "SELECT phase, lead_session FROM plans WHERE id = 'shape1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!((phase.as_str(), lead.as_deref()), ("shaping", None));
}

#[test]
fn shaping_schema_refuses_shaping_with_a_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    // The accepted case first: a shaping Plan is Lead-less. This is also the red step
    // until the migration lands.
    write(
        &mut store,
        &format!(
            "{CONTEXT}
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', NULL, 'lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('shape1', 'ctx', 'Shape', 'shaping', NULL, 't');"
        ),
    )
    .expect("a shaping Plan with a null lead_session is accepted");
    assert!(
        write(
            &mut store,
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('shape2', 'ctx', 'Shape 2', 'shaping', 'lead1', 't');"
        )
        .is_err(),
        "a shaping Plan with a Lead violates phase <> 'shaping' OR lead_session IS NULL"
    );
    assert!(
        write(
            &mut store,
            "UPDATE plans SET lead_session = 'lead1' WHERE id = 'shape1'"
        )
        .is_err(),
        "a shaping Plan may not gain a Lead"
    );
}

#[test]
fn shaping_schema_refuses_null_lead_in_other_phases() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    // The accepted cases first: a Lead-less Plan is legal in shaping, and in done after it
    // settled. This is also the red step until the migration lands.
    write(
        &mut store,
        &format!(
            "{CONTEXT}
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, done_at)
               VALUES ('shape1', 'ctx', 'Shape', 'shaping', NULL, 't', NULL),
                      ('shaped', 'ctx', 'Shaped', 'done', NULL, 't', 't');"
        ),
    )
    .expect("a Lead-less Plan is accepted in shaping and in done");
    // grounding, plan, dispatch and deploy may never be Lead-less.
    for (id, phase, locked) in [
        ("g", "grounding", "NULL"),
        ("p", "plan", "NULL"),
        ("d", "dispatch", "'t'"),
        ("e", "deploy", "'t'"),
    ] {
        assert!(
            write(
                &mut store,
                &format!(
                    "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                       VALUES ('{id}', 'ctx', 'Title {id}', '{phase}', NULL, 't', {locked});"
                )
            )
            .is_err(),
            "a null lead_session in phase {phase} is refused"
        );
    }
}

#[test]
fn shaping_schema_allows_shaping_done_transitions() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    write(
        &mut store,
        &format!(
            "{CONTEXT}
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('shape1', 'ctx', 'Shape', 'shaping', NULL, 't');"
        ),
    )
    .expect("a shaping Plan with a null lead_session is accepted");
    // shaping -> done through the plans_phase trigger, the Lead-less Plan never locked.
    write(
        &mut store,
        "UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'shape1'",
    )
    .expect("shaping>done is legal for a Lead-less Plan");
    let (phase, locked): (String, Option<String>) = store
        .read(|conn| {
            conn.query_row(
                "SELECT phase, locked_at FROM plans WHERE id = 'shape1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!((phase.as_str(), locked), ("done", None));
    // done -> shaping, the new explore Job reopen, clears done_at.
    write(
        &mut store,
        "UPDATE plans SET phase = 'shaping', done_at = NULL WHERE id = 'shape1'",
    )
    .expect("done>shaping is legal for a Lead-less Plan");
    // A Lead-less Plan may not go to grounding.
    let err = write(
        &mut store,
        "UPDATE plans SET phase = 'grounding' WHERE id = 'shape1'",
    )
    .expect_err("shaping>grounding is refused");
    assert_eq!(err.code(), ErrorCode::IllegalTransition, "{err:?}");
}

/// A store before the migration: one Plan with its Jobs, Attempts, Dependencies and Escalations.
const BEFORE: &str = "BEGIN;
INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, 't', 'migrate', 1, NULL, '/');
INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
  VALUES ('lead1', 'lead', 'ctx', 'pay', 'lead', '/', 'O3', 'idle', 't', 't');
INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
  VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                  kind, status, appended, fingerprint, created_txn, created_at)
  VALUES ('pay', NULL, 'a', 'A', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'scout', 'queued', 1, '0000000000000000000000000000000000000000000000000000000000000000', 1, 't'),
         ('pay', NULL, 'b', 'B', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'scout', 'queued', 1, '0000000000000000000000000000000000000000000000000000000000000000', 1, 't');
INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'a', 'b');
INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
  VALUES ('pay', 'a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a1', '/w', 't');
INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
  VALUES ('pay', 'a1-q1', 'a', 1, 'What?', 'open', 1, 't');
COMMIT;";

#[test]
fn shaping_schema_keeps_existing_plans_on_migration() {
    let list = named_migrations();
    let index = list
        .iter()
        .position(|(name, ..)| name.ends_with("-shaping"))
        .expect("the shaping migration exists, named <YYYYMMDDTHHMM>-shaping");
    assert_eq!(list[index].1, "breaking", "{:?}", list[index].0);
    let dir = tempfile::tempdir().unwrap();
    let conn = prefix_database(index, &home(&dir)).unwrap();
    conn.execute_batch(BEFORE).unwrap();
    let select = "SELECT id, context, title, phase, lead_session, created_at, locked_at, done_at, archived_at FROM plans ORDER BY id";
    let before = rows(&conn, select);
    assert_eq!(before.len(), 1);
    conn.execute_batch(list[index].2).unwrap();
    assert_eq!(
        rows(&conn, select),
        before,
        "every Plan row and column is kept, id included"
    );
    for (table, expected) in [
        ("jobs", 2),
        ("attempts", 1),
        ("dependencies", 1),
        ("escalations", 1),
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, expected, "every {table} row is kept");
    }
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        violations, 0,
        "the rebuilt plans table keeps every foreign key"
    );
}
