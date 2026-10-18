//! The SQLite store (contract §4.1–§4.5): open, the DDL, its constraints and
//! triggers, writes only through cpd, migrations and two writers.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TestEnv;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::store::{APPLICATION_ID, HISTORY_TABLES, SCHEMA_HASH, Store, VERSION, WriteTxn, canonical_hash};
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;
use serde_json::{Value, json};

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn open(env: &TestEnv) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

/// A connection that is not cpd's, like the `sqlite3` shell: no `cpd_txn()`.
fn raw(env: &TestEnv) -> Connection {
    let conn = Connection::open(home(env).db()).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn
}

fn sqlite_message(err: &CpdError) -> String {
    err.details()["sqlite_message"].as_str().unwrap_or_default().to_owned()
}

/// Runs `sql` in its own write transaction and returns the refusal.
fn refused(store: &mut Store, sql: &str) -> CpdError {
    store
        .write("test", |tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .expect_err(sql)
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A Context, its Lead, a Plan with one PR `pay`, a `queued` Job `api` (with a
/// running Attempt and an open Escalation) and a `draft` Job `ui` that needs `api`.
fn fixture(store: &mut Store) -> i64 {
    store
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                       VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
                     INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                       VALUES ('pay', 'pay', '/r', 'main', 'waiting', 'cpd/pay/pay', {id}, 't');
                     INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'pay', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{FP}', {id}, 't'),
                              ('pay', 'pay', 'ui', 'Ui', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, NULL, {id}, 't');
                     INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'ui', 'api');
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 't');
                     INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                       VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Why?', 'open', 1, 't');",
                    id = tx.id()
                ),
            )?;
            Ok(tx.id())
        })
        .unwrap()
}

/// An open store holding the fixture's Context, Lead and Plan.
fn fixture_store(env: &TestEnv) -> Store {
    let mut store = open(env);
    fixture(&mut store);
    store
}

fn names(conn: &Connection, sql: &str) -> BTreeSet<String> {
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

// ---------- §4.1 the database ----------

#[test]
fn a_new_database_is_wal_with_cpds_application_id() {
    let env = TestEnv::new();
    let store = open(&env);
    assert_eq!(store.path(), env.cpd_home().join("cpd.db"));
    assert!(store.schema_mismatch().is_none());
    drop(store);
    let conn = raw(&env);
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    let app: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0)).unwrap();
    assert_eq!(app, 1668310066);
    assert_eq!(APPLICATION_ID, 1668310066);
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, VERSION);
}

#[test]
fn cpd_home_is_created_on_first_use() {
    let env = TestEnv::new();
    let nested = env.root().join("a/b/home");
    let store = Store::open(&CpdHome::resolve(Some(nested.clone().into()), None).unwrap(), 5000).unwrap();
    assert_eq!(store.path(), nested.join("cpd.db"));
    assert!(nested.join("cpd.db").is_file());
}

#[test]
fn a_file_that_is_not_a_database_is_store_failed() {
    let env = TestEnv::new();
    fs::write(home(&env).db(), "this is not sqlite, not at all, and long enough to have a header").unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["path"], home(&env).db().display().to_string());
    assert!(err.details()["sqlite_code"].is_i64(), "{err:?}");
}

#[test]
fn a_foreign_application_id_is_refused_and_never_written() {
    let env = TestEnv::new();
    let conn = Connection::open(home(&env).db()).unwrap();
    conn.pragma_update(None, "application_id", 42).unwrap();
    drop(conn);
    let before = fs::read(home(&env).db()).unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert_eq!(err.details()["application_id"], 42);
    assert_eq!(fs::read(home(&env).db()).unwrap(), before);
    let conn = Connection::open(home(&env).db()).unwrap();
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
    assert_eq!(mode, "delete");
}

// ---------- §4.4 the DDL ----------

fn contract() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/contract.md")).unwrap()
}

/// What contract §4.4 describes that the Job `kind-tables` has yet to create: the lookup tables, and
/// `events` and `context_records`, whose `kind` is a foreign key there and still a `CHECK` here. The
/// Job removes this list.
const AFTER_KIND_TABLES: [&str; 5] = ["event_kinds", "record_kinds", "history_tables", "events", "context_records"];

/// The ```sql blocks of contract §4.4, in order.
fn contract_ddl_blocks() -> Vec<String> {
    let text = contract();
    let start = text.find("### 4.4 DDL").unwrap();
    let end = text.find("### 4.5 ").unwrap();
    let section = &text[start..end];
    section.split("```sql\n").skip(1).map(|b| b.split("```").next().unwrap().to_owned()).collect()
}

fn normalized(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `(name, sql)` of every table, index and trigger in `conn`, normalized: a
/// table rebuilt by a migration is stored as `CREATE TABLE "<name>"`.
fn schema_sql(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(|row| {
            let (name, sql) = row.unwrap();
            let sql = normalized(&sql.replacen(&format!("CREATE TABLE \"{name}\""), &format!("CREATE TABLE {name}"), 1));
            (name, sql)
        })
        .collect()
}

#[test]
fn the_schema_is_contract_4_4_as_written() {
    let blocks = contract_ddl_blocks();
    assert_eq!(blocks.len(), 9, "§4.4 changed shape");
    assert!(blocks[2].contains("CREATE TRIGGER escalations_hist_ins"), "block 3 is the generated example");
    // The contract's DDL, every block but the generated example, run as written.
    let contract = Connection::open_in_memory().unwrap();
    for (i, block) in blocks.iter().enumerate() {
        if i != 2 {
            contract.execute_batch(block).unwrap_or_else(|e| panic!("block {}: {e}", i + 1));
        }
    }
    let expected = schema_sql(&contract);
    assert!(expected.len() > 40, "{expected:?}");
    let env = TestEnv::new();
    drop(open(&env));
    let found: std::collections::BTreeMap<String, String> = schema_sql(&raw(&env)).into_iter().collect();
    for (name, sql) in expected {
        if !AFTER_KIND_TABLES.contains(&name.as_str()) {
            assert_eq!(found.get(&name), Some(&sql), "{name}");
        }
    }
}

#[test]
fn every_table_index_and_trigger_of_4_4_exists() {
    let env = TestEnv::new();
    drop(open(&env));
    let mut expected = BTreeSet::new();
    for block in contract_ddl_blocks() {
        for line in block.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let name = match words.as_slice() {
                ["CREATE", "TABLE" | "INDEX" | "TRIGGER", name, ..] => name,
                ["CREATE", "UNIQUE", "INDEX", name, ..] => name,
                _ => continue,
            };
            if !AFTER_KIND_TABLES[..3].contains(name) {
                expected.insert(name.to_string());
            }
        }
    }
    for table in HISTORY_TABLES {
        expected.insert(format!("{table}_hist_ins"));
        expected.insert(format!("{table}_hist_upd"));
        // `pull_requests` has no `_no_delete`: an unowned row may be deleted (`pull_requests_owned` is in the DDL).
        if *table != "pull_requests" {
            expected.insert(format!("{table}_no_delete"));
        }
    }
    // Described in §4.4's prose, written like `dependencies_hist_del`.
    expected.insert("pr_needs_hist_del".to_owned());
    expected.insert("pull_requests_hist_del".to_owned());
    let found = names(
        &raw(&env),
        "SELECT name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
    );
    assert_eq!(found, expected);
    // Every table is STRICT.
    let tables = names(&raw(&env), "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'");
    assert_eq!(tables.len(), 17, "§4.4's tables but the three lookup tables, and schema_migrations");
    for table in tables {
        let strict: i64 = raw(&env)
            .query_row("SELECT strict FROM pragma_table_list WHERE name = ?1", [&table], |r| r.get(0))
            .unwrap();
        assert_eq!(strict, 1, "{table}");
    }
}

#[test]
fn generated_history_triggers_match_the_contracts_example() {
    let env = TestEnv::new();
    drop(open(&env));
    let conn = raw(&env);
    let example = &contract_ddl_blocks()[2];
    for statement in example.split_inclusive("END;") {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        let name = statement.split_whitespace().nth(2).unwrap();
        let sql: String =
            conn.query_row("SELECT sql FROM sqlite_schema WHERE name = ?1", [name], |r| r.get(0)).unwrap();
        assert_eq!(normalized(&format!("{sql};")), normalized(statement), "{name}");
    }
}

#[test]
fn every_column_of_every_table_appears_in_its_history_triggers() {
    let env = TestEnv::new();
    drop(open(&env));
    let conn = raw(&env);
    let sql_of = |name: &str| -> String {
        conn.query_row("SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1", [name], |r| r.get(0))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    for table in HISTORY_TABLES {
        let columns = names(&conn, &format!("SELECT name FROM pragma_table_info('{table}')"));
        assert!(!columns.is_empty(), "{table}");
        let ins = sql_of(&format!("{table}_hist_ins"));
        let upd = sql_of(&format!("{table}_hist_upd"));
        for column in &columns {
            assert!(ins.contains(&format!("'{column}', NEW.{column}")), "{table}_hist_ins lacks {column}");
            assert!(upd.contains(&format!("'{column}', OLD.{column}")), "{table}_hist_upd lacks OLD.{column}");
            assert!(upd.contains(&format!("'{column}', NEW.{column}")), "{table}_hist_upd lacks NEW.{column}");
        }
        if ["dependencies", "pr_needs", "pull_requests"].contains(table) {
            let del = sql_of(&format!("{table}_hist_del"));
            for column in &columns {
                assert!(del.contains(&format!("'{column}', OLD.{column}")), "{table}_hist_del lacks {column}");
            }
        }
    }
}

#[test]
fn writes_are_copied_into_history_with_their_txn() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let txn = fixture(&mut store);
    let update = store
        .write("test", |tx| {
            exec(tx, "UPDATE escalations SET status = 'answered', answer = 'Because.', answered_at = 't'")?;
            Ok(tx.id())
        })
        .unwrap();
    assert_eq!(update, txn + 1);
    let rows: Vec<(i64, String, String, Option<String>, Option<String>)> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT txn, pk, op, before, after FROM history WHERE tbl = 'escalations' ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].1.as_str(), rows[0].2.as_str()), (txn, r#"["pay","api-a1-q1"]"#, "insert"));
    assert_eq!(rows[0].3, None);
    assert_eq!((rows[1].0, rows[1].2.as_str()), (txn + 1, "update"));
    let before: Value = serde_json::from_str(rows[1].3.as_deref().unwrap()).unwrap();
    let after: Value = serde_json::from_str(rows[1].4.as_deref().unwrap()).unwrap();
    assert_eq!(before["status"], "open");
    assert_eq!(after["status"], "answered");
    assert_eq!(after["answer"], "Because.");
    assert_eq!(after["asker_pid"], 1);
}

#[test]
fn check_constraints_reject_a_bad_value_in_every_constrained_table() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let txn = fixture(&mut store);
    let cases = [
        ("history", "INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'x', '{}', 'insert', NULL, '{}')".to_owned()),
        ("contexts", "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('x', 'T', 't', 'm')".to_owned()),
        ("context_records", "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'n1', 'bogus', 't', 'm', 'x')".to_owned()),
        ("plans", "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('ops', 'ctx', 'O', 'bogus', 'lead1', 't')".to_owned()),
        ("prs", format!("INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES ('pay', 'web', '/r', 'main', 'waiting', 'cpd/other', {txn}, 't')")),
        ("pr_needs", "INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', 'pay', 'pay')".to_owned()),
        ("plan_reviews", format!("INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('pay', 'r2', 1, '{FP}', 'pending', 't')")),
        ("jobs", format!("INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at) VALUES ('pay', 'pay', 'web', 'W', 'X9', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, NULL, {txn}, 't')")),
        ("dependencies", "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'ui', 'ui')".to_owned()),
        ("attempts", "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at) VALUES ('pay', 'ui-a1', 'ui', 1, 'S2', 'ladder', 'reviewing', 'cpd/pay--ui-a1', '/w', 't')".to_owned()),
        ("escalations", "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at) VALUES ('pay', 'api-a1-q2', 'api-a1', 2, 'Q', 'pending', 1, 't')".to_owned()),
        ("sessions", "INSERT INTO sessions (id, role, context, name, cwd, tier, tmux_pane, state, state_at, created_at) VALUES ('s2', 'lead', 'ctx', 'L', '/', 'O3', 'x1', 'idle', 't', 't')".to_owned()),
        ("events", "INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', 'review-needed', 'ctx', 'x', '{}')".to_owned()),
        ("tier_catalog", "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1', 'X', 'm', '[]', 't')".to_owned()),
    ];
    for (table, sql) in cases {
        let err = refused(&mut store, &sql);
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{table}: {err:?}");
        assert!(sqlite_message(&err).contains("CHECK constraint failed"), "{table}: {err:?}");
    }
    // The same rows with the bad value fixed are accepted, so the CHECK was the cause.
    store
        .write("test", |tx| {
            exec(tx, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('xy', 'T', 't', 'm');
                      INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1', 'O', 'm', '[]', 't');
                      INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', 'job-added', 'ctx', 'x', '{}');")
        })
        .unwrap();
}

#[test]
fn transitions_outside_section_3_are_illegal_transition() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    for sql in [
        "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay'",
        "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api'",
        "UPDATE attempts SET status = 'starting' WHERE id = 'api-a1'",
        "UPDATE escalations SET status = 'answered', answer = 'a', answered_at = 't' WHERE id = 'api-a1-q1';
         UPDATE escalations SET status = 'expired', expired_at = 't' WHERE id = 'api-a1-q1'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
        assert_eq!(err.exit().code(), 3);
    }
    // A legal one passes; an ended Attempt then accepts no update at all.
    store
        .write("test", |tx| {
            exec(tx, "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
                      UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'api-a1'")
        })
        .unwrap();
    let err = refused(&mut store, "UPDATE attempts SET metrics = '{}' WHERE id = 'api-a1'");
    assert_eq!(err.code(), ErrorCode::IllegalTransition, "{err:?}");
}

#[test]
fn a_locked_job_and_its_dependencies_are_job_locked() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    for sql in [
        "UPDATE jobs SET title = 'New' WHERE key = 'api'",
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'pay', 'web', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, 1, 't');
         INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'api', 'web')",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::JobLocked, "{sql}: {err:?}");
    }
    // A draft Job's Dependency may be deleted, and the deletion is in history.
    store.write("test", |tx| exec(tx, "UPDATE jobs SET title = 'Ui 2' WHERE key = 'ui'; DELETE FROM dependencies WHERE job = 'ui'")).unwrap();
    let op: String = store
        .read(|c| Ok(c.query_row("SELECT op FROM history WHERE tbl = 'dependencies' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(op, "delete");
    // A Locked Job's Dependencies may not be.
    store
        .write("test", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'ui', 'api');
                     UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 'ui'"
                ),
            )
        })
        .unwrap();
    let err = refused(&mut store, "DELETE FROM dependencies WHERE job = 'ui'");
    assert_eq!(err.code(), ErrorCode::JobLocked, "{err:?}");
}

#[test]
fn appending_a_job_inserts_its_dependencies_in_its_own_transaction() {
    // dependencies_locked: a non-draft Job created in this transaction (append) may get Dependencies.
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    store
        .write("job add", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'pay', 'web', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, '{FP}', {}, 't');
                     INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'web', 'api');",
                    tx.id()
                ),
            )
        })
        .unwrap();
}

#[test]
fn append_only_tables_refuse_updates_and_deletes() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    store
        .write("test", |tx| {
            exec(tx, "INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', 'job-added', 'ctx', 'x', '{}');
                      INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'n1', 'note', 't', 'm', 'x');")
        })
        .unwrap();
    let cases = [
        ("UPDATE txns SET command = 'x'", "append_only: txns"),
        ("DELETE FROM txns", "append_only: txns"),
        ("UPDATE history SET tbl = 'x'", "append_only: history"),
        ("DELETE FROM history", "append_only: history"),
        ("UPDATE events SET text = 'y'", "append_only: events"),
        ("DELETE FROM events", "append_only: events"),
        ("UPDATE context_records SET text = 'y'", "append_only: context_records"),
        ("DELETE FROM escalations", "append_only: escalations"),
        ("DELETE FROM contexts WHERE id = 'nope'", ""),
    ];
    for (sql, message) in cases {
        if message.is_empty() {
            // A delete that matches no row fires no trigger.
            store.write("test", |tx| exec(tx, sql)).unwrap();
            continue;
        }
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{sql}: {err:?}");
        assert_eq!(sqlite_message(&err), message, "{sql}");
    }
    // The fixture's rows: the generated `<table>_no_delete` refuses them.
    for table in ["contexts", "sessions", "plans", "prs", "jobs", "attempts", "escalations"] {
        let err = refused(&mut store, &format!("DELETE FROM {table}"));
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{table}: {err:?}");
        assert_eq!(sqlite_message(&err), format!("append_only: {table}"));
    }
}

// ---------- §4.3 writes only through cpd ----------

#[test]
fn a_connection_without_cpd_txn_cannot_write() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    let conn = raw(&env);
    for sql in [
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('zz', 'T', 't', 'm')",
        "UPDATE contexts SET title = 'Changed'",
        "UPDATE jobs SET status = 'running' WHERE key = 'api'",
        "INSERT INTO txns (id, at, command, pid, cwd) VALUES (100, 't', 'sqlite3', 1, '/')",
        "INSERT INTO events (txn, at, kind, context, text, data) VALUES (1, 't', 'job-added', 'ctx', 'x', '{}')",
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1', 'O', 'm', '[]', 't')",
        // A forged history row naming an existing txn (Escalation rs-store-a1-q2: history_own).
        "INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (2, 'jobs', '[1]', 'update', '{}', '{}')",
    ] {
        let err = conn.execute_batch(sql).expect_err(sql);
        assert!(err.to_string().contains("no such function: cpd_txn"), "{sql}: {err}");
    }
    let title: String = conn.query_row("SELECT title FROM contexts WHERE id = 'ctx'", [], |r| r.get(0)).unwrap();
    assert_eq!(title, "Ctx");
}

#[test]
fn a_cpd_txn_that_does_not_match_the_txns_row_is_refused() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    let conn = raw(&env);
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(77i64)).unwrap();
    let err = conn
        .execute_batch("INSERT INTO txns (id, at, command, pid, cwd) VALUES (78, 't', 'forged', 1, '/')")
        .unwrap_err();
    assert!(err.to_string().contains("txns.id must be cpd_txn()"), "{err}");
    let err = conn
        .execute_batch("INSERT INTO events (txn, at, kind, context, text, data) VALUES (1, 't', 'job-added', 'ctx', 'x', '{}')")
        .unwrap_err();
    assert!(err.to_string().contains("events.txn must be cpd_txn()"), "{err}");
    let err = conn
        .execute_batch("INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (2, 'jobs', '[1]', 'update', '{}', '{}')")
        .unwrap_err();
    assert!(err.to_string().contains("history.txn must be cpd_txn()"), "{err}");
    let forged: i64 = conn.query_row("SELECT count(*) FROM history WHERE txn = 2 AND op = 'update'", [], |r| r.get(0)).unwrap();
    assert_eq!(forged, 0);
}

#[test]
fn a_read_transaction_cannot_write() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    let err = store
        .read(|c| {
            c.execute("UPDATE contexts SET title = 'Changed'", [])?;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::StoreFailed);
    assert!(sqlite_message(&err).contains("outside a cpd write transaction"), "{err:?}");
}

#[test]
fn every_write_transaction_has_its_txns_row() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let id = store.write("job add", |tx| Ok(tx.id())).unwrap();
    assert_eq!(id, 2, "txn 1 is the migration");
    let row: (String, i64, String) = store
        .read(|c| Ok(c.query_row("SELECT command, pid, cwd FROM txns WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    assert_eq!(row.0, "job add");
    assert_eq!(row.1, std::process::id() as i64);
    assert_eq!(row.2, std::env::current_dir().unwrap().to_string_lossy());
    let at: String = store.read(|c| Ok(c.query_row("SELECT at FROM txns WHERE id = 2", [], |r| r.get(0))?)).unwrap();
    assert!(at.len() == 24 && at.ends_with('Z') && at.as_bytes()[19] == b'.', "{at}");
}

#[test]
fn a_failed_write_rolls_back_everything_including_its_txns_row() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let err = store
        .write("test", |tx| {
            exec(tx, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm')")?;
            Err::<(), _>(CpdError::new(ErrorCode::CasLost, "lost"))
        })
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::CasLost);
    let counts: (i64, i64, i64) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT count(*) FROM txns), (SELECT count(*) FROM contexts), (SELECT count(*) FROM history)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(counts, (1, 0, 0));
    // The next write reuses the id.
    assert_eq!(store.write("test", |tx| Ok(tx.id())).unwrap(), 2);
}

#[test]
fn a_changed_schema_refuses_writes_but_not_reads() {
    let env = TestEnv::new();
    let mut store = open(&env);
    fixture(&mut store);
    drop(store);
    raw(&env).execute_batch("DROP TRIGGER contexts_no_delete").unwrap();
    let mut store = open(&env);
    assert!(store.schema_mismatch().is_some());
    let err = store.write("test", |_| Ok(())).unwrap_err();
    assert_eq!(err.code(), ErrorCode::StoreTampered);
    assert_eq!(err.exit().code(), 1);
    let n: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM contexts", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 1);
    let report = store.tamper().unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0]["kind"], "schema");
}

#[test]
fn the_tamper_report_lists_history_rows_without_a_txn() {
    let env = TestEnv::new();
    let mut store = open(&env);
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
    let conn = raw(&env);
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(999i64)).unwrap();
    conn.execute_batch("INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (999, 'contexts', '[\"x\"]', 'insert', NULL, '{}')")
        .unwrap();
    assert_eq!(store.tamper().unwrap(), vec![json!({"kind": "history", "id": 1, "txn": 999})]);
}

// ---------- §4.5 versions and migrations ----------

#[test]
fn migrating_an_empty_file_creates_the_schema_once() {
    let env = TestEnv::new();
    fs::write(home(&env).db(), b"").unwrap();
    let store = open(&env);
    assert!(store.schema_mismatch().is_none());
    drop(store);
    // Reopening migrates nothing more: idempotent.
    let mut store = open(&env);
    let rows: Vec<(i64, String)> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT id, command FROM txns")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(rows, vec![(1, "migrate".to_owned())]);
    let conn = raw(&env);
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, VERSION);
    let app: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0)).unwrap();
    assert_eq!(app, APPLICATION_ID);
}

#[test]
fn a_fresh_database_has_archived_at_and_the_five_new_event_kinds_and_matches_its_schema_hash() {
    let env = TestEnv::new();
    let mut store = fixture_store(&env);
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    let hash = store.read(|c| Ok(canonical_hash(c)?)).unwrap();
    assert_eq!(hash, SCHEMA_HASH, "a fresh store hashes to the hash pinned in the code (§4.3)");

    for table in ["contexts", "plans", "sessions"] {
        let archived: Option<String> =
            store.read(|c| Ok(c.query_row(&format!("SELECT archived_at FROM {table} LIMIT 1"), [], |r| r.get(0))?)).unwrap();
        assert_eq!(archived, None, "{table}.archived_at is null until the first archive");
    }
    let insert = |tx: &WriteTxn<'_>, kind: &str| {
        tx.execute(
            "INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', ?1, 'ctx', 'x', '{}')",
            [kind],
        )
    };
    let mut last = 0;
    for kind in ["context-renamed", "plan-renamed", "context-archived", "plan-archived", "session-archived"] {
        last = store
            .write("test", |tx| {
                insert(tx, kind)?;
                Ok(tx.last_insert_rowid())
            })
            .unwrap_or_else(|e| panic!("{kind}: {e:?}"));
    }
    assert_eq!(last, 5);
    let err = store.write("test", |tx| Ok(insert(tx, "plan-exploded")?)).unwrap_err();
    assert!(format!("{err:?}").contains("CHECK"), "{err:?}");
}

#[test]
fn a_newer_schema_is_store_too_new() {
    let env = TestEnv::new();
    drop(open(&env));
    raw(&env).pragma_update(None, "user_version", VERSION + 1).unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["user_version"], VERSION + 1);
    assert_eq!(err.details()["floor"], Value::Null);
    assert_eq!(err.details()["unknown"], json!([]));
}

#[test]
fn a_failed_migration_rolls_back_entirely() {
    let env = TestEnv::new();
    // A database whose migration 1 cannot apply: a table `history` already exists.
    raw(&env).execute_batch("CREATE TABLE history (x)").unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["migration"], "0001-initial");
    let conn = raw(&env);
    let tables = names(&conn, "SELECT name FROM sqlite_schema");
    assert_eq!(tables, BTreeSet::from(["history".to_owned()]));
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, 0);
    let app: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0)).unwrap();
    assert_eq!(app, 0);
}

// ---------- §4.5 named migrations: the runner's table ----------

#[test]
fn schema_migrations_records_every_legacy_migration_and_is_written_only_by_the_runner() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let rows: Vec<(String, String, i64, i64)> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT name, compat, seq, converted FROM schema_migrations ORDER BY seq")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();
    let expected = [
        ("0001-initial", "breaking"),
        ("0002-cockpit-parity", "additive"),
        ("0003-archive", "breaking"),
        ("0004-cross-repo", "breaking"),
        ("0005-plan-reopen", "breaking"),
        ("0006-pr-poller", "breaking"),
    ];
    let expected: Vec<(String, String, i64, i64)> =
        expected.iter().enumerate().map(|(i, (n, c))| (n.to_string(), c.to_string(), i as i64 + 1, 0)).collect();
    assert_eq!(rows, expected, "a fresh store records its legacy migrations as not converted");

    // Not from the `sqlite3` shell: there is no `cpd_txn()`.
    let err = raw(&env)
        .execute("INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted) VALUES ('x-xxxx', 'additive', ?1, 99, 1, 0)", [FP])
        .unwrap_err();
    assert!(err.to_string().contains("cpd_txn"), "{err}");
    // Not by cpd either, once recorded.
    for sql in ["UPDATE schema_migrations SET compat = 'additive'", "DELETE FROM schema_migrations"] {
        let err = refused(&mut store, sql);
        assert!(sqlite_message(&err).starts_with("append_only"), "{sql}: {err:?}");
    }
    // And only for the migrating transaction.
    let err = refused(
        &mut store,
        &format!("INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted) VALUES ('x-xxxx', 'additive', '{FP}', 99, 1, 0)"),
    );
    assert!(sqlite_message(&err).contains("must be cpd_txn()"), "{err:?}");
}

// ---------- §4.2 two writers in separate processes ----------

/// Not a test on its own: the body of the child processes below. Without
/// `STORE_CHILD_OUT` it does nothing.
#[test]
fn child_writer() {
    let Some(out) = std::env::var_os("STORE_CHILD_OUT") else { return };
    let var = |name: &str| std::env::var(name).unwrap();
    let home = CpdHome::resolve(Some(var("STORE_CHILD_HOME").into()), None).unwrap();
    let busy_ms: u64 = var("STORE_CHILD_BUSY_MS").parse().unwrap();
    let hold = Duration::from_millis(var("STORE_CHILD_HOLD_MS").parse().unwrap());
    let started = std::env::var_os("STORE_CHILD_STARTED").map(PathBuf::from);
    let begun = now_ms();
    let result = Store::open(&home, busy_ms).and_then(|mut store| {
        store.write("test writer", |tx| {
            let entered = now_ms();
            if let Some(started) = &started {
                fs::write(started, "").unwrap();
            }
            std::thread::sleep(hold);
            Ok(json!({"txn": tx.id(), "entered": entered, "left": now_ms()}))
        })
    });
    let value = match result {
        Ok(v) => json!({"ok": true, "begun": begun, "result": v}),
        Err(e) => json!({"ok": false, "begun": begun, "ended": now_ms(), "error": e.to_json()}),
    };
    fs::write(out, value.to_string()).unwrap();
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Kills and reaps its child on drop, so a panicking test leaves no process.
struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    fn new(child: Child) -> Self {
        KillOnDrop(Some(child))
    }

    fn into_inner(mut self) -> Child {
        self.0.take().unwrap()
    }
}

impl std::ops::Deref for KillOnDrop {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for KillOnDrop {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn a_guarded_child_is_gone_after_a_panic() {
    let pid = std::cell::Cell::new(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let child = KillOnDrop::new(std::process::Command::new("sleep").arg("60").spawn().unwrap());
        pid.set(child.id());
        panic!("boom");
    }));
    assert!(result.is_err());
    let alive = std::process::Command::new("kill").args(["-0", &pid.get().to_string()]).output().unwrap().status.success();
    assert!(!alive, "child {} survived the panic", pid.get());
}

struct Writer {
    child: KillOnDrop,
    out: PathBuf,
}

impl Writer {
    fn spawn(env: &TestEnv, name: &str, busy_ms: u64, hold_ms: u64, started: Option<&Path>) -> Writer {
        let out = env.root().join(format!("{name}.json"));
        let mut cmd = env.command(std::env::current_exe().unwrap());
        cmd.args(["child_writer", "--exact", "--test-threads=1", "--quiet"])
            .env("STORE_CHILD_OUT", &out)
            .env("STORE_CHILD_HOME", env.cpd_home())
            .env("STORE_CHILD_BUSY_MS", busy_ms.to_string())
            .env("STORE_CHILD_HOLD_MS", hold_ms.to_string())
            .stdout(std::process::Stdio::null());
        if let Some(started) = started {
            cmd.env("STORE_CHILD_STARTED", started);
        }
        Writer { child: KillOnDrop::new(cmd.spawn().unwrap()), out }
    }

    fn wait(mut self) -> Value {
        assert!(self.child.wait().unwrap().success());
        serde_json::from_str(&fs::read_to_string(&self.out).unwrap()).unwrap()
    }
}

impl Writer {
    /// Waits until the writer is inside its write transaction.
    fn wait_started(&mut self, started: &Path) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !started.exists() {
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!("the writer exited ({status}) before starting: {}", fs::read_to_string(&self.out).unwrap_or_default());
            }
            assert!(Instant::now() < deadline, "{} never appeared", started.display());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn a_second_writer_waits_for_the_first_and_reads_do_not() {
    let env = TestEnv::new();
    drop(open(&env));
    let started = env.root().join("a.started");
    let mut a = Writer::spawn(&env, "a", 5000, 1500, Some(&started));
    a.wait_started(&started);
    let b = Writer::spawn(&env, "b", 20_000, 0, None);
    // While A holds the write lock, a reader opens and reads at once.
    let t = Instant::now();
    let mut reader = open(&env);
    let txns: i64 = reader.read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap();
    assert_eq!(txns, 1, "A has not committed yet");
    assert!(t.elapsed() < Duration::from_millis(1000), "the reader waited {:?}", t.elapsed());
    let (a, b) = (a.wait(), b.wait());
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(b["ok"], true, "{b}");
    assert_eq!(a["result"]["txn"], 2);
    assert_eq!(b["result"]["txn"], 3);
    assert!(b["result"]["entered"].as_u64() >= a["result"]["left"].as_u64(), "B ran while A held the lock: {a} {b}");
}

#[test]
fn a_writer_that_cannot_get_the_lock_in_time_is_store_busy() {
    let env = TestEnv::new();
    drop(open(&env));
    let started = env.root().join("a.started");
    let mut a = Writer::spawn(&env, "a", 5000, 3000, Some(&started));
    a.wait_started(&started);
    let b = Writer::spawn(&env, "b", 200, 0, None).wait();
    assert_eq!(b["ok"], false, "{b}");
    assert_eq!(b["error"]["code"], "store_busy", "{b}");
    assert_eq!(b["error"]["details"]["busy_ms"], 200);
    let waited = b["ended"].as_u64().unwrap() - b["begun"].as_u64().unwrap();
    assert!(waited >= 200, "gave up after {waited} ms");
    assert_eq!(a.wait()["ok"], true);
    let mut store = open(&env);
    let txns: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap();
    assert_eq!(txns, 2, "B wrote nothing");
}

#[test]
fn processes_opening_a_new_database_at_once_migrate_it_once() {
    let env = TestEnv::new();
    let writers: Vec<Writer> = (0..4).map(|i| Writer::spawn(&env, &format!("w{i}"), 20_000, 0, None)).collect();
    let mut ids: Vec<u64> = writers
        .into_iter()
        .map(|w| {
            let v = w.wait();
            assert_eq!(v["ok"], true, "{v}");
            v["result"]["txn"].as_u64().unwrap()
        })
        .collect();
    ids.sort();
    assert_eq!(ids, vec![2, 3, 4, 5]);
    let mut store = open(&env);
    let migrations: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM txns WHERE command = 'migrate'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(migrations, 1);
}


#[test]
fn version_on_an_empty_home_reports_no_store_schema_and_creates_nothing() {
    let env = TestEnv::new();
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["schema"], VERSION);
    assert!(out["store_schema"].is_null(), "{out}");
    assert_eq!(out["protocol"], 1, "{out}");
    assert!(!home(&env).db().exists());
    assert!(!home(&env).locks_dir().exists());
}

#[test]
fn version_on_a_migrated_home_reports_the_same_schema() {
    let env = TestEnv::new();
    drop(open(&env));
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["schema"], VERSION);
    assert_eq!(out["store_schema"], VERSION);
}

#[test]
fn version_on_a_newer_database_succeeds_and_does_not_change_it() {
    let env = TestEnv::new();
    drop(open(&env));
    raw(&env).pragma_update(None, "user_version", VERSION + 1).unwrap();
    let before = fs::read(home(&env).db()).unwrap();
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["schema"], VERSION);
    assert_eq!(out["store_schema"], VERSION + 1);
    assert_eq!(fs::read(home(&env).db()).unwrap(), before);
}
