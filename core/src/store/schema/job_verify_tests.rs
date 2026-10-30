//! The migration `job-verify-kind` (contract §4.4, §4.5): `jobs.kind` accepts `verify`; the rebuilt table keeps its rows and triggers.

use rusqlite::Connection;
use serde_json::Value;

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn sqlite_message(err: &CpdError) -> String {
    err.details()["sqlite_message"].as_str().unwrap_or_default().to_owned()
}

/// A Plan `pl` with a `free` PR `a`; Jobs of the kinds before `verify` in every status; Dependencies and an Attempt.
const FIXTURE: &str = "
  INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
  INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
    VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
  INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
    VALUES ('pl', 'ctx', 'Plan', 'dispatch', 'lead', 't', 't');
  INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
    VALUES ('pl', 'a', '/r', 'main', 'waiting', 'cpd/pl/a', cpd_txn(), 't');
  INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
    VALUES ('pl', 'a', 'w-draft', 'W', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, 'work', NULL, cpd_txn(), 't'),
           ('pl', 'a', 'd-queued', 'D', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'docs', 'FP', cpd_txn(), 't'),
           ('pl', 'a', 'i-running', 'I', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'impl', 'FP', cpd_txn(), 't'),
           ('pl', 'a', 'o-merged', 'O', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'open', 'FP', cpd_txn(), 't'),
           ('pl', 'a', 'u-failed', 'U', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'update', 'FP', cpd_txn(), 't'),
           ('pl', 'a', 'w-gone', 'G', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'work', 'FP', cpd_txn(), 't');
  INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, tests, test_files, fingerprint, created_txn, created_at)
    VALUES ('pl', 'a', 't-queued', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'test', '[\"t::one\"]', '[\"tests/t.rs\"]', 'FP', cpd_txn(), 't');
  UPDATE jobs SET status = 'running' WHERE key IN ('i-running', 'o-merged', 'u-failed');
  UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'o-merged';
  UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'u-failed';
  UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'w-gone';
  INSERT INTO dependencies (plan, job, needs) VALUES ('pl', 'i-running', 't-queued'), ('pl', 'd-queued', 'w-draft');
  INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
    VALUES ('pl', 'i-running-a1', 'i-running', 1, 'S2', 'ladder', 'running', 'cpd/pl--i-running-a1', '/w', 't');
";

/// A store as every migration before `job-verify-kind` leaves it, holding [`FIXTURE`]. The test fails here while the migration is missing.
fn before_verify() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED.iter().copied().take_while(|n| !n.name.ends_with("-job-verify-kind")).collect();
    assert!(earlier.len() < NAMED.len(), "job-verify-kind is a built-in migration");
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(&mut store, &FIXTURE.replace("FP", FP)).unwrap();
    drop(store);
    (dir, home, earlier)
}

/// Every row of `table`, every column, as JSON, in primary-key-free `rowid` order.
fn table_rows(store: &mut Store, table: &str) -> Vec<Value> {
    store
        .read(|c: &Connection| {
            let columns = table_columns(c, table)?;
            let pairs: Vec<String> = columns.iter().map(|(name, _)| format!("'{name}', {name}")).collect();
            let mut stmt = c.prepare(&format!("SELECT json_object({}) FROM {table} ORDER BY rowid", pairs.join(", ")))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|r| serde_json::from_str(&r).unwrap()).collect())
        })
        .unwrap()
}

/// `(name, sql)` of the triggers on `jobs`, whitespace and comments normalised, without the `skipped_n` of `job-tenancy`, which sorts after this migration.
fn jobs_triggers(store: &mut Store) -> Vec<(String, String)> {
    store
        .read(|c: &Connection| {
            let mut stmt = c.prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'jobs' ORDER BY name")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .map(|(name, sql)| {
                    let code: Vec<&str> = sql.lines().map(|l| l.split("--").next().unwrap_or_default()).collect();
                    (name, code.join(" ").split_whitespace().collect::<Vec<_>>().join(" ").replace(" ,", ",").replace(" )", ")").replace(", 'skipped_n', OLD.skipped_n", "").replace(", 'skipped_n', NEW.skipped_n", "")
                        .replace(" UNION ALL SELECT 'skipped_n', OLD.skipped_n WHERE OLD.skipped_n IS NOT NEW.skipped_n", "")
                        .replace(" UNION ALL SELECT 'skipped_n', NEW.skipped_n WHERE OLD.skipped_n IS NOT NEW.skipped_n", "")
                        .replace(", 'summary', OLD.summary", "").replace(", 'summary', NEW.summary", "")
                        .replace(" UNION ALL SELECT 'summary', OLD.summary WHERE OLD.summary IS NOT NEW.summary", "")
                        .replace(" UNION ALL SELECT 'summary', NEW.summary WHERE OLD.summary IS NOT NEW.summary", ""))
                })
                .collect())
        })
        .unwrap()
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store.read(|c: &Connection| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

fn job_of_kind(key: &str, kind: &str, status: &str, columns: &str, values: &str) -> String {
    let fingerprint = if status == "queued" { format!("'{FP}'") } else { "NULL".to_owned() };
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint{columns}, created_txn, created_at)
         VALUES ('pl', 'a', '{key}', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', '{status}', 1, '{kind}', {fingerprint}{values}, cpd_txn(), 't')"
    )
}

/// A fully migrated store holding only the Plan and PR of [`FIXTURE`].
fn migrated_empty() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    let head = FIXTURE.split("  INSERT\u{20}INTO jobs").next().unwrap();
    write(&mut store, head).unwrap();
    (dir, store)
}

#[test]
fn the_jobs_table_accepts_the_verify_kind_and_still_refuses_an_unknown_kind() {
    let (_dir, mut store) = migrated_empty();
    write(&mut store, &job_of_kind("v-draft", "verify", "draft", "", "")).unwrap();
    write(&mut store, &job_of_kind("v-queued", "verify", "queued", "", "")).unwrap();
    assert_eq!(count(&mut store, "SELECT count(*) FROM jobs WHERE kind = 'verify'"), 2);
    let bad = write(&mut store, &job_of_kind("e-bad", "e2e", "draft", "", "")).expect_err("e2e is no kind");
    assert!(sqlite_message(&bad).contains("CHECK constraint failed"), "{bad:?}");
}

#[test]
fn the_migration_keeps_every_job_dependency_and_attempt_row() {
    let (_dir, home, earlier) = before_verify();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    let before: Vec<Vec<Value>> = ["jobs", "dependencies", "attempts"].iter().map(|t| table_rows(&mut old, t)).collect();
    drop(old);
    assert_eq!(before[0].len(), 7, "the fixture: a Job of every kind and status");
    assert_eq!((before[1].len(), before[2].len()), (2, 1));

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    for (table, want) in ["jobs", "dependencies", "attempts"].iter().zip(before) {
        let mut after = table_rows(&mut store, table);
        for row in &mut after {
            // `skipped_n` is the column of `job-tenancy`, which sorts after this migration: null in every row.
            if let Some(row) = row.as_object_mut().filter(|_| *table == "jobs") {
                assert_eq!(row.remove("skipped_n"), Some(Value::Null));
                // `summary` is the column of `explore-kinds`, which sorts after this migration: null in every row.
                assert_eq!(row.remove("summary"), Some(Value::Null));
            }
            // `runner` and `worker_pid` are the columns of `worker-runner`, which sorts after this migration: `pi` and null.
            if let Some(row) = row.as_object_mut().filter(|_| *table == "attempts") {
                assert_eq!(row.remove("runner"), Some(Value::from("pi")));
                assert_eq!(row.remove("worker_pid"), Some(Value::Null));
                // `gate_hook`, `gate_command` and `gate_exit` are the columns of `gate-facts`, later still: null.
                for key in ["gate_hook", "gate_command", "gate_exit"] {
                    assert_eq!(row.remove(key), Some(Value::Null));
                }
            }
        }
        assert_eq!(after, want, "every row of {table}, column for column (`seq` kept)");
    }
    assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
}

#[test]
fn the_migration_keeps_the_triggers_of_jobs() {
    let (_dir, home, earlier) = before_verify();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    let before = jobs_triggers(&mut old);
    drop(old);
    assert!(!before.is_empty(), "jobs has triggers");

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    assert_eq!(jobs_triggers(&mut store), before);
    let history = "SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'update'";
    let seen = count(&mut store, history);
    write(&mut store, "UPDATE jobs SET title = 'Renamed' WHERE key = 'w-draft'").unwrap();
    assert_eq!(count(&mut store, history), seen + 1, "an UPDATE of a Job writes a history row");
}

#[test]
fn a_verify_job_with_tests_or_an_approval_is_refused_like_a_docs_job() {
    let (_dir, mut store) = migrated_empty();
    let tests = ", tests, test_files";
    let named = ", '[\"t::one\"]', '[\"tests/t.rs\"]'";
    // The docs Job is the model: refused `spec_invalid`.
    let docs = write(&mut store, &job_of_kind("d-1", "docs", "draft", tests, named)).expect_err("docs with tests");
    assert_eq!(sqlite_message(&docs), "spec_invalid");
    write(&mut store, &job_of_kind("v-ok", "verify", "draft", "", "")).unwrap();
    let with_tests = write(&mut store, &job_of_kind("v-1", "verify", "draft", tests, named)).expect_err("verify with tests");
    assert_eq!(sqlite_message(&with_tests), "spec_invalid", "{with_tests:?}");
    let approved = write(&mut store, &job_of_kind("v-2", "verify", "draft", ", test_approved", ", '[\"tests/t.rs\"]'")).expect_err("verify approved");
    assert_eq!(sqlite_message(&approved), "spec_invalid", "{approved:?}");
    let on_update = write(&mut store, "UPDATE jobs SET test_approved = '[\"tests/t.rs\"]' WHERE key = 'v-ok'").expect_err("update approved");
    assert_eq!(sqlite_message(&on_update), "spec_invalid", "{on_update:?}");
}
