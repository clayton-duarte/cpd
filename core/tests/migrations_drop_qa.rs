//! The DELETE-only migration `drop-question-answer` (pro-d8, A2), pinned from outside.
//!
//! Each test builds a fully migrated store, rewinds it to just before the migration (the
//! migration's `schema_migrations` row is removed through a raw connection), inserts a Context with
//! records, then lets `Store::open` run the migration as the runner does: with the reconcile, the
//! foreign-key check and the schema check.

use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, params};

const MIGRATION: &str = "20261103T2200-drop-question-answer";
const CONTEXT: &str = "cx";

fn home_in(dir: &tempfile::TempDir) -> CpdHome {
    CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
}

/// A raw connection that can write the rows the store's triggers watch.
fn raw(home: &CpdHome) -> Connection {
    let conn = Connection::open(home.db()).unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64))
        .unwrap();
    conn
}

/// Makes the store as it was before the migration: it is not recorded as applied.
fn rewind(conn: &Connection) {
    let guard: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'schema_migrations_no_delete'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute_batch("DROP TRIGGER schema_migrations_no_delete")
        .unwrap();
    let removed = conn
        .execute(
            "DELETE FROM schema_migrations WHERE name >= ?1",
            params![MIGRATION],
        )
        .unwrap();
    assert!(removed >= 1, "{MIGRATION} is in the list of migrations");
    conn.execute_batch(&guard).unwrap();
}

fn add_record(conn: &Connection, id: &str, kind: &str, question: Option<&str>) {
    conn.execute(
        "INSERT INTO context_records (context, id, kind, at, machine, text, question) \
         VALUES (?1, ?2, ?3, '2026-11-03T12:00:00Z', 'm', 'text', ?4)",
        params![CONTEXT, id, kind, question],
    )
    .unwrap();
}

/// A store before the migration: `cx` holds `cx-d1`, `cx-n1` and, with `with_qa`, `cx-q1`
/// (with a curation and a retirement) and `cx-a1`, the answer to `cx-q1`.
fn store_before(home: &CpdHome, with_qa: bool) {
    drop(Store::open(home, 5000).unwrap());
    let conn = raw(home);
    rewind(&conn);
    conn.execute(
        "INSERT INTO contexts (id, title, created_at, created_on) \
         VALUES (?1, 't', '2026-11-03T12:00:00Z', 'm')",
        params![CONTEXT],
    )
    .unwrap();
    add_record(&conn, "cx-d1", "decision", None);
    add_record(&conn, "cx-n1", "note", None);
    if with_qa {
        add_record(&conn, "cx-q1", "question", None);
        add_record(&conn, "cx-a1", "answer", Some("cx-q1"));
        conn.execute(
            "INSERT INTO record_curations (context, record, topic, short, curated_at) \
             VALUES (?1, 'cx-q1', 'topic', 'short', '2026-11-03T12:00:00Z')",
            params![CONTEXT],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at) \
             VALUES (?1, 'cx-q1', 'duplicate', 'cx-d1', 'pending', '2026-11-03T12:00:00Z')",
            params![CONTEXT],
        )
        .unwrap();
    }
}

/// Runs the migration: opens the store, which applies what is pending, and returns a reader.
fn migrate(home: &CpdHome) -> Connection {
    let store = Store::open(home, 5000).unwrap_or_else(|e| panic!("the migration failed: {e:?}"));
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    drop(store);
    raw(home)
}

fn strings(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn drop_qa_deletes_question_and_answer_rows() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    store_before(&home, true);
    let conn = migrate(&home);
    assert_eq!(
        strings(
            &conn,
            "SELECT id FROM context_records WHERE context = 'cx' ORDER BY id"
        ),
        ["cx-d1", "cx-n1"]
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM record_curations WHERE record IN ('cx-q1', 'cx-a1')"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM record_retirements WHERE record IN ('cx-q1', 'cx-a1')"
        ),
        0
    );
}

#[test]
fn drop_qa_writes_history_delete_rows() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    store_before(&home, true);
    let conn = migrate(&home);
    let ops = |op: &str| {
        count(
            &conn,
            &format!(
                "SELECT count(*) FROM history WHERE tbl = 'context_records' AND op = '{op}' \
                 AND json_extract(before, '$.kind') IN ('question', 'answer')"
            ),
        )
    };
    assert_eq!(
        ops("insert"),
        0,
        "an insert row has no `before`; see the inserts below"
    );
    assert_eq!(
        ops("delete"),
        2,
        "one delete row for the question, one for the answer"
    );
    let inserts = count(
        &conn,
        "SELECT count(*) FROM history WHERE tbl = 'context_records' AND op = 'insert' \
         AND json_extract(after, '$.kind') IN ('question', 'answer')",
    );
    assert_eq!(inserts, 2, "the insert rows stay");
}

#[test]
fn drop_qa_keeps_record_kinds_rows() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    store_before(&home, true);
    let conn = migrate(&home);
    assert_eq!(
        strings(&conn, "SELECT name FROM record_kinds ORDER BY name").len(),
        6
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM context_records WHERE kind IN ('question', 'answer')"
        ),
        0
    );
}

#[test]
fn drop_qa_keeps_the_delete_guard() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    store_before(&home, true);
    let conn = migrate(&home);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM context_records WHERE kind = 'question'"
        ),
        0
    );
    for table in ["context_records", "record_curations", "record_retirements"] {
        let name = format!("{table}_no_delete");
        assert_eq!(
            count(
                &conn,
                &format!(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name = '{name}'"
                )
            ),
            1,
            "{name} exists after the migration"
        );
    }
    assert!(
        conn.execute("DELETE FROM context_records WHERE id = 'cx-d1'", [])
            .is_err(),
        "a plain DELETE of a decision is refused"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM context_records WHERE id = 'cx-d1'"
        ),
        1
    );
}

#[test]
fn drop_qa_is_idempotent_on_a_clean_store() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    store_before(&home, false);
    // Red against the stub: a clean store alone cannot tell a stub from the migration, so the
    // migration text must hold the delete of the rows it is named for.
    let sql = cpd_core::store::testing::named_migrations()
        .into_iter()
        .find(|(name, ..)| *name == MIGRATION)
        .map(|(.., sql)| sql)
        .unwrap();
    assert!(
        sql.contains("DELETE FROM context_records"),
        "{MIGRATION} deletes the question and answer rows"
    );
    let conn = migrate(&home);
    assert_eq!(
        strings(
            &conn,
            "SELECT id FROM context_records WHERE context = 'cx' ORDER BY id"
        ),
        ["cx-d1", "cx-n1"]
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM schema_migrations WHERE name = '20261103T2200-drop-question-answer'"
        ),
        1,
        "the migration is recorded as applied"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM history WHERE op = 'delete' AND tbl = 'context_records'"
        ),
        0
    );
}

#[test]
fn drop_qa_deletes_inbox_items_naming_a_deleted_record() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    store_before(&home, true);
    let conn = raw(&home);
    for (subject, record, closed) in [
        ("on a question", "cx-q1", None),
        (
            "on an answer, closed",
            "cx-a1",
            Some("2026-11-03T13:00:00Z"),
        ),
        ("on a decision", "cx-d1", None),
    ] {
        conn.execute(
            "INSERT INTO inbox_items (kind, urgency, subject, context, record, label, opened_at, closed_at) \
             VALUES ('unread', 'white', ?1, ?2, ?3, 'l', '2026-11-03T12:00:00Z', ?4)",
            params![subject, CONTEXT, record, closed],
        )
        .unwrap();
    }
    drop(conn);
    let conn = migrate(&home);
    assert_eq!(
        strings(&conn, "SELECT record FROM inbox_items ORDER BY id"),
        ["cx-d1"]
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name = 'inbox_items_no_delete'"
        ),
        1,
        "the delete guard is back"
    );
}
