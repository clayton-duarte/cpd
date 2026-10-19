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
use cpd_core::store::{APPLICATION_ID, HISTORY_TABLES, Store, VERSION, WriteTxn};
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

/// The ```sql blocks of contract §4.4, in order.
fn contract_ddl_blocks() -> Vec<String> {
    let text = contract();
    let start = text.find("### 4.4 DDL").unwrap();
    let end = text.find("### 4.5 ").unwrap();
    let section = &text[start..end];
    section.split("```sql\n").skip(1).map(|b| b.split("```").next().unwrap().to_owned()).collect()
}

/// The SQL without its `--` comments and with its whitespace collapsed: comments are
/// documentation, not schema (a migration cannot change one without rebuilding the table).
fn normalized(sql: &str) -> String {
    let code: Vec<&str> = sql.lines().map(|line| line.split("--").next().unwrap_or_default()).collect();
    code.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
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
    assert_eq!(blocks.len(), 8, "§4.4 changed shape");
    assert!(blocks[1].contains("CREATE TRIGGER escalations_hist_ins"), "block 2 is the generated example");
    // The contract's DDL, every block but the generated example, run as written.
    let contract = Connection::open_in_memory().unwrap();
    for (i, block) in blocks.iter().enumerate() {
        if i != 1 {
            contract.execute_batch(block).unwrap_or_else(|e| panic!("block {}: {e}", i + 1));
        }
    }
    let expected = schema_sql(&contract);
    assert!(expected.len() > 40, "{expected:?}");
    let env = TestEnv::new();
    drop(open(&env));
    let found: std::collections::BTreeMap<String, String> = schema_sql(&raw(&env)).into_iter().collect();
    for (name, sql) in expected {
        assert_eq!(found.get(&name), Some(&sql), "{name}");
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
            expected.insert(name.to_string());
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
    assert_eq!(tables.len(), 17);
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
    let example = &contract_ddl_blocks()[1];
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
    let hash = store.read(|c| Ok(cpd_core::store::schema_hash(c)?)).unwrap();
    assert_eq!(hash, "7da9b792cadf9d12a54eb26ad99a78c48fdcbb2e4a220f767973a7760cd16c9a", "version 13 (the PR poll's Inbox kinds, migration 0013) on top of version 12 (the Inbox, migration 0012)");

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
    store.write("test", |tx| Ok(insert(tx, "session-message")?)).unwrap(); // migration 9
    let err = store.write("test", |tx| Ok(insert(tx, "plan-exploded")?)).unwrap_err();
    assert!(format!("{err:?}").contains("CHECK"), "{err:?}");
}

#[test]
fn a_fresh_database_holds_one_live_manager_per_context_and_a_brief_with_its_plan() {
    let env = TestEnv::new();
    let mut store = fixture_store(&env);
    let columns: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM pragma_table_info('contexts') WHERE name IN ('lead_session', 'grounded_seq')", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(columns, 0, "the grounded Lead is retired (migration 0010)");

    let manager = |id: &str, context: &str, plan: &str| {
        format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'manager', '{context}', {plan}, 'm', '/', 'O5', 'stopped', 't', 't')"
        )
    };
    store.write("test", |tx| exec(tx, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c2', 'C2', 't', 'm')")).unwrap();
    store.write("test", |tx| exec(tx, &manager("mgr1", "ctx", "NULL"))).unwrap();
    let second = refused(&mut store, &manager("mgr2", "ctx", "NULL"));
    assert!(sqlite_message(&second).contains("UNIQUE constraint failed"), "{second:?}");
    let with_plan = refused(&mut store, &manager("mgr3", "c2", "'pay'"));
    assert!(sqlite_message(&with_plan).contains("CHECK constraint failed"), "a Manager has no Plan: {with_plan:?}");
    // Another Context has its own; an archived Manager makes room for a new one.
    store.write("test", |tx| exec(tx, &manager("mgr4", "c2", "NULL"))).unwrap();
    store.write("test", |tx| exec(tx, "UPDATE sessions SET archived_at = 't' WHERE id = 'mgr1'")).unwrap();
    store.write("test", |tx| exec(tx, &manager("mgr5", "ctx", "NULL"))).unwrap();

    let brief = |plan: &str| {
        format!("INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine) VALUES ('ctx', 'm-b1', 'brief', 't', 'm', 'B', {plan})")
    };
    let bare = refused(&mut store, &brief("NULL, NULL"));
    assert!(sqlite_message(&bare).contains("CHECK constraint failed"), "a brief needs its Plan: {bare:?}");
    store.write("test", |tx| exec(tx, &brief("'pay', 'm'"))).unwrap();
}

#[test]
fn a_newer_schema_is_store_too_new() {
    let env = TestEnv::new();
    drop(open(&env));
    raw(&env).pragma_update(None, "user_version", VERSION + 1).unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["version"], VERSION + 1);
    assert_eq!(err.details()["supported"], VERSION);
}

#[test]
fn a_failed_migration_rolls_back_entirely() {
    let env = TestEnv::new();
    // A database whose migration 1 cannot apply: a table `history` already exists.
    raw(&env).execute_batch("CREATE TABLE history (x)").unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["to"], 1);
    let conn = raw(&env);
    let tables = names(&conn, "SELECT name FROM sqlite_schema");
    assert_eq!(tables, BTreeSet::from(["history".to_owned()]));
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, 0);
    let app: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0)).unwrap();
    assert_eq!(app, 0);
}

#[test]
fn a_migration_whose_schema_differs_from_the_expected_hash_rolls_back() {
    let env = TestEnv::new();
    // Every migration applies, but the schema it leaves is not the one the binary
    // describes: an object the migrations do not know stays in `sqlite_schema`.
    raw(&env).execute_batch("CREATE TABLE stray (x)").unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["version"], VERSION);
    let expected = err.details()["expected"].as_str().unwrap();
    let actual = err.details()["actual"].as_str().unwrap();
    assert_ne!(expected, actual);
    let conn = raw(&env);
    assert_eq!(names(&conn, "SELECT name FROM sqlite_schema"), BTreeSet::from(["stray".to_owned()]));
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, 0, "the store stays at its old version");
    let app: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0)).unwrap();
    assert_eq!(app, 0);
    // Rerunning is safe: the same refusal, nothing written.
    let again = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(again.code(), ErrorCode::MigrationFailed, "{again:?}");
}

#[test]
fn a_write_after_another_connection_bumped_user_version_is_store_too_new_and_appends_nothing() {
    let env = TestEnv::new();
    let mut store = fixture_store(&env);
    let txns = |store: &mut Store| store.read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get::<_, i64>(0))?)).unwrap();
    let before = txns(&mut store);
    raw(&env).pragma_update(None, "user_version", VERSION + 1).unwrap();
    let mut ran = false;
    let err = store
        .write("test", |tx| {
            ran = true;
            exec(tx, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('late', 'Late', 't', 'm')")
        })
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
    assert_eq!(err.details()["version"], VERSION + 1);
    assert_eq!(err.details()["supported"], VERSION);
    assert!(!ran, "the closure must not run against a newer schema");
    assert_eq!(txns(&mut store), before);
    let contexts: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM contexts WHERE id = 'late'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(contexts, 0);
}

/// `(foreign_keys, legacy_alter_table)` of the store's connection.
fn pragmas(store: &mut Store) -> (i64, i64) {
    store
        .read(|c| {
            let get = |name: &str| c.pragma_query_value(None, name, |r| r.get::<_, i64>(0));
            Ok((get("foreign_keys")?, get("legacy_alter_table")?))
        })
        .unwrap()
}

#[test]
fn foreign_keys_are_on_and_legacy_alter_table_off_after_the_migrating_open() {
    let env = TestEnv::new();
    version_5_database(&env);
    let mut store = open(&env);
    assert_eq!(pragmas(&mut store), (1, 0));
}

#[test]
fn a_write_from_a_deleted_cwd_succeeds_and_records_unreadable() {
    let env = TestEnv::new();
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"hs\"\n").unwrap();
    // The shell enters a directory, deletes it and runs cpd from there.
    let out = env
        .command("sh")
        .args(["-c", "mkdir gone && cd gone && rmdir ../gone && exec \"$@\"", "sh", common::CPD])
        .args(["context", "new", "ctx", "--title", "Ctx"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout} {}", String::from_utf8_lossy(&out.stderr));
    let mut store = open(&env);
    let cwds: Vec<String> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT cwd FROM txns WHERE command = 'context new'")?;
            Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    assert_eq!(cwds, ["(unreadable)"]);
}

// ---------- §4.5 migration 6: pull_requests and prs.pull_request ----------

/// The `CREATE TABLE prs (...) STRICT;` of versions 4 and 5, from `0004.sql` (migration 5 changes no table).
fn prs_at_version_4() -> String {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/src/store/migrations/0004.sql")).unwrap();
    let start = text.find("CREATE TABLE prs (").unwrap();
    let end = start + text[start..].find(") STRICT;").unwrap() + ") STRICT;".len();
    text[start..end].to_owned()
}

/// A database at version 5: a fresh one with `pull_requests` gone and `prs` as migration 4 made it, holding
/// - `op1`: its PR open, `pr_title` and the review and the checks polled;
/// - `op2`: its PR open, never polled (`pr_title` null);
/// - `mg`: its PR merged;
/// - `wt`: its PR waiting.
fn version_5_database(env: &TestEnv) {
    drop(open(env));
    let conn = raw(env);
    // Migration 7 drops it: a database of version 5 still has `contexts.sync_failing`.
    conn.execute_batch("ALTER TABLE contexts ADD COLUMN sync_failing INTEGER NOT NULL DEFAULT 0 CHECK (sync_failing IN (0, 1))")
        .unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64)).unwrap();
    let pr = |plan: &str, key: &str, status: &str, more: &str, values: &str| {
        let (worktree, started) = if status == "waiting" { ("NULL", "NULL") } else { ("'/wt'", "'2026-01-02T00:00:00.000Z'") };
        format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{plan}', 'ctx', 'Plan {plan}', '{phase}', 'l-{plan}', 't', {locked}, {done});
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-{plan}', 'lead', 'ctx', '{plan}', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at {more})
               VALUES ('{plan}', '{key}', '/r/{key}', 'main', '{status}', 'cpd/{plan}/{key}', {worktree}, {started}, 1, 't' {values});",
            phase = match status {
                "waiting" => "plan",
                "merged" => "done",
                _ => "deploy",
            },
            locked = if status == "waiting" { "NULL" } else { "'2026-01-02T00:00:00.000Z'" },
            done = if status == "merged" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" },
        )
    };
    conn.execute_batch(&format!(
        "DROP TABLE inbox_items;
         DROP TABLE pull_requests;
         DROP TABLE prs;
         {}
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         {}{}{}{}
         PRAGMA user_version = 5;",
        prs_at_version_4(),
        pr(
            "op1",
            "api",
            "open",
            ", pr_number, pr_url, pr_opened_at, pr_polled_at, pr_title, pr_review_decision, pr_checks",
            ", 7, 'https://github.com/acme/api/pull/7', '2026-01-05T00:00:00.000Z', '2026-01-06T00:00:00.000Z', 'Add api', 'approved', 'passed'",
        ),
        pr(
            "op2",
            "web",
            "open",
            ", pr_number, pr_url, pr_opened_at",
            ", 8, 'https://github.com/acme/web/pull/8', '2026-01-05T00:00:01.000Z'",
        ),
        pr(
            "mg",
            "docs",
            "merged",
            ", pr_number, pr_url, pr_opened_at, merged_at",
            ", 9, 'https://github.com/acme/docs/pull/9', '2026-01-05T00:00:02.000Z', '2026-01-08T00:00:00.000Z'",
        ),
        pr("wt", "cli", "waiting", "", ""),
    ))
    .unwrap();
    assert_eq!(names(&conn, "SELECT name FROM pragma_table_info('prs') WHERE name LIKE 'pr\\_%' ESCAPE '\\'").len(), 8);
}

#[test]
fn a_version_5_database_migrates_to_6_with_the_back_filled_rows_and_the_link() {
    let env = TestEnv::new();
    version_5_database(&env);
    let store = open(&env);
    // Its schema is exactly that of a fresh database (migrations 6 and 7 ran).
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    drop(store);
    let conn = raw(&env);
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, VERSION);
    let violations: i64 = conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
    assert_eq!(violations, 0);

    // One row per PR with a number, in Plan and key order; the title is the polled one, else the Plan's.
    let row = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
        Ok(json!([r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?,
                  r.get::<_, i64>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?, r.get::<_, String>(9)?,
                  r.get::<_, Option<String>>(10)?, r.get::<_, String>(11)?]))
    };
    let rows: Vec<Value> = conn
        .prepare("SELECT id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at FROM pull_requests ORDER BY id")
        .unwrap()
        .query_map([], row)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        rows,
        vec![
            json!([1, "acme/docs", 9, "https://github.com/acme/docs/pull/9", "Plan mg", 0, "merged", "none", "none",
                   "2026-01-05T00:00:02.000Z", "2026-01-08T00:00:00.000Z", "2026-01-05T00:00:02.000Z"]),
            json!([2, "acme/api", 7, "https://github.com/acme/api/pull/7", "Add api", 0, "open", "approved", "passed",
                   "2026-01-05T00:00:00.000Z", null, "2026-01-05T00:00:00.000Z"]),
            json!([3, "acme/web", 8, "https://github.com/acme/web/pull/8", "Plan op2", 0, "open", "none", "none",
                   "2026-01-05T00:00:01.000Z", null, "2026-01-05T00:00:01.000Z"]),
        ]
    );

    // `pull_request` is set exactly for the open and the merged PR; every other column is kept.
    let prs: Vec<(String, String, Option<i64>, Option<i64>, Option<String>)> = conn
        .prepare("SELECT plan, status, pull_request, pr_number, merged_at FROM prs ORDER BY plan")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let some = |s: &str| Some(s.to_owned());
    assert_eq!(
        prs,
        vec![
            ("mg".into(), "merged".into(), Some(1), Some(9), some("2026-01-08T00:00:00.000Z")),
            ("op1".into(), "open".into(), Some(2), Some(7), None),
            ("op2".into(), "open".into(), Some(3), Some(8), None),
            ("wt".into(), "waiting".into(), None, None, None),
        ]
    );
    let columns = names(&conn, "SELECT name FROM pragma_table_info('prs')");
    for gone in ["pr_seen", "pr_polled_at", "pr_title", "pr_review_decision", "pr_checks"] {
        assert!(!columns.contains(gone), "{gone}");
    }
    for kept in ["pr_number", "pr_url", "pr_opened_at", "pull_request"] {
        assert!(columns.contains(kept), "{kept}");
    }

    // The history triggers of both tables exist; the back-fill wrote an `insert` row for each pull request.
    let triggers = names(&conn, "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('pull_requests', 'prs')");
    for trigger in ["pull_requests_hist_ins", "pull_requests_hist_upd", "pull_requests_hist_del", "pull_requests_owned", "prs_hist_ins", "prs_hist_upd"] {
        assert!(triggers.contains(trigger), "{trigger}: {triggers:?}");
    }
    assert!(!triggers.contains("pull_requests_no_delete"));
    let inserted: Vec<String> = conn
        .prepare("SELECT pk FROM history WHERE tbl = 'pull_requests' AND op = 'insert' ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(inserted, ["[1]", "[2]", "[3]"]);
}

#[test]
fn an_owned_pull_request_cannot_be_deleted_and_an_unowned_one_can_with_a_history_row() {
    let env = TestEnv::new();
    version_5_database(&env);
    let mut store = open(&env);
    // Owned by the open PR of `op1`.
    let err = refused(&mut store, "DELETE FROM pull_requests WHERE number = 7");
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert!(sqlite_message(&err).starts_with("append_only"), "{err:?}");
    // Owned by the merged PR of `mg`.
    assert!(sqlite_message(&refused(&mut store, "DELETE FROM pull_requests WHERE number = 9")).starts_with("append_only"));
    // One no PR points to: a pull request of the User's that no Plan owns.
    store
        .write("test", |tx| {
            exec(
                tx,
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                 VALUES ('acme/other', 3, 'https://github.com/acme/other/pull/3', 'Other', 1, 'open', 'none', 'pending', 't', 't')",
            )
        })
        .unwrap();
    let id: i64 = store.read(|c| Ok(c.query_row("SELECT id FROM pull_requests WHERE number = 3", [], |r| r.get(0))?)).unwrap();
    store.write("test", |tx| exec(tx, "DELETE FROM pull_requests WHERE number = 3")).unwrap();
    let (op, pk, before, after): (String, String, String, Option<String>) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT op, pk, before, after FROM history WHERE tbl = 'pull_requests' ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!((op.as_str(), pk, after), ("delete", format!("[{id}]"), None));
    let before: Value = serde_json::from_str(&before).unwrap();
    assert_eq!((&before["repo"], &before["number"], &before["draft"], &before["checks"]), (&json!("acme/other"), &json!(3), &json!(1), &json!("pending")));
    let left: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM pull_requests", [], |r| r.get(0))?)).unwrap();
    assert_eq!(left, 3);
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
    assert_eq!(out["protocol"], 3, "{out}");
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
