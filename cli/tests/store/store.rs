//! The SQLite store (contract §4.1–§4.5): open, the DDL, its constraints and
//! triggers, writes only through cpd, migrations and two writers.

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::home::{HasCpdHome, Home};
use crate::scripted::ScriptedClock;
use common::TestEnv;
use cpd_core::clock::ManualClock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::store::{APPLICATION_ID, ORPHAN_HISTORY_SQL, Persist, SCHEMA_HASH, Store, VERSION, WriteTxn, canonical_hash, is_transient};
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;
use serde_json::{Value, json};

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn home(env: &impl HasCpdHome) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn open(env: &impl HasCpdHome) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

/// A connection that is not cpd's, like the `sqlite3` shell: no `cpd_txn()`.
fn raw(env: &impl HasCpdHome) -> Connection {
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
fn fixture_store(env: &impl HasCpdHome) -> Store {
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
    let env = Home::empty();
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

/// §4.1: every connection cpd opens syncs with NORMAL (1) in WAL mode; synchronous is per connection.
#[test]
fn every_connection_is_synchronous_normal_in_wal_mode() {
    let env = Home::new();
    let mut store = open(&env);
    let (sync, mode): (i64, String) = store
        .read(|conn| {
            Ok((
                conn.pragma_query_value(None, "synchronous", |r| r.get(0)).unwrap(),
                conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap(),
            ))
        })
        .unwrap();
    assert_eq!(sync, 1);
    assert_eq!(mode, "wal");
}

#[test]
fn cpd_home_is_created_on_first_use() {
    let env = Home::new();
    let nested = env.root().join("a/b/home");
    let store = Store::open(&CpdHome::resolve(Some(nested.clone().into()), None).unwrap(), 5000).unwrap();
    assert_eq!(store.path(), nested.join("cpd.db"));
    assert!(nested.join("cpd.db").is_file());
}

#[test]
fn a_file_that_is_not_a_database_is_store_failed() {
    let env = Home::new();
    fs::write(home(&env).db(), "this is not sqlite, not at all, and long enough to have a header").unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["path"], home(&env).db().display().to_string());
    assert!(err.details()["sqlite_code"].is_i64(), "{err:?}");
}

#[test]
fn a_foreign_application_id_is_refused_and_never_written() {
    let env = Home::empty();
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

/// `(table, no_delete)` of every row of `history_tables`: the tables whose history triggers are generated (§4.4).
fn history_tables(env: &impl HasCpdHome) -> Vec<(String, bool)> {
    let conn = raw(env);
    let mut stmt = conn.prepare("SELECT name, no_delete FROM history_tables ORDER BY name").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1))).unwrap().map(Result::unwrap).collect()
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
    // `ALTER TABLE … ADD COLUMN` splices its column in as `, <column>)`: the spaces before `,` and `)` are layout too.
    code.join(" ").split_whitespace().collect::<Vec<_>>().join(" ").replace(" ,", ",").replace(" )", ")")
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
    assert_eq!(blocks.len(), 10, "§4.4 changed shape"); // `delete-tombstone` (hs-d527) added the block of `plans_deleted_once`
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
    let env = Home::new();
    drop(open(&env));
    let found: std::collections::BTreeMap<String, String> = schema_sql(&raw(&env)).into_iter().collect();
    for (name, sql) in expected {
        assert_eq!(found.get(&name), Some(&sql), "{name}");
    }
}

#[test]
fn every_table_index_and_trigger_of_4_4_exists() {
    let env = Home::new();
    drop(open(&env));
    let mut expected = BTreeSet::new();
    for block in contract_ddl_blocks() {
        for line in block.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let name = match words.as_slice() {
                ["CREATE", "TABLE" | "INDEX" | "TRIGGER" | "VIEW", name, ..] => name,
                ["CREATE", "UNIQUE", "INDEX", name, ..] => name,
                _ => continue,
            };
            expected.insert(name.to_string());
        }
    }
    for (table, no_delete) in history_tables(&env) {
        expected.insert(format!("{table}_hist_ins"));
        expected.insert(format!("{table}_hist_upd"));
        // `delete-tombstone` (hs-d527): a generated `_hist_del` for every table that refuses a delete.
        if no_delete {
            expected.insert(format!("{table}_hist_del"));
        }
        // `pull_requests` has no `_no_delete`: an unowned row may be deleted (`pull_requests_owned` is in the DDL).
        // `dependencies` and `pr_needs` write their own (in the DDL).
        if no_delete || ["dependencies", "pr_needs"].contains(&table.as_str()) {
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
    assert_eq!(tables.len(), 30, "§4.4's tables, the three lookup tables and schema_migrations");
    for table in tables {
        let strict: i64 = raw(&env)
            .query_row("SELECT strict FROM pragma_table_list WHERE name = ?1", [&table], |r| r.get(0))
            .unwrap();
        assert_eq!(strict, 1, "{table}");
    }
}

#[test]
fn generated_history_triggers_match_the_contracts_example() {
    let env = Home::new();
    drop(open(&env));
    let conn = raw(&env);
    let example = &contract_ddl_blocks()[2];
    for statement in example.split_inclusive("END;") {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        let name = statement.split_whitespace().nth(2).unwrap();
        let mut sql: String =
            conn.query_row("SELECT sql FROM sqlite_schema WHERE name = ?1", [name], |r| r.get(0)).unwrap();
        if name == "escalations_hist_upd" {
            // The example abbreviates the changed-column lines of the columns its comments name.
            let columns = names(&conn, "SELECT name FROM pragma_table_info('escalations')");
            let omitted: Vec<&String> = columns.iter().filter(|c| !normalized(statement).contains(&format!("'{c}'"))).collect();
            assert!(!omitted.is_empty(), "the example abbreviates");
            for column in &omitted {
                assert!(statement.contains(&format!(" {column},")) || statement.contains(&format!(" {column}\n")), "the comment names {column}");
            }
            sql = sql
                .lines()
                .filter(|line| !omitted.iter().any(|c| line.trim_start().starts_with(&format!("UNION ALL SELECT '{c}',"))))
                .collect::<Vec<_>>()
                .join("\n");
        }
        assert_eq!(normalized(&format!("{sql};")), normalized(statement), "{name}");
    }
}

#[test]
fn every_column_of_every_table_appears_in_its_history_triggers() {
    let env = Home::new();
    drop(open(&env));
    let conn = raw(&env);
    let sql_of = |name: &str| -> String {
        conn.query_row("SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1", [name], |r| r.get(0))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    for (table, _) in history_tables(&env) {
        let table = table.as_str();
        let columns = names(&conn, &format!("SELECT name FROM pragma_table_info('{table}')"));
        assert!(!columns.is_empty(), "{table}");
        let ins = sql_of(&format!("{table}_hist_ins"));
        let upd = sql_of(&format!("{table}_hist_upd"));
        let key = names(&conn, &format!("SELECT name FROM pragma_table_info('{table}') WHERE pk > 0"));
        for column in &columns {
            assert!(ins.contains(&format!("'{column}', NEW.{column}")), "{table}_hist_ins lacks {column}");
            // §4.4 "History row shape": the key always, every other column when it changed.
            for row in ["OLD", "NEW"] {
                if key.contains(column) {
                    let forms = [
                        format!("SELECT '{column}' AS k, {row}.{column} AS v"),
                        format!("SELECT '{column}', {row}.{column}\n"),
                        format!("SELECT '{column}', {row}.{column} UNION ALL"),
                    ];
                    assert!(forms.iter().any(|f| upd.contains(f)), "{table}_hist_upd lacks the key {row}.{column}");
                } else {
                    let line = format!("UNION ALL SELECT '{column}', {row}.{column} WHERE OLD.{column} IS NOT NEW.{column}\n");
                    assert!(upd.contains(&line), "{table}_hist_upd lacks {line:?}");
                }
            }
        }
        if ["dependencies", "pr_needs", "pull_requests"].contains(&table) {
            let del = sql_of(&format!("{table}_hist_del"));
            // `pull_requests_hist_del` is hand-written and owned by its migrations (§4.5 K4): `pr-comments` and `pr-base` (pro-d118) are additive, so they
            // cannot rewrite it (two additive migrations that did would not commute) and its columns are not in it.
            let added_by_additive = ["updated_at", "feedback", "base_ref", "default_branch"];
            for column in columns.iter().filter(|c| table != "pull_requests" || !added_by_additive.contains(&c.as_str())) {
                assert!(del.contains(&format!("'{column}', OLD.{column}")), "{table}_hist_del lacks {column}");
            }
        }
    }
}

#[test]
fn writes_are_copied_into_history_with_their_txn() {
    let env = Home::new();
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
    // §4.4 "History row shape": the key and the changed columns, a change from NULL included.
    assert_eq!(before, json!({"plan": "pay", "id": "api-a1-q1", "status": "open", "answer": null, "answered_at": null}));
    assert_eq!(after, json!({"plan": "pay", "id": "api-a1-q1", "status": "answered", "answer": "Because.", "answered_at": "t"}));
}

/// sql-F2 (hs-d234): a Job status flip used to store the whole row twice, its long text included.
#[test]
fn a_job_status_change_writes_only_the_key_and_the_changed_columns_to_history() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    // A Job with the size of a real one (7 KB of text on average, quality-sql-perf.md).
    let long = "x".repeat(3500);
    let seq: i64 = store
        .write("test", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'pay', 'big', 'Big', 'S2', '{long}', '{long}', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, '{FP}', cpd_txn(), 't')"
                ),
            )?;
            Ok(tx.query_row("SELECT seq FROM jobs WHERE key = 'big'", [], |r| r.get(0))?)
        })
        .unwrap();
    let last = |store: &mut Store| -> (Value, Value, i64) {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT before, after, length(before) + length(after) FROM history WHERE tbl = 'jobs' AND op = 'update' ORDER BY id DESC LIMIT 1",
                    [],
                    |r| {
                        let text = |i| r.get::<_, String>(i).map(|s| serde_json::from_str::<Value>(&s).unwrap());
                        Ok((text(0)?, text(1)?, r.get(2)?))
                    },
                )?)
            })
            .unwrap()
    };
    store.write("test", |tx| exec(tx, "UPDATE jobs SET status = 'running' WHERE key = 'big'")).unwrap();
    let (before, after, bytes) = last(&mut store);
    assert_eq!((before, after), (json!({"seq": seq, "status": "queued"}), json!({"seq": seq, "status": "running"})), "{bytes} B");
    eprintln!("history bytes of a Job status change (before + after): {bytes}");
    assert!(bytes < 100, "{bytes} B");
    // A change to a value from NULL is a change.
    store.write("test", |tx| exec(tx, "UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'big'")).unwrap();
    let (before, after, _) = last(&mut store);
    assert_eq!(before, json!({"seq": seq, "status": "running", "failed_cause": null}));
    assert_eq!(after, json!({"seq": seq, "status": "failed", "failed_cause": "crashed"}));
    // An UPDATE that changes nothing still writes one row, the key only.
    let count = |store: &mut Store| -> i64 {
        store.read(|c| Ok(c.query_row("SELECT count(*) FROM history WHERE tbl = 'jobs'", [], |r| r.get(0))?)).unwrap()
    };
    let rows = count(&mut store);
    store.write("test", |tx| exec(tx, "UPDATE jobs SET status = status WHERE key = 'big'")).unwrap();
    assert_eq!(count(&mut store), rows + 1);
    assert_eq!(last(&mut store), (json!({"seq": seq}), json!({"seq": seq}), 2 * json!({"seq": seq}).to_string().len() as i64));
}

#[test]
fn check_constraints_reject_a_bad_value_in_every_constrained_table() {
    let env = Home::new();
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
        // A kind is a foreign key to its lookup table (§4.5 K1); every other bad value is a CHECK.
        let expected = if ["events", "context_records"].contains(&table) { "FOREIGN KEY constraint failed" } else { "CHECK constraint failed" };
        assert!(sqlite_message(&err).contains(expected), "{table}: {err:?}");
    }
    // The same rows with the bad value fixed are accepted, so the CHECK (or the foreign key) was the cause.
    store
        .write("test", |tx| {
            exec(tx, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('xy', 'T', 't', 'm');
                      INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1', 'O', 'm', '[]', 't');
                      INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', 'job-added', 'ctx', 'x', '{}');")
        })
        .unwrap();
}

/// Contract §4.4 `pr_needs` (migration `local-needs`): a need names a PR of the Plan by key; one naming no PR is refused.
#[test]
fn a_need_names_a_pr_of_its_plan_and_one_naming_no_pr_is_refused() {
    let env = Home::new();
    let mut store = open(&env);
    let txn = fixture(&mut store);
    store
        .write("test", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                       VALUES ('pay', 'deploy', '/r', 'main', 'waiting', 'cpd/pay/deploy', {txn}, 't');
                     INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', 'deploy', 'pay');"
                ),
            )
        })
        .unwrap();
    let stored: (String, String) =
        store.read(|c| Ok(c.query_row("SELECT pr, needs FROM pr_needs WHERE plan = 'pay'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!(stored, ("deploy".to_owned(), "pay".to_owned()));
    let err = refused(&mut store, "INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', 'pay', 'nowhere')");
    assert!(sqlite_message(&err).contains("FOREIGN KEY constraint failed"), "{err:?}");
}

#[test]
fn transitions_outside_section_3_are_illegal_transition() {
    let env = Home::new();
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
    let env = Home::new();
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
fn a_stub_draft_has_a_title_only_and_a_job_that_is_not_a_draft_has_its_tier_and_its_spec() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    let insert = |key: &str, status: &str, fingerprint: &str| {
        format!(
            "INSERT INTO jobs (plan, pr, key, title, status, appended, fingerprint, created_txn, created_at)
             VALUES ('pay', 'pay', '{key}', 'T', '{status}', 1, {fingerprint}, cpd_txn(), 't')"
        )
    };
    // A stub draft is stored, and may be cancelled as such.
    store.write("test", |tx| exec(tx, &insert("stub", "draft", "NULL"))).unwrap();
    store
        .write("test", |tx| exec(tx, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'stub'"))
        .unwrap();
    // No other status is, and a draft cannot be queued while it lacks its Tier or Spec.
    store.write("test", |tx| exec(tx, &insert("stub2", "draft", "NULL"))).unwrap();
    for sql in [insert("direct", "queued", &format!("'{FP}'")), format!("UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 'stub2'")] {
        let err = refused(&mut store, &sql);
        assert!(sqlite_message(&err).contains("CHECK constraint failed"), "{sql}: {err:?}");
    }
    store
        .write("test", |tx| {
            exec(
                tx,
                &format!(
                    "UPDATE jobs SET tier = 'S2', goal = 'g', context = 'c', touches = '[\"a\"]', acceptance = '[\"b\"]', out_of_scope = '[]' WHERE key = 'stub2';
                     UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 'stub2'"
                ),
            )
        })
        .unwrap();
}

#[test]
fn appending_a_job_inserts_its_dependencies_in_its_own_transaction() {
    // dependencies_locked: a non-draft Job created in this transaction (append) may get Dependencies.
    let env = Home::new();
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
    let env = Home::new();
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
    let env = Home::new();
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
    let env = Home::new();
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
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    let err = store
        .read(|c| {
            c.execute("UPDATE contexts SET title = 'Changed'", [])?;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::StoreFailed);
    assert!(sqlite_message(&err).contains("readonly"), "{err:?}");
}

/// store-G3: the statements that fire no trigger are stopped too.
#[test]
fn a_read_transaction_cannot_run_ddl_or_make_a_temp_table() {
    let env = Home::new();
    let mut store = open(&env);
    fixture(&mut store);
    for sql in ["CREATE TEMP TABLE scratch (x)", "CREATE TABLE scratch (x)", "DROP TABLE tier_catalog"] {
        let err = store.read(|c| Ok(c.execute_batch(sql)?)).expect_err(sql);
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{sql}: {err:?}");
        assert!(sqlite_message(&err).contains("readonly"), "{sql}: {err:?}");
    }
    // The guard is off again for the next write, and the schema is untouched.
    store.write("test", |tx| exec(tx, "UPDATE contexts SET title = 'Changed'")).unwrap();
    assert!(store.schema_mismatch().is_none());
    let tables: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM sqlite_temp_schema", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(tables, 0);
}

#[test]
fn every_write_transaction_has_its_txns_row() {
    let env = Home::new();
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
    let env = Home::new();
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
    let env = Home::new();
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
    let env = Home::new();
    let mut store = open(&env);
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
    let conn = raw(&env);
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(999i64)).unwrap();
    conn.execute_batch("INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (999, 'contexts', '[\"x\"]', 'insert', NULL, '{}')")
        .unwrap();
    assert_eq!(store.tamper().unwrap(), vec![json!({"kind": "history", "id": 1, "txn": 999})]);
}

#[test]
fn the_tamper_report_lists_every_orphan_among_many_history_rows_in_id_order() {
    let env = Home::new();
    let mut store = open(&env);
    let conn = raw(&env);
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    let current = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
    let seen = current.clone();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(seen.load(std::sync::atomic::Ordering::SeqCst)))
        .unwrap();
    let real: i64 = conn.query_row("SELECT min(id) FROM txns", [], |r| r.get(0)).unwrap();
    for n in 0..30i64 {
        let txn = if n % 7 == 3 { 900 + n } else { real };
        current.store(txn, std::sync::atomic::Ordering::SeqCst);
        conn.execute_batch(&format!(
            "INSERT INTO history (txn, tbl, pk, op, before, after) VALUES ({txn}, 'contexts', '[\"x\"]', 'insert', NULL, '{{}}')"
        ))
        .unwrap();
    }
    let report = store.tamper().unwrap();
    let orphans: Vec<i64> = report.iter().map(|e| e["txn"].as_i64().unwrap()).collect();
    assert_eq!(orphans, vec![903, 910, 917, 924]);
    let ids: Vec<i64> = report.iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    assert!(report.iter().all(|e| e["kind"] == "history"));
}

#[test]
fn the_tamper_query_reads_history_through_the_history_txn_index_not_the_table() {
    let env = Home::new();
    let _store = open(&env);
    let conn = raw(&env);
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {ORPHAN_HISTORY_SQL}")).unwrap();
    let details: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert!(
        details.iter().any(|d| d.starts_with("SCAN h") && d.contains("COVERING INDEX history_txn")),
        "history must be scanned through history_txn: {details:?}"
    );
    assert!(!details.iter().any(|d| d == "SCAN h"), "history must not be scanned as a table: {details:?}");
}

// ---------- §4.4 the read indexes (migration `read-indexes`: sql-F4, tui-F3, engine-F2; sql-F6 refused) ----------

/// The detail column of `EXPLAIN QUERY PLAN <sql>` on a fresh store, one line per step.
fn query_plan(sql: &str) -> Vec<String> {
    let env = Home::new();
    let _store = open(&env);
    let conn = raw(&env);
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    stmt.query_map([], |r| r.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

/// The statement under test is read by `index`, not by a table scan.
#[track_caller]
fn assert_uses(sql: &str, index: &str) {
    let plan = query_plan(sql);
    assert!(plan.iter().any(|step| step.contains(&format!("USING INDEX {index}"))), "{index} unused by {sql}: {plan:?}");
}

/// sql-F4, tui-F3: the job counts of `plan_heads` (`tui/src/read/shared.rs`), run by every cockpit
/// refresh (tree, widgets, statusbar, plan flow), scanned `jobs` with its overflow pages.
#[test]
fn the_cockpits_job_counts_are_read_through_jobs_plan_status() {
    assert_uses(
        "SELECT j.plan, j.status, count(*),
                j.status = 'running' AND EXISTS (SELECT 1 FROM attempts a
                  WHERE a.plan = j.plan AND a.job = j.key AND a.status = 'merging')
           FROM jobs j WHERE NULL IS NULL OR j.plan = NULL GROUP BY j.plan, j.status, 4",
        "jobs_plan_status",
    );
}

/// sql-F6 asked for `events(kind, id)`. It is not in the schema, because SQLite (no statistics) then reads
/// every Event of the kind for the statements that test a kind and a Session or a Plan, instead of
/// `events_session` or `events_plan`: the Session SELECT of `session list` and the tree went from 6.7 to 797 ms
/// on a copy of the live store, the inbox rules and E2's active time got 4 to 9 times slower. So these
/// statements must keep the selective indexes.
#[test]
fn the_lookups_of_events_by_kind_and_session_or_plan_keep_their_selective_index() {
    // `core/src/session/row.rs`, `tui/src/read/shared.rs`, `core/src/inbox/rules.rs` (per Session).
    assert_uses(
        "SELECT json_extract(e.data, '$.place') FROM events e WHERE e.session = 'x' AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1",
        "events_session",
    );
    assert_uses("SELECT max(e.id) FROM events e WHERE e.session = 'x' AND e.kind = 'session-state'", "events_session");
    // `core/src/watch/liveness.rs` (E2, per Attempt per tick), `core/src/job/retry.rs`.
    assert_uses(
        "SELECT at FROM events WHERE plan = 'p' AND attempt = 'a' AND kind = 'attempt-status' AND json_extract(data, '$.to') = 'running' ORDER BY id LIMIT 1",
        "events_plan",
    );
    assert_uses("SELECT max(id) FROM events WHERE plan = 'p' AND job = 'j' AND kind = 'job-retried'", "events_plan");
    // `events follow --session --kind` (`core/src/events/select.rs`).
    assert_uses("SELECT id FROM events WHERE id > 0 AND session = 'x' AND kind IN ('session-state') ORDER BY id LIMIT 500", "events_session");
}

/// engine-F2: E2's Session read (`core/src/watch/liveness.rs`, unit-tested there against its own
/// statement) must not read every Session ever made. The index lists only the Sessions E2 can
/// still see as live, so a `stopped` Session with no pane costs nothing.
#[test]
fn only_the_live_sessions_are_in_sessions_live() {
    let env = Home::new();
    let mut store = fixture_store(&env);
    store
        .write("test", |tx| {
            exec(
                tx,
                "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, tmux_pane)
                   VALUES ('gone', 'lead', 'ctx', 'Gone', '/', 'O3', 'stopped', 't', 't', NULL),
                          ('parked', 'lead', 'ctx', 'Parked', '/', 'O3', 'stopped', 't', 't', '%7');",
            )
        })
        .unwrap();
    let conn = raw(&env);
    let sql = "SELECT id FROM sessions INDEXED BY sessions_live WHERE state <> 'stopped' OR tmux_pane IS NOT NULL ORDER BY id";
    let mut stmt = conn.prepare(sql).unwrap();
    let listed: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    // `lead1` is `idle`, `parked` is `stopped` with a pane; `gone` is `stopped` with none.
    assert_eq!(listed, vec!["lead1".to_owned(), "parked".to_owned()]);
}

// ---------- §4.5 versions and migrations ----------

#[test]
fn migrating_an_empty_file_creates_the_schema_once() {
    let env = Home::empty();
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
    let env = Home::empty();
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
    store.write("test", |tx| Ok(insert(tx, "session-message")?)).unwrap(); // migration 9
    let err = store.write("test", |tx| Ok(insert(tx, "plan-exploded")?)).unwrap_err();
    assert!(format!("{err:?}").contains("FOREIGN KEY"), "an Event kind is a foreign key to event_kinds: {err:?}");
}

/// Migration `loop-guard` (contract §4.4, §5.2): a fresh store takes the Event kind `session-resumed` and the Inbox kind `loop-guard`.
#[test]
fn a_fresh_database_accepts_the_session_resumed_event_and_a_loop_guard_inbox_item() {
    let env = Home::empty();
    let mut store = fixture_store(&env);
    store
        .write("test", |tx| {
            Ok(tx.execute("INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', 'session-resumed', 'ctx', 'x', '{}')", [])?)
        })
        .unwrap_or_else(|e| panic!("session-resumed: {e:?}"));
    store
        .write("test", |tx| {
            Ok(tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, context, label, opened_at) VALUES ('loop-guard', 'red', 'session:x', 'ctx', 'pay: loop guard, 21 turns in 10 min', 't')",
                [],
            )?)
        })
        .unwrap_or_else(|e| panic!("loop-guard: {e:?}"));
}

/// The Inbox kind `drafts-parked` (contract §4.4; hs-d731, hs-d733): a fresh store takes it, magenta, and refuses another tier's
/// kind it does not know.
#[test]
fn a_fresh_database_accepts_a_drafts_parked_inbox_item() {
    let env = Home::empty();
    let mut store = fixture_store(&env);
    store
        .write("test", |tx| {
            Ok(tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, context, label, opened_at) VALUES ('drafts-parked', 'magenta', 'drafts-parked:pay', 'ctx', 'pay: 2 parked drafts', 't')",
                [],
            )?)
        })
        .unwrap_or_else(|e| panic!("drafts-parked: {e:?}"));
}

#[test]
fn a_fresh_database_holds_one_live_manager_per_context_and_a_brief_with_its_plan() {
    let env = Home::empty();
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
    let env = Home::new();
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
    let env = Home::empty();
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

#[test]
fn a_migration_whose_schema_differs_from_the_expected_hash_rolls_back() {
    let env = Home::empty();
    // Every migration applies, but the schema it leaves is not the one the binary
    // describes: an object the migrations do not know stays in `sqlite_schema`.
    raw(&env).execute_batch("CREATE TABLE stray (x)").unwrap();
    let err = Store::open(&home(&env), 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["migration"], "schema_check");
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
    let env = Home::new();
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
    assert_eq!(err.details()["user_version"], VERSION + 1);
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
    let env = Home::empty();
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
        .args(["context", "new", "ctx", "--title", "Ctx", "--owner", "acme"])
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

// ---------- §4.5 named migrations: the runner's table ----------

#[test]
fn schema_migrations_records_every_legacy_migration_and_is_written_only_by_the_runner() {
    let env = Home::empty();
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
        ("0007-context-records", "breaking"),
        ("0008-phase-deploy", "breaking"),
        ("0009-session-message", "breaking"),
        ("0010-manager", "breaking"),
        ("0011-pr-mergeable", "breaking"),
        ("0012-inbox", "breaking"),
        ("0013-pr-inbox", "breaking"),
        ("0014-register", "breaking"),
    ];
    let expected: Vec<(String, String, i64, i64)> =
        expected.iter().enumerate().map(|(i, (n, c))| (n.to_string(), c.to_string(), i as i64 + 1, 0)).collect();
    assert_eq!(rows[..expected.len()], expected, "a fresh store records its legacy migrations as not converted");
    // The named migrations follow, the first being `kind-tables`; none is converted.
    let (kind_tables, later) = rows[expected.len()..].split_first().expect("a named migration");
    assert!(kind_tables.0.ends_with("-kind-tables"), "{kind_tables:?}");
    assert_eq!((kind_tables.1.as_str(), kind_tables.2), ("breaking", expected.len() as i64 + 1));
    assert!(kind_tables.3 == 0 && later.iter().all(|r| r.3 == 0), "{rows:?}");

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

// ---------- §4.2 two writers ----------

/// The wall time in milliseconds since the epoch, as a record of the store writes it.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// The order in which the writers enter and leave their write transactions: a number from one counter, so "B entered
/// after A left" needs no clock.
static ORDER: AtomicU64 = AtomicU64::new(0);

/// A writer: a thread of this test that opens a `Store` of its own on the home and writes once, as another `cpd`
/// process would (SQLite's lock is the file's, not the process's). A holding writer keeps the write lock until the
/// test sends `release`, not for a time.
struct Writer {
    thread: Option<std::thread::JoinHandle<Value>>,
    /// Lets a holding writer finish its transaction. Dropping every sender does too, so a test that panics holds nothing.
    release: mpsc::Sender<()>,
    /// One message once the writer is inside its write transaction.
    started: mpsc::Receiver<()>,
}

impl Writer {
    /// A writer that writes at once.
    fn spawn(env: &Home, busy_ms: u64) -> Writer {
        Writer::spawn_with(env, busy_ms, false)
    }

    /// A writer that holds the write lock until [`Writer::release`].
    fn spawn_holding(env: &Home, busy_ms: u64) -> Writer {
        Writer::spawn_with(env, busy_ms, true)
    }

    fn spawn_with(env: &Home, busy_ms: u64, holds: bool) -> Writer {
        let home = home(env);
        let (release, released) = mpsc::channel();
        let (started_tx, started) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result = Store::open(&home, busy_ms).and_then(|mut store| {
                store.write("test writer", |tx| {
                    let entered = ORDER.fetch_add(1, Ordering::SeqCst);
                    let _ = started_tx.send(());
                    if holds {
                        let _ = released.recv();
                    }
                    Ok(json!({"txn": tx.id(), "entered": entered, "left": ORDER.fetch_add(1, Ordering::SeqCst)}))
                })
            });
            match result {
                Ok(v) => json!({"ok": true, "result": v}),
                Err(e) => json!({"ok": false, "error": e.to_json()}),
            }
        });
        Writer { thread: Some(thread), release, started }
    }

    /// Lets the writer finish its transaction.
    fn release(&self) {
        let _ = self.release.send(());
    }

    /// Waits until the writer is inside its write transaction.
    fn wait_started(&mut self) {
        if self.started.recv().is_err() {
            let ended = self.thread.take().expect("the writer is joined once").join().expect("the writer thread");
            panic!("the writer ended before it was inside its write transaction: {ended}");
        }
    }

    /// The writer's result: `{"ok": true, "result": {"txn", "entered", "left"}}` or `{"ok": false, "error"}`.
    fn wait(mut self) -> Value {
        self.thread.take().expect("the writer is joined once").join().expect("the writer thread")
    }
}

#[test]
fn a_second_writer_waits_for_the_first_and_reads_do_not() {
    let env = Home::new();
    let mut a = Writer::spawn_holding(&env, 5000);
    a.wait_started();
    let b = Writer::spawn(&env, 20_000);
    // While A holds the write lock (until this test lets go of it), a reader opens and reads: had it to wait for A,
    // it would wait for ever.
    let mut reader = open(&env);
    let txns: i64 = reader.read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap();
    assert_eq!(txns, 1, "A has not committed yet");
    a.release();
    let (a, b) = (a.wait(), b.wait());
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(b["ok"], true, "{b}");
    assert_eq!(a["result"]["txn"], 2);
    assert_eq!(b["result"]["txn"], 3);
    assert!(b["result"]["entered"].as_u64() >= a["result"]["left"].as_u64(), "B ran while A held the lock: {a} {b}");
}

#[test]
fn a_writer_that_cannot_get_the_lock_in_time_is_store_busy() {
    let env = Home::new();
    let mut a = Writer::spawn_holding(&env, 5000);
    a.wait_started();
    let a_pid = std::process::id();
    // B waits `busy_ms` in SQLite's own busy handler, a short real wait inside the library.
    let begun = Instant::now();
    let b = Store::open(&home(&env), 200).and_then(|mut store| store.write("test writer", |_| Ok(())));
    let waited = begun.elapsed();
    let error = b.expect_err("B cannot get the lock").to_json();
    assert_eq!(error["code"], "store_busy", "{error}");
    assert_eq!(error["details"]["busy_ms"], 200);
    // §4.2: the error names the holder, A: its pid, its command and for how long.
    let holder = &error["details"]["holder"];
    assert_eq!(holder["pid"], a_pid, "{error}");
    assert_eq!(holder["command"], "test writer", "{error}");
    assert!(holder["since_ms"].as_u64().unwrap() >= 200, "{error}");
    // A is a thread of this process: its argv is the process's.
    assert_eq!(holder["argv"].as_str().unwrap(), std::env::args().collect::<Vec<_>>().join(" "), "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains(&format!("pid {a_pid} (`test writer`")), "{message}");
    assert!(waited >= Duration::from_millis(200), "gave up after {waited:?}");
    a.release();
    assert_eq!(a.wait()["ok"], true);
    let mut store = open(&env);
    let txns: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap();
    assert_eq!(txns, 2, "B wrote nothing");
}

/// The holder's record is removed when its transaction ends, by a commit or a rollback.
#[test]
fn the_holders_record_is_gone_after_a_commit_and_after_a_rollback() {
    let env = Home::new();
    let mut store = open(&env);
    let records = || -> Vec<String> {
        let names = fs::read_dir(home(&env).locks_dir()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned());
        names.filter(|n| n.starts_with("store-writer-")).collect()
    };
    let during = store.write("test", |_| Ok(records())).unwrap();
    assert_eq!(during, [format!("store-writer-{}.json", std::process::id())]);
    assert!(records().is_empty(), "{:?}", records());
    store.write("test", |_| Err::<(), _>(CpdError::new(ErrorCode::StoreFailed, "no"))).unwrap_err();
    assert!(records().is_empty(), "{:?}", records());
}

/// A transaction that holds the lock for a second or more says where the time went, in `run/store-slow.log`.
#[test]
fn a_slow_write_is_logged_with_where_its_time_went() {
    let env = Home::new();
    let mut store = open(&env);
    // The slow work is virtual time: the store reads its clock (`use_clock`), and the closure advances it 1.1 s, past
    // the 1 s threshold.
    let clock = ManualClock::new();
    store.use_clock(Arc::new(clock.clone()));
    store.write("slow", |_| Ok(clock.advance(Duration::from_millis(1100)))).unwrap();
    let text = fs::read_to_string(env.cpd_home().join("run/store-slow.log")).unwrap();
    // Other writes of a loaded machine may be in it too: find this one.
    let line = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|l| l["command"] == "slow")
        .unwrap_or_else(|| panic!("no line for the slow write: {text}"));
    assert_eq!(line["pid"].as_u64(), Some(u64::from(std::process::id())));
    let phases = &line["phases"];
    assert!(phases["closure_ms"].as_u64().unwrap() >= 1100, "{line}");
    assert!(phases["held_ms"].as_u64().unwrap() >= phases["closure_ms"].as_u64().unwrap(), "{line}");
    assert!(phases["commit_ms"].is_u64() && phases["begin_ms"].is_u64() && phases["waited_ms"].is_u64(), "{line}");
}

/// A lock held by a connection that is not cpd's has no record: `store_busy` says the holder is unknown.
#[test]
fn a_store_busy_with_no_recorded_holder_says_so() {
    let env = Home::new();
    drop(open(&env));
    let other = raw(&env);
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut store = Store::open(&home(&env), 100).unwrap();
    let e = store.write("test", |_| Ok(())).unwrap_err();
    assert_eq!(e.code(), ErrorCode::StoreBusy, "{e:?}");
    assert!(e.details()["holder"].is_null(), "{e:?}");
    assert!(e.message().ends_with("Its holder is unknown: no process has recorded that it holds the lock."), "{e:?}");
}

#[test]
fn a_record_of_a_dead_process_is_not_the_holder() {
    let env = Home::new();
    drop(open(&env));
    // A pid no process has: above every system's `pid_max` (no child to spawn and reap).
    let dead = i32::MAX as u32;
    let record = json!({"pid": dead, "command": "gone", "argv": "gone", "session": null, "attempt": null, "since_unix_ms": now_ms()});
    fs::write(home(&env).locks_dir().join(format!("store-writer-{dead}.json")), record.to_string()).unwrap();
    let other = raw(&env);
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let e = Store::open(&home(&env), 100).unwrap().write("test", |_| Ok(())).unwrap_err();
    assert!(e.details()["holder"].is_null(), "{e:?}");
}

type Lines = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

fn persisting(lines: &Lines, give_up: fn() -> bool) -> Persist {
    persisting_then(lines, give_up, || {})
}

/// [`persisting`] that also runs `then` after each line it logs.
fn persisting_then(lines: &Lines, give_up: fn() -> bool, then: impl Fn() + 'static) -> Persist {
    let lines = lines.clone();
    Persist::new(
        move |line| {
            lines.lock().unwrap().push(line.to_owned());
            then();
        },
        give_up,
    )
}

/// §4.2: a persisting store waits out a lock held for longer than `store_busy_ms`, logs one line for
/// the error and one when it clears, and the write then completes; a plain store gives up.
#[test]
fn a_persisting_store_waits_for_a_lock_held_longer_than_store_busy_ms() {
    let env = Home::new();
    let mut a = Writer::spawn_holding(&env, 5000);
    a.wait_started();
    let a_pid = std::process::id();

    let mut plain = Store::open(&home(&env), 100).unwrap();
    assert_eq!(plain.write("test", |_| Ok(())).unwrap_err().code(), ErrorCode::StoreBusy);

    // The back-off between two tries is virtual time (the store reads and sleeps on `clock`); each try waits
    // `store_busy_ms` in SQLite's busy handler, a short real wait inside the library.
    let clock = ManualClock::new();
    let lines = Lines::default();
    // A holds the lock until the persisting store has met it and said so: the store waits past `store_busy_ms` for
    // as long as A holds it.
    let release = a.release.clone();
    let persist = persisting_then(&lines, || false, move || {
        let _ = release.send(());
    });
    let mut store = Store::open_persistent_with(Arc::new(clock.clone()), &home(&env), 100, persist).unwrap();
    let txn = store.write("persisted", |tx| Ok(tx.id())).unwrap();
    assert!(clock.elapsed() >= Duration::from_millis(100), "it did not wait past store_busy_ms: {:?}", clock.elapsed());
    assert_eq!(a.wait()["ok"], true);
    let lines = lines.lock().unwrap().clone();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("store_busy: "), "{lines:?}");
    assert!(lines[0].contains(&format!("pid {a_pid} (`test writer`")), "the line names the holder: {lines:?}");
    assert!(lines[1].starts_with("the store is clear again after "), "{lines:?}");
    let command: String = store.read(|c| Ok(c.query_row("SELECT command FROM txns WHERE id = ?1", [txn], |r| r.get(0))?)).unwrap();
    assert_eq!(command, "persisted");
}

#[test]
fn a_persisting_store_gives_up_when_told_to_and_its_reads_never_wait() {
    let env = Home::new();
    drop(open(&env));
    let other = raw(&env);
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let lines = Lines::default();
    // Told to give up at once: the error is returned and nothing is logged.
    let mut store = Store::open_persistent(&home(&env), 100, persisting(&lines, || true)).unwrap();
    assert_eq!(store.write("test", |_| Ok(())).unwrap_err().code(), ErrorCode::StoreBusy);
    assert!(lines.lock().unwrap().is_empty());
    // A reader never waits for a writer: it reads, persisting or not.
    assert_eq!(store.read(|c| Ok(c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?)).unwrap(), 1);
}

#[test]
fn only_busy_and_locked_are_transient() {
    let busy = CpdError::new(ErrorCode::StoreBusy, "busy");
    let locked = CpdError::new(ErrorCode::StoreFailed, "locked").with("sqlite_code", 6);
    let busy_snapshot = CpdError::new(ErrorCode::StoreFailed, "snapshot").with("sqlite_code", 517);
    let constraint = CpdError::new(ErrorCode::StoreFailed, "constraint").with("sqlite_code", 275);
    let other = CpdError::new(ErrorCode::StoreFailed, "other");
    let tampered = CpdError::new(ErrorCode::StoreTampered, "tampered");
    let flags = [&busy, &locked, &busy_snapshot, &constraint, &other, &tampered].map(is_transient);
    assert_eq!(flags, [true, true, true, false, false, false]);
}

#[test]
fn processes_opening_a_new_database_at_once_migrate_it_once() {
    let env = Home::empty();
    let writers: Vec<Writer> = (0..4).map(|_| Writer::spawn(&env, 20_000)).collect();
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
fn version_on_an_empty_home_reports_no_store_and_creates_nothing() {
    let env = TestEnv::empty();
    let out = env.cpd(&["--version"]).ok();
    assert!(out["store"].is_null(), "{out}");
    assert_eq!(out["protocol"], cpd_core::events::PROTOCOL, "{out}");
    assert!(!home(&env).db().exists());
    assert!(!home(&env).locks_dir().exists());
}

#[test]
fn version_on_a_migrated_home_reports_the_verdict_only() {
    let env = TestEnv::new();
    drop(open(&env));
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out.as_object().unwrap().len(), 4, "only ok, version, protocol and store: {out}");
    assert_eq!(out["store"], json!({"opens": true, "refusal": null, "unknown": [], "pending": []}), "{out}");
}

#[test]
fn version_on_a_store_that_does_not_open_reports_the_refusal() {
    let env = TestEnv::new();
    drop(open(&env));
    raw(&env).pragma_update(None, "user_version", VERSION + 1).unwrap();
    let before = fs::read(home(&env).db()).unwrap();
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["store"]["opens"], false, "{out}");
    assert_eq!(out["store"]["refusal"]["code"], "store_too_new", "{out}");
    assert!(out["store"]["refusal"]["message"].as_str().unwrap().contains("newer cpd"), "{out}");
    assert_eq!(fs::read(home(&env).db()).unwrap(), before);
}

#[test]
fn version_on_a_legacy_home_reports_the_conversion_up_to_0014_and_refuses_a_later_number() {
    let named: Vec<String> = cpd_core::store::testing::named_migrations().into_iter().map(|(n, ..)| n.to_owned()).collect();
    for (version, legacy_pending) in [
        (7, &["0008-phase-deploy", "0009-session-message", "0010-manager", "0011-pr-mergeable", "0012-inbox", "0013-pr-inbox", "0014-register"][..]),
        (11, &["0012-inbox", "0013-pr-inbox", "0014-register"][..]),
        (12, &["0013-pr-inbox", "0014-register"][..]),
        (13, &["0014-register"][..]),
        (14, &[][..]),
    ] {
        let env = TestEnv::empty();
        // `--version` reads only `user_version` of a legacy store: it never converts.
        raw(&env).pragma_update(None, "user_version", version).unwrap();
        let before = fs::read(home(&env).db()).unwrap();
        let out = env.cpd(&["--version"]).ok();
        let pending: Vec<String> = legacy_pending.iter().map(|n| n.to_string()).chain(named.iter().cloned()).collect();
        assert_eq!(out["store"], json!({"opens": true, "refusal": null, "unknown": [], "pending": pending}), "version {version}: {out}");
        assert_eq!(fs::read(home(&env).db()).unwrap(), before, "version {version}");
    }
    // 15 is no legacy version: a numbered migration this binary lacks.
    let env = TestEnv::empty();
    raw(&env).pragma_update(None, "user_version", 15).unwrap();
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["store"]["refusal"]["code"], "store_too_new", "{out}");
}

/// A migration this binary lacks, recorded as a newer cpd would (`cpd_txn()` is stood in by the
/// newest `txns` id).
fn apply_newer(env: &impl HasCpdHome, name: &str, compat: &str) {
    drop(open(env));
    let conn = raw(env);
    let txn: i64 = conn.query_row("SELECT max(id) FROM txns", [], |r| r.get(0)).unwrap();
    conn.create_scalar_function("cpd_txn", 0, rusqlite::functions::FunctionFlags::SQLITE_UTF8, move |_| Ok(txn)).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
         VALUES (?1, ?2, lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?3, 0)",
        rusqlite::params![name, compat, txn],
    )
    .unwrap();
}

#[test]
fn version_lists_the_additive_migrations_it_lacks_and_still_opens() {
    let env = TestEnv::new();
    apply_newer(&env, "29991231T2358-first", "additive");
    apply_newer(&env, "29991231T2359-second", "additive");
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["store"]["opens"], true, "{out}");
    assert_eq!(out["store"]["unknown"], json!(["29991231T2358-first", "29991231T2359-second"]), "{out}");
}

#[test]
fn version_refuses_a_store_whose_floor_it_lacks() {
    let env = TestEnv::new();
    apply_newer(&env, "29991231T2359-breaking", "breaking");
    let out = env.cpd(&["--version"]).ok();
    assert_eq!(out["store"]["opens"], false, "{out}");
    assert_eq!(out["store"]["refusal"]["code"], "store_too_new", "{out}");
    assert_eq!(out["store"]["unknown"], json!(["29991231T2359-breaking"]), "{out}");
}

/// store-W8: a held `locks/store.lock` is waited for at most `busy_ms`.
#[test]
fn a_held_setup_lock_gives_lock_timeout_after_store_busy_ms() {
    use nix::fcntl::{Flock, FlockArg};
    let env = Home::empty();
    let h = home(&env);
    fs::create_dir_all(h.locks_dir()).unwrap();
    let path = h.locks_dir().join("store.lock");
    let file = fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
    let _held = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();

    // The wait is the open's own retry, on virtual time.
    let clock = ManualClock::new();
    let err = Store::open_with(&clock, &h, 300, None).map(drop).unwrap_err();
    let waited = clock.elapsed();

    assert_eq!(err.code(), ErrorCode::LockTimeout, "{err}");
    assert!(waited >= Duration::from_millis(300), "gave up after {waited:?}");
    assert!(waited < Duration::from_secs(5), "waited {waited:?}");
    assert!(err.message().contains("store.lock"), "{err}");
    assert!(err.message().contains("300"), "{err}");
    assert_eq!(err.details()["lock"], path.display().to_string());
    assert_eq!(err.details()["waited_ms"], 300);
    assert!(!home(&env).db().exists(), "a refused open created the database");
}

/// A process that migrates, as the contract's marker (§1.4) describes it: `locks/store.lock` held, with
/// the marker of `pid` written into it. Dropping the guard releases the lock.
struct Holder {
    _lock: nix::fcntl::Flock<fs::File>,
}

fn hold_setup_lock(h: &CpdHome, marker: Option<Value>) -> Holder {
    use nix::fcntl::{Flock, FlockArg};
    use std::io::Write;
    fs::create_dir_all(h.locks_dir()).unwrap();
    let file = fs::OpenOptions::new().create(true).append(true).open(h.locks_dir().join("store.lock")).unwrap();
    let mut lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    if let Some(marker) = marker {
        lock.write_all(marker.to_string().as_bytes()).unwrap();
    }
    Holder { _lock: lock }
}

impl Holder {
    /// The migrating holder's marker going away while it still holds the lock.
    fn clear_marker(&self) {
        self._lock.set_len(0).unwrap();
    }
}

fn migrating_marker(pid: u32) -> Value {
    json!({"migrating": true, "pid": pid, "started_unix_ms": now_ms(), "from": 3, "to": 20, "command": "cpd test-migration"})
}

/// hs-n278: a holder that migrates is waited out for longer than `store_busy_ms`, then the open succeeds.
#[test]
fn an_open_waits_out_a_migrating_holder_for_longer_than_store_busy_ms() {
    let env = Home::new();
    let h = home(&env);
    let holder = hold_setup_lock(&h, Some(migrating_marker(std::process::id())));
    // The migration ends 1500 ms of the open's virtual time after it began: the open outlasts store_busy_ms (300 ms)
    // while a migration holds the lock.
    let clock = ScriptedClock::new().at(Duration::from_millis(1500), move || drop(holder));
    let store = Store::open_with(&clock, &h, 300, None);
    store.expect("the open waits for the migration");
    assert!(clock.elapsed() >= Duration::from_millis(1400), "did not wait for the migration: {:?}", clock.elapsed());
}

/// hs-n278, contract §4.2: a waiter that saw the migrating holder's marker keeps waiting while the same holder
/// holds the lock after the marker is gone, until the release, whatever its own `busy_ms`.
#[test]
fn a_waiter_that_saw_the_marker_waits_for_the_release_after_the_marker_is_gone() {
    let env = Home::new();
    let h = home(&env);
    let holder = std::sync::Arc::new(std::sync::Mutex::new(Some(hold_setup_lock(&h, Some(migrating_marker(std::process::id()))))));
    let (clearing, releasing) = (holder.clone(), holder);
    // The waiter (1 ms bound) polls every 20 ms, so it reads the marker in its first poll, then the lock stays held
    // for 300 ms more without a marker.
    let clock = ScriptedClock::new()
        .at(Duration::from_millis(300), move || clearing.lock().unwrap().as_ref().expect("held").clear_marker())
        .at(Duration::from_millis(600), move || drop(releasing.lock().unwrap().take()));
    let store = Store::open_with(&clock, &h, 1, None);
    store.expect("the waiter outlasts the marker, until the release");
    assert!(clock.elapsed() >= Duration::from_millis(550), "did not wait for the release: {:?}", clock.elapsed());
}

/// hs-n278, contract §4.2: an opener that waited out a migration waits for SQLite in its own setup at least
/// `MIGRATION_SETTLE_MS`, whatever its `busy_ms`: the migrating holder may still be finishing (here: another
/// connection holds the database's exclusive lock a while longer).
#[test]
fn an_opener_that_waited_out_a_migration_waits_for_sqlite_in_its_setup() {
    let env = Home::new();
    let h = home(&env);
    Store::open(&h, 5000).map(drop).unwrap();
    // A rollback-journal database, so that an exclusive lock blocks a reader at once.
    let finishing = Connection::open(h.db()).unwrap();
    finishing.pragma_update(None, "journal_mode", "DELETE").unwrap();
    finishing.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let holder = hold_setup_lock(&h, Some(migrating_marker(std::process::id())));
    // The waiter (1 ms bound) reads the marker in its first poll. The holder lets go of the setup lock 300 ms of
    // virtual time on, and the connection finishes after 300 ms more: SQLite's own busy handler is that second wait,
    // a short real wait inside the library (a reader of the database that gives up after its `busy_timeout` of
    // 300 ms, then the commit).
    let (db, finisher) = (h.db(), std::sync::Mutex::new(None));
    let clock = ScriptedClock::new().at(Duration::from_millis(300), || {
        drop(holder);
        *finisher.lock().unwrap() = Some(std::thread::spawn(move || {
            let reader = Connection::open(db).unwrap();
            reader.busy_timeout(Duration::from_millis(300)).unwrap();
            let blocked = reader.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get::<_, i64>(0));
            assert!(blocked.is_err(), "the finishing connection holds the database");
            finishing.execute_batch("COMMIT").unwrap();
        }));
    });
    let begun = Instant::now();
    let store = Store::open_with(&clock, &h, 1, None);
    let waited = begun.elapsed();
    finisher.lock().unwrap().take().expect("the holder let go of the setup lock").join().unwrap();
    store.expect("the waiter waits for SQLite in its setup");
    assert!(clock.elapsed() >= Duration::from_millis(300), "did not wait for the setup lock: {:?}", clock.elapsed());
    assert!(waited >= Duration::from_millis(250), "did not wait for the database: {waited:?}");
}

/// hs-n278: a holder with no marker keeps the `store_busy_ms` bound.
#[test]
fn a_holder_that_is_not_migrating_still_times_out_after_store_busy_ms() {
    let env = Home::new();
    let h = home(&env);
    for marker in [None, Some(json!({"migrating": false, "pid": std::process::id(), "started_unix_ms": 0, "from": 1, "to": 2, "command": "x"}))] {
        let _holder = hold_setup_lock(&h, marker);
        let err = Store::open_with(&ManualClock::new(), &h, 300, None).map(drop).unwrap_err();
        assert_eq!(err.code(), ErrorCode::LockTimeout, "{err}");
    }
}

/// hs-n278: a marker of a process that is gone does not make anyone wait.
#[test]
fn a_marker_of_a_dead_process_does_not_block() {
    let env = Home::new();
    let h = home(&env);
    // A pid no process has: above every system's `pid_max` (no child to spawn and reap).
    let dead = i32::MAX as u32;
    let _holder = hold_setup_lock(&h, Some(migrating_marker(dead)));
    let err = Store::open_with(&ManualClock::new(), &h, 300, None).map(drop).unwrap_err();
    assert_eq!(err.code(), ErrorCode::LockTimeout, "{err}");
}

/// hs-n278, contract §1.4: the opener that migrates writes its marker while it migrates and empties the file
/// when it is done; a second opener that finds the lock held waits it out, whatever its own `busy_ms`.
///
/// The test, not the clock, keeps the migration in progress: the manifest of an old Context store (migration 0007 reads
/// it, after the opener has written its marker) is a FIFO, which blocks the read until the test has a writer on it. The
/// writer's open returns when the migration is there, so the test needs no poll to know it.
#[test]
fn the_opener_that_migrates_writes_its_marker_and_clears_it() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // A home with no store: this opener migrates from 0.
    let env = Home::empty();
    let h = home(&env);
    let lock = h.locks_dir().join("store.lock");
    let folder = h.old_contexts_dir().join("c1");
    fs::create_dir_all(folder.join("records")).unwrap();
    let manifest = folder.join("context.json");
    nix::unistd::mkfifo(&manifest, nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR).unwrap();
    let first = {
        let (h, manifest) = (h.clone(), manifest.clone());
        std::thread::spawn(move || {
            let opened = Store::open(&h, 60_000).map(drop);
            // An open that ended without reading the manifest lets the test's writer through, to fail on what it finds.
            let _ = fs::OpenOptions::new().read(true).custom_flags(nix::fcntl::OFlag::O_NONBLOCK.bits()).open(&manifest);
            opened
        })
    };
    let mut writer = fs::OpenOptions::new().write(true).open(&manifest).unwrap();
    let seen: Value = serde_json::from_str(&fs::read_to_string(&lock).unwrap()).expect("no marker while the store migrated");
    assert_eq!(seen["migrating"], true, "{seen}");
    assert_eq!(seen["pid"], std::process::id(), "{seen}");
    assert_eq!(seen["from"], 0, "{seen}");
    assert!(seen["to"].as_u64().unwrap() > 0, "{seen}");
    assert!(seen["started_unix_ms"].as_u64().unwrap() > 0 && seen["command"].is_string(), "{seen}");
    // A second opener, whose own bound is 1 ms, waits for the migration instead of giving up. The migration cannot end
    // until the test writes the manifest, which it does 100 ms of the second opener's virtual time on.
    let second = {
        let clock = ScriptedClock::new().at(Duration::from_millis(100), || {
            assert!(!first.is_finished(), "an opener returned while the migration was held");
            assert!(!fs::read_to_string(&lock).unwrap().is_empty(), "the marker was cleared while the migration was held");
            writer
                .write_all(br#"{"v":1,"id":"c1","title":"Old","jira":null,"created_at":"2026-01-01T00:00:00.000Z","created_on":"m1"}"#)
                .unwrap();
            drop(writer);
        });
        Store::open_with(&clock, &h, 1, None).map(drop)
    };
    first.join().unwrap().unwrap();
    second.expect("the second opener waits for the migration");
    assert_eq!(fs::read_to_string(&lock).unwrap(), "", "the marker outlived the migration");
}

/// `SQLITE_BUSY` (5) and `SQLITE_LOCKED` (6) are both `store_busy` (contract §4.2).
#[test]
fn sqlite_busy_and_sqlite_locked_are_store_busy() {
    for primary in [5, 6] {
        let sqlite = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(primary), None);
        let err = CpdError::from(sqlite);
        assert_eq!(err.code(), ErrorCode::StoreBusy, "SQLite code {primary}: {err:?}");
    }
}
