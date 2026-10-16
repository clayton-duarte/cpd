//! The schema: migrations, the generated history triggers and the schema hash
//! (contract §4.3–§4.5).

use std::path::Path;

use rusqlite::{Connection, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ids::PrKey;

/// A migration: applied once, in order, inside the migrating transaction.
pub(super) type Migration = fn(&Connection) -> Result<()>;

/// Migrations `1..=N`, in order; `MIGRATIONS[i]` takes the schema to version `i + 1`.
pub(super) const MIGRATIONS: &[Migration] = &[migration_1, migration_2, migration_3];

/// The schema hash of each version (§4.3): `SCHEMA_HASHES[i]` is version `i + 1`'s.
/// A test asserts that migrating an empty database gives each of them.
pub(super) const SCHEMA_HASHES: &[&str] = &[
    "0c73adb58e9a2027348333015d1aaf952d3bc3b0923ea27bcd41cac1079dcced",
    "37449f2445f3d621405857abab17a5f3760382270a1b6a55f46a116792998646",
    "f91dd416097aa862afda7c4b120efc99a220013a5521bce53290e776a878a332",
];

const _: () = assert!(SCHEMA_HASHES.len() == MIGRATIONS.len(), "one schema hash per migration");

/// The schema version this binary writes: `N`.
pub const VERSION: i64 = MIGRATIONS.len() as i64;

/// The tables whose rows are copied into `history` by generated triggers (§4.4),
/// at the current version. `dependencies` and `pr_needs` get no generated
/// `_no_delete`: their migrations write their own (a draft Job's Dependency,
/// and a PR's need before the lock, may be deleted) and their `_hist_del`.
pub const HISTORY_TABLES: &[&str] = &[
    "contexts",
    "context_records",
    "plans",
    "prs",
    "pr_needs",
    "plan_reviews",
    "jobs",
    "dependencies",
    "attempts",
    "escalations",
    "sessions",
    "tier_catalog",
];

/// [`HISTORY_TABLES`] as migration 1 created them; frozen with it.
const VERSION_1_HISTORY_TABLES: &[&str] = &[
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
    for table in VERSION_1_HISTORY_TABLES {
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

/// The line of `0003.sql` that splits it into the part before the PRs are
/// filled and the part after.
const FILL_PRS: &str = "\n-- @fill-prs\n";

/// Plans across repos (§4.5, R34 A6, B2), steps 2 to 4; step 1 (foreign keys
/// off, `legacy_alter_table` on) and step 5 (`foreign_key_check`) are the
/// caller's, around the migrating transaction. Needs the migration's `txns`
/// row: each migrated PR's `created_txn` and history row name it.
fn migration_3(conn: &Connection) -> Result<()> {
    let (tables, rebuild) = include_str!("migrations/0003.sql")
        .split_once(FILL_PRS)
        .expect("0003.sql has its `-- @fill-prs` line");
    conn.execute_batch(tables)?;
    create_history_triggers(conn, "prs", true)?;
    create_history_triggers(conn, "pr_needs", false)?;
    fill_prs(conn)?;
    conn.execute_batch(rebuild)?;
    for table in ["plans", "jobs", "context_records"] {
        create_history_triggers(conn, table, true)?;
    }
    Ok(())
}

/// §4.5 step 2: one PR per Plan, its per-repo columns kept as stored, its
/// status from the Plan's phase. No `pr_needs`: one PR has no needs.
fn fill_prs(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, repo FROM plans ORDER BY id")?;
    let plans: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_>>()?;
    let mut insert = conn.prepare(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks, all_merged_at,
                          start_failed_at, created_txn, created_at, started_at, merged_at)
         SELECT id, ?2, repo, base,
                CASE phase WHEN 'grounding' THEN 'waiting' WHEN 'plan' THEN 'waiting' WHEN 'dispatch' THEN 'dispatch'
                           WHEN 'pr' THEN 'open' WHEN 'done' THEN 'merged' END,
                lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks, all_merged_at,
                NULL, cpd_txn(), created_at, locked_at, done_at
         FROM plans WHERE id = ?1",
    )?;
    for (plan, repo) in plans {
        insert.execute((&plan, migrated_pr_key(&repo).as_str()))?;
    }
    Ok(())
}

/// The key of the PR migration 3 makes from a Plan (§4.5): the base name of
/// its repo, lower-cased, each character outside `[a-z0-9-]` replaced by `-`,
/// prefixed `pr-` when it does not then start with a letter, cut to 24
/// characters.
fn migrated_pr_key(repo: &str) -> PrKey {
    let name = Path::new(repo).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut key: String =
        name.chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' { c } else { '-' }).collect();
    if !key.starts_with(|c: char| c.is_ascii_lowercase()) {
        key.insert_str(0, "pr-");
    }
    key.truncate(24);
    PrKey::parse(&key).expect("the rule above always makes a valid PR key")
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
    use crate::error::{CpdError, ErrorCode};
    use crate::home::CpdHome;
    use crate::store::{APPLICATION_ID, Store};

    /// A connection with a stand-in `cpd_txn()`, which the history triggers call.
    fn connection(path: &std::path::Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64)).unwrap();
        conn
    }

    /// The schema after the first `n` migrations only, with the pragmas
    /// `Store::open` sets around them (§4.5 migration 3, step 1).
    fn at_version(conn: &Connection, n: usize) {
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
        for migration in &MIGRATIONS[..n] {
            migration(conn).unwrap();
        }
        conn.pragma_update(None, "legacy_alter_table", "OFF").unwrap();
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

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
    }

    /// A database at version `n` holding `sql`, written as cpd's txn 1.
    fn database_at(home: &CpdHome, n: usize, sql: &str) {
        std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
        let conn = connection(&home.db());
        conn.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
        at_version(&conn, n);
        conn.execute_batch(&format!(
            "BEGIN;
             INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '2026-01-01T00:00:00.000Z', 'migrate', 1, NULL, '/');
             {sql}
             COMMIT;"
        ))
        .unwrap();
    }

    // ---------- migration 2 ----------

    const VERSION_1_PLAN: &str =
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/repo', 'O3', 'idle', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                            pr_number, pr_url, pr_opened_at, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', '/repo', 'main', 'cpd/pay', '/wt', 'pr', 'l-pay', 7, 'https://example.test/pull/7',
                   '2026-01-05T00:00:00.000Z', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');";

    #[test]
    fn a_version_1_database_migrates_and_its_pr_keeps_the_poll_columns_null() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        database_at(&home, 1, VERSION_1_PLAN);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        let (version, row, migrations): (i64, (String, i64, Option<String>, Option<String>, Option<String>), i64) = store
            .read(|c| {
                Ok((
                    c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                    c.query_row(
                        "SELECT key, pr_number, pr_title, pr_review_decision, pr_checks FROM prs WHERE plan = 'pay'",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )?,
                    c.query_row("SELECT count(*) FROM txns WHERE command = 'migrate'", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(version, VERSION);
        assert_eq!(row, ("repo".to_owned(), 7, None, None, None), "the PR keeps the three columns null until its next poll");
        assert_eq!(migrations, 2, "the old migration's row and the new one");
    }

    #[test]
    fn prs_history_lists_the_poll_columns_and_their_checks_hold() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        database_at(&home, 1, VERSION_1_PLAN);
        let mut store = Store::open(&home, 5000).unwrap();

        store
            .write("test", |tx| {
                tx.execute(
                    "UPDATE prs SET pr_title = 'T', pr_review_decision = 'approved', pr_checks = 'passed' WHERE plan = 'pay'",
                    [],
                )?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
        let after: String = store
            .read(|c| Ok(c.query_row("SELECT after FROM history WHERE tbl = 'prs' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))?))
            .unwrap();
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["pr_title"], "T");
        assert_eq!(after["pr_review_decision"], "approved");
        assert_eq!(after["pr_checks"], "passed");

        for bad in ["pr_title = ''", "pr_review_decision = 'maybe'", "pr_checks = 'green'", "pr_title = NULL"] {
            let sql = format!("UPDATE prs SET {bad} WHERE plan = 'pay'");
            let err = store.write("test", |tx| Ok::<(), CpdError>({ tx.execute(&sql, [])?; })).unwrap_err();
            assert!(err.to_string().contains("CHECK") || format!("{err:?}").contains("CHECK"), "{bad}: {err:?}");
        }
    }

    // ---------- migration 3 ----------

    #[test]
    fn migrated_pr_keys_follow_4_5() {
        for (repo, key) in [
            ("/src/pay", "pay"),
            ("/src/My Repo.git", "my-repo-git"),
            ("/src/2fa", "pr-2fa"),
            ("/src/-x", "pr--x"),
            ("/src/Ünïcode", "pr--n-code"),
            ("/src/web/", "web"),
            ("/src/a-very-long-repository-name-indeed", "a-very-long-repository-n"),
            ("/src/123456789012345678901234567", "pr-123456789012345678901"),
            ("/", "pr-"),
        ] {
            assert_eq!(migrated_pr_key(repo).as_str(), key, "{repo}");
        }
    }

    const T: &str = "2026-01-01T00:00:00.000Z";
    const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// A version 2 database with one Plan in each phase, Jobs, Dependencies,
    /// Attempts, an Escalation, a Plan review, Events and Context records.
    fn version_2(home: &CpdHome) {
        let sql = format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
               ('l-gr', 'lead', 'ctx', 'gr', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-pl', 'lead', 'ctx', 'pl', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-di', 'lead', 'ctx', 'di', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-pr', 'lead', 'ctx', 'pr', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-dn', 'lead', 'ctx', 'dn', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at) VALUES
               ('gr', 'ctx', 'Grounding', '/src/pay', 'main', 'cpd/gr', NULL, 'grounding', 'l-gr', '2026-01-01T00:00:00.001Z'),
               ('pl', 'ctx', 'Planning', '/src/My Repo.git', 'main', 'cpd/pl', NULL, 'plan', 'l-pl', '2026-01-01T00:00:00.002Z');
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, locked_at) VALUES
               ('di', 'ctx', 'Dispatch', '/src/2fa', 'main', 'cpd/di', '/src/2fa.di', 'dispatch', 'l-di', '2026-01-01T00:00:00.003Z', '2026-01-02T00:00:00.003Z');
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                                pr_number, pr_url, pr_opened_at, pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks,
                                all_merged_at, created_at, locked_at) VALUES
               ('pr', 'ctx', 'Open', '/src/a-very-long-repository-name-indeed', 'main', 'cpd/pr', '/src/long.pr', 'pr', 'l-pr',
                7, 'https://example.test/pull/7', '2026-01-03T00:00:00.004Z', '{{\"checks\":{{\"ci\":\"SUCCESS\"}},\"reviewIds\":[\"r1\"],\"commentIds\":[]}}',
                '2026-01-04T00:00:00.004Z', 'Open PR', 'approved', 'passed', '2026-01-03T00:00:00.004Z', '2026-01-01T00:00:00.004Z', '2026-01-02T00:00:00.004Z');
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                                pr_number, pr_url, pr_opened_at, created_at, locked_at, done_at) VALUES
               ('dn', 'ctx', 'Done', '/src/web/', 'develop', 'cpd/dn', '/src/web.dn', 'done', 'l-dn',
                9, 'https://example.test/pull/9', '2026-01-03T00:00:00.005Z', '2026-01-01T00:00:00.005Z', '2026-01-02T00:00:00.005Z', '2026-01-05T00:00:00.005Z');
             INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
               VALUES ('pl', 'r1', 1, '{FP}', 'approved', '{T}', '{T}');
             INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, merged_sha) VALUES
               ('pl', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, NULL, 1, '{T}', NULL),
               ('pl', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, NULL, 1, '{T}', NULL),
               ('di', 'x', 'X', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{FP}', 1, '{T}', NULL),
               ('di', 'y', 'Y', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{FP}', 1, '{T}', NULL),
               ('pr', 'z', 'Z', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'abc'),
               ('dn', 'w', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'def');
             INSERT INTO dependencies (plan, job, needs) VALUES ('pl', 'b', 'a'), ('di', 'x', 'y');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree,
                                   merged_sha, started_at, ended_at) VALUES
               ('di', 'y-a1', 'y', 1, 'S2', 'ladder', 'running', NULL, NULL, 'cpd/di--y-a1', '/src/2fa.di--y-a1', NULL, '{T}', NULL),
               ('pr', 'z-a1', 'z', 1, 'S2', 'ladder', 'ended', 'merged', 'merged', 'cpd/pr--z-a1', '/w', 'abc', '{T}', '{T}');
             INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
               VALUES ('di', 'y-a1-q1', 'y-a1', 1, 'Why?', 'open', 1, '{T}');
             INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES
               (1, '{T}', 'plan-created', 'ctx', 'gr', NULL, 'x', '{{}}'),
               (1, '{T}', 'job-merged', 'ctx', 'pr', 'z', 'x', '{{}}'),
               (1, '{T}', 'all-merged', 'ctx', 'pr', NULL, 'x', '{{}}');
             INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine, plan_repo, plan_jira) VALUES
               ('ctx', 'm1-p1', 'plan', '{T}', 'm1', 'Grounding', 'gr', 'm1', '/src/pay', 'PAY-1');
             INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n1', 'note', '{T}', 'm1', 'A note');"
        );
        database_at(home, 2, &sql);
    }

    /// `version_2`, migrated by `Store::open`.
    fn migrated(dir: &tempfile::TempDir) -> Store {
        let home = home(dir);
        version_2(&home);
        let store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        store
    }

    fn rows(store: &mut Store, sql: &str) -> Vec<Value> {
        store
            .read(|c| {
                let mut stmt = c.prepare(sql)?;
                let n = stmt.column_count();
                let rows = stmt
                    .query_map([], |r| {
                        (0..n)
                            .map(|i| {
                                Ok(match r.get_ref(i)? {
                                    rusqlite::types::ValueRef::Null => Value::Null,
                                    rusqlite::types::ValueRef::Integer(v) => Value::from(v),
                                    rusqlite::types::ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
                                    other => panic!("{other:?}"),
                                })
                            })
                            .collect::<Result<Vec<Value>>>()
                            .map(Value::from)
                    })?
                    .collect::<Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    fn count(store: &mut Store, sql: &str) -> i64 {
        store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn a_version_2_database_migrates_to_3_with_one_pr_per_plan() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        assert_eq!(count(&mut store, "PRAGMA user_version"), 3);
        let migrate = count(&mut store, "SELECT max(id) FROM txns WHERE command = 'migrate'");
        assert_eq!(migrate, 2);

        let prs = rows(
            &mut store,
            "SELECT plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pr_seen,
                    pr_polled_at, pr_title, pr_review_decision, pr_checks, all_merged_at, start_failed_at, created_txn,
                    created_at, started_at, merged_at
             FROM prs ORDER BY created_at",
        );
        let empty_seen = r#"{"checks":{},"reviewIds":[],"commentIds":[]}"#;
        let n = Value::Null;
        assert_eq!(
            prs,
            vec![
                serde_json::json!(["gr", "pay", "/src/pay", "main", "waiting", "cpd/gr", n, n, n, n, empty_seen,
                                   n, n, n, n, n, n, migrate, "2026-01-01T00:00:00.001Z", n, n]),
                serde_json::json!(["pl", "my-repo-git", "/src/My Repo.git", "main", "waiting", "cpd/pl", n, n, n, n, empty_seen,
                                   n, n, n, n, n, n, migrate, "2026-01-01T00:00:00.002Z", n, n]),
                serde_json::json!(["di", "pr-2fa", "/src/2fa", "main", "dispatch", "cpd/di", "/src/2fa.di", n, n, n, empty_seen,
                                   n, n, n, n, n, n, migrate, "2026-01-01T00:00:00.003Z", "2026-01-02T00:00:00.003Z", n]),
                serde_json::json!(["pr", "a-very-long-repository-n", "/src/a-very-long-repository-name-indeed", "main", "open",
                                   "cpd/pr", "/src/long.pr", 7, "https://example.test/pull/7", "2026-01-03T00:00:00.004Z",
                                   r#"{"checks":{"ci":"SUCCESS"},"reviewIds":["r1"],"commentIds":[]}"#,
                                   "2026-01-04T00:00:00.004Z", "Open PR", "approved", "passed", "2026-01-03T00:00:00.004Z", n,
                                   migrate, "2026-01-01T00:00:00.004Z", "2026-01-02T00:00:00.004Z", n]),
                serde_json::json!(["dn", "web", "/src/web/", "develop", "merged", "cpd/dn", "/src/web.dn", 9,
                                   "https://example.test/pull/9", "2026-01-03T00:00:00.005Z", empty_seen, n, n, n, n, n, n,
                                   migrate, "2026-01-01T00:00:00.005Z", "2026-01-02T00:00:00.005Z", "2026-01-05T00:00:00.005Z"]),
            ]
        );
        assert_eq!(count(&mut store, "SELECT count(*) FROM pr_needs"), 0);

        // Every Job has its Plan's one PR, is no review Job, and keeps its seq.
        assert_eq!(
            rows(&mut store, "SELECT seq, plan, pr, review, key, status FROM jobs ORDER BY seq"),
            vec![
                serde_json::json!([1, "pl", "my-repo-git", 0, "a", "draft"]),
                serde_json::json!([2, "pl", "my-repo-git", 0, "b", "draft"]),
                serde_json::json!([3, "di", "pr-2fa", 0, "x", "queued"]),
                serde_json::json!([4, "di", "pr-2fa", 0, "y", "running"]),
                serde_json::json!([5, "pr", "a-very-long-repository-n", 0, "z", "merged"]),
                serde_json::json!([6, "dn", "web", 0, "w", "merged"]),
            ]
        );

        // Every other row and id is kept.
        assert_eq!(
            rows(&mut store, "SELECT id, phase, locked_at, done_at FROM plans ORDER BY created_at"),
            vec![
                serde_json::json!(["gr", "grounding", n, n]),
                serde_json::json!(["pl", "plan", n, n]),
                serde_json::json!(["di", "dispatch", "2026-01-02T00:00:00.003Z", n]),
                serde_json::json!(["pr", "pr", "2026-01-02T00:00:00.004Z", n]),
                serde_json::json!(["dn", "done", "2026-01-02T00:00:00.005Z", "2026-01-05T00:00:00.005Z"]),
            ]
        );
        assert_eq!(rows(&mut store, "SELECT id, kind, plan, job FROM events ORDER BY id"), vec![
            serde_json::json!([1, "plan-created", "gr", n]),
            serde_json::json!([2, "job-merged", "pr", "z"]),
            serde_json::json!([3, "all-merged", "pr", n]),
        ]);
        assert_eq!(rows(&mut store, "SELECT seq, id, plan_id, plan_machine, plan_jira FROM context_records ORDER BY seq"), vec![
            serde_json::json!([1, "m1-p1", "gr", "m1", "PAY-1"]),
            serde_json::json!([2, "m1-n1", n, n, n]),
        ]);
        assert_eq!(rows(&mut store, "SELECT plan, job, needs FROM dependencies ORDER BY plan"), vec![
            serde_json::json!(["di", "x", "y"]),
            serde_json::json!(["pl", "b", "a"]),
        ]);
        assert_eq!(count(&mut store, "SELECT count(*) FROM attempts"), 2);
        assert_eq!(count(&mut store, "SELECT count(*) FROM escalations"), 1);
        assert_eq!(count(&mut store, "SELECT count(*) FROM sessions"), 5);
        assert_eq!(count(&mut store, "SELECT count(*) FROM plan_reviews"), 1);
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"), 0);

        // The moved and dropped columns are gone.
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_table_info('plans')"), 9);
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_table_info('context_records') WHERE name = 'plan_repo'"), 0);

        // History: one insert per migrated PR in the migration's txn; the copies write none.
        assert_eq!(
            rows(&mut store, "SELECT txn, tbl, pk, op FROM history WHERE txn = 2 ORDER BY id"),
            ["di", "dn", "gr", "pl", "pr"]
                .iter()
                .map(|plan| {
                    let key = prs.iter().find(|p| p[0] == *plan).unwrap()[1].clone();
                    serde_json::json!([2, "prs", serde_json::json!([plan, key]).to_string(), "insert"])
                })
                .collect::<Vec<_>>()
        );

        // AUTOINCREMENT goes on after the kept ids.
        let (job, event, record) = store
            .write("test", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                                       appended, created_txn, created_at)
                       VALUES ('gr', 'pay', 'n', 'N', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), '{T}');
                     INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (cpd_txn(), '{T}', 'pr-added', 'ctx', 'gr', 'x', '{{}}');
                     INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n2', 'note', '{T}', 'm1', 'x');"
                ))?;
                Ok(tx.query_row(
                    "SELECT (SELECT max(seq) FROM jobs), (SELECT max(id) FROM events), (SELECT max(seq) FROM context_records)",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!((job, event, record), (7, 4, 3));
    }

    #[test]
    fn a_migrated_database_has_the_schema_of_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        drop(migrated(&dir));
        let fresh = tempfile::tempdir().unwrap();
        let fresh_home = home(&fresh);
        drop(Store::open(&fresh_home, 5000).unwrap());
        let hash = |home: &CpdHome| schema_hash(&Connection::open(home.db()).unwrap()).unwrap();
        assert_eq!(hash(&home(&dir)), hash(&fresh_home));
        assert_eq!(hash(&fresh_home), SCHEMA_HASHES[2]);
    }

    #[test]
    fn a_migration_that_breaks_a_foreign_key_fails_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        // An Event about a Plan that does not exist (written with foreign keys off).
        database_at(&home, 2, &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (1, '{T}', 'plan-created', 'ctx', 'gone', 'x', '{{}}');"
        ));
        let err = Store::open(&home, 5000).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
        assert_eq!(err.details()["foreign_key_violations"], serde_json::json!([{"table": "events", "rowid": 1, "parent": "plans"}]));
        let conn = Connection::open(home.db()).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, 2);
        assert_eq!(schema_hash(&conn).unwrap(), SCHEMA_HASHES[1]);
    }

    // ---------- the triggers of §4.4 after migration 3 ----------

    /// Runs `sql` in its own write transaction.
    fn write(store: &mut Store, sql: &str) -> std::result::Result<(), CpdError> {
        store.write("test", |tx| Ok(tx.execute_batch(sql)?))
    }

    fn refused(store: &mut Store, sql: &str) -> CpdError {
        write(store, sql).expect_err(sql)
    }

    fn sqlite_message(err: &CpdError) -> String {
        err.details()["sqlite_message"].as_str().unwrap_or_default().to_owned()
    }

    /// A second PR `web` in Plan `gr` (grounding), with a draft Job `v`.
    const WEB: &str = "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                         VALUES ('gr', 'web', '/src/web', 'main', 'waiting', 'cpd/gr/web', cpd_txn(), 't');
                       INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                         status, appended, created_txn, created_at)
                         VALUES ('gr', 'pay', 'u', 'U', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't'),
                                ('gr', 'web', 'v', 'V', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');";

    #[test]
    fn prs_status_refuses_a_transition_outside_3_8() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        for sql in [
            "UPDATE prs SET status = 'open', lead_worktree = '/w', started_at = 't', pr_number = 1, pr_url = 'u', pr_opened_at = 't' WHERE plan = 'gr'",
            "UPDATE prs SET status = 'waiting', lead_worktree = NULL, started_at = NULL WHERE plan = 'di'",
            "UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE plan = 'di'",
            "UPDATE prs SET status = 'open', merged_at = NULL WHERE plan = 'dn'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
        }
        write(&mut store, "UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'gr';
                           UPDATE prs SET status = 'cancelled' WHERE plan = 'pl';
                           UPDATE prs SET status = 'open', pr_number = 1, pr_url = 'u', pr_opened_at = 't' WHERE plan = 'di';
                           UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pr'")
            .unwrap();
    }

    #[test]
    fn prs_locked_refuses_a_change_of_identity_repo_or_base_after_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        for sql in [
            "UPDATE prs SET repo = '/src/other' WHERE plan = 'di'",
            "UPDATE prs SET base = 'develop' WHERE plan = 'pr'",
            "UPDATE prs SET key = 'other' WHERE plan = 'dn'",
            "UPDATE prs SET lead_branch = 'cpd/di/pr-2fa' WHERE plan = 'di'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::PrLocked, "{sql}: {err:?}");
            assert_eq!(err.exit().code(), 3);
        }
        // Before the lock they may change; after it, the other columns may.
        write(&mut store, "UPDATE prs SET repo = '/src/other', base = 'develop' WHERE plan IN ('gr', 'pl');
                           UPDATE prs SET pr_polled_at = 't' WHERE plan = 'pr'")
            .unwrap();
    }

    #[test]
    fn pr_needs_locked_refuses_a_need_after_the_lock_unless_the_pr_is_new() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        let add = |key: &str| {
            format!(
                "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('di', '{key}', '/src/web', 'main', 'waiting', 'cpd/di/{key}', cpd_txn(), 't');"
            )
        };
        write(&mut store, &add("old")).unwrap();
        let err = refused(&mut store, "INSERT INTO pr_needs (plan, pr, needs) VALUES ('di', 'old', 'pr-2fa')");
        assert_eq!(err.code(), ErrorCode::PrLocked, "{err:?}");
        // `pr add` in dispatch: the PR and its needs in one transaction.
        write(&mut store, &format!("{} INSERT INTO pr_needs (plan, pr, needs) VALUES ('di', 'new', 'pr-2fa');", add("new"))).unwrap();
        // Before the lock, any time.
        write(&mut store, WEB).unwrap();
        write(&mut store, "INSERT INTO pr_needs (plan, pr, needs) VALUES ('gr', 'web', 'pay')").unwrap();
    }

    #[test]
    fn pr_needs_no_delete_refuses_after_the_lock_and_history_records_a_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        write(&mut store, &format!("{WEB} INSERT INTO pr_needs (plan, pr, needs) VALUES ('gr', 'web', 'pay');")).unwrap();
        write(&mut store, "DELETE FROM pr_needs WHERE plan = 'gr'").unwrap();
        assert_eq!(
            rows(&mut store, "SELECT pk, op, before, after FROM history WHERE tbl = 'pr_needs' ORDER BY id"),
            vec![
                serde_json::json!([r#"["gr","web","pay"]"#, "insert", Value::Null, r#"{"plan":"gr","pr":"web","needs":"pay"}"#]),
                serde_json::json!([r#"["gr","web","pay"]"#, "delete", r#"{"plan":"gr","pr":"web","needs":"pay"}"#, Value::Null]),
            ]
        );
        write(&mut store, "INSERT INTO pr_needs (plan, pr, needs) VALUES ('gr', 'web', 'pay');
                           UPDATE plans SET phase = 'plan' WHERE id = 'gr';
                           UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'gr'")
            .unwrap();
        let err = refused(&mut store, "DELETE FROM pr_needs WHERE plan = 'gr'");
        assert_eq!(err.code(), ErrorCode::PrLocked, "{err:?}");
    }

    #[test]
    fn dependencies_same_pr_refuses_a_dependency_across_prs() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        write(&mut store, WEB).unwrap();
        let err = refused(&mut store, "INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')");
        assert_eq!(err.code(), ErrorCode::NeedsOtherPr, "{err:?}");
        assert_eq!(err.exit().code(), 3);
        write(&mut store, "UPDATE jobs SET pr = 'web' WHERE key = 'u' AND plan = 'gr';
                           INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')")
            .unwrap();
    }

    #[test]
    fn jobs_pr_dependencies_refuses_moving_a_job_with_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        write(&mut store, WEB).unwrap();
        write(&mut store, "UPDATE jobs SET pr = 'web' WHERE key = 'u' AND plan = 'gr';
                           INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')")
            .unwrap();
        for sql in ["UPDATE jobs SET pr = 'pay' WHERE key = 'u' AND plan = 'gr'", "UPDATE jobs SET pr = 'pay' WHERE key = 'v' AND plan = 'gr'"] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::NeedsOtherPr, "{sql}: {err:?}");
        }
        // `job edit --pr --needs`: delete the Dependencies, move, insert again.
        write(&mut store, "DELETE FROM dependencies WHERE plan = 'gr';
                           UPDATE jobs SET pr = 'pay' WHERE plan = 'gr';
                           INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')")
            .unwrap();
    }

    #[test]
    fn jobs_locked_refuses_changing_a_locked_jobs_pr_or_review() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        write(&mut store, "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                             VALUES ('pr', 'web', '/src/web', 'main', 'waiting', 'cpd/pr/web', cpd_txn(), 't')")
            .unwrap();
        for sql in [
            "UPDATE jobs SET pr = 'web' WHERE plan = 'pr' AND key = 'z'",
            "UPDATE jobs SET review = 1 WHERE plan = 'di' AND key = 'x'",
            "UPDATE jobs SET title = 'New' WHERE plan = 'di' AND key = 'x'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::JobLocked, "{sql}: {err:?}");
        }
        write(&mut store, "UPDATE jobs SET review = 1, title = 'New' WHERE plan = 'pl' AND key = 'a'").unwrap();
    }

    #[test]
    fn plans_phase_and_jobs_status_refuse_illegal_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        for sql in [
            "UPDATE plans SET phase = 'pr', locked_at = 't' WHERE id = 'pl'",
            "UPDATE plans SET phase = 'plan', locked_at = NULL WHERE id = 'di'",
            "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'di' AND key = 'x'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
        }
    }

    #[test]
    fn events_and_context_records_stay_append_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        for (sql, message) in [
            ("UPDATE events SET text = 'y'", "append_only: events"),
            ("DELETE FROM events", "append_only: events"),
            ("UPDATE context_records SET text = 'y'", "append_only: context_records"),
            ("DELETE FROM context_records", "append_only: context_records"),
            ("DELETE FROM plans", "append_only: plans"),
            ("DELETE FROM prs", "append_only: prs"),
            ("DELETE FROM jobs", "append_only: jobs"),
            ("INSERT INTO events (txn, at, kind, context, text, data) VALUES (1, 't', 'pr-added', 'ctx', 'x', '{}')",
             "events.txn must be cpd_txn()"),
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(sqlite_message(&err), message, "{sql}");
        }
        // The six new Event kinds are accepted.
        write(&mut store, "INSERT INTO events (txn, at, kind, context, text, data) VALUES
                             (cpd_txn(), 't', 'pr-added', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-edited', 'ctx', 'x', '{}'),
                             (cpd_txn(), 't', 'pr-cancelled', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-started', 'ctx', 'x', '{}'),
                             (cpd_txn(), 't', 'pr-opened', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-merged', 'ctx', 'x', '{}')")
            .unwrap();
    }

    #[test]
    fn the_rebuilt_tables_history_triggers_list_their_new_columns() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        write(&mut store, "UPDATE jobs SET review = 1 WHERE plan = 'pl' AND key = 'a';
                           UPDATE plans SET title = 'G2' WHERE id = 'gr'")
            .unwrap();
        let after = rows(&mut store, "SELECT tbl, after FROM history WHERE op = 'update' ORDER BY id");
        let job: Value = serde_json::from_str(after[0][1].as_str().unwrap()).unwrap();
        assert_eq!((job["pr"].clone(), job["review"].clone()), (Value::from("my-repo-git"), Value::from(1)));
        let plan: Value = serde_json::from_str(after[1][1].as_str().unwrap()).unwrap();
        assert!(plan.get("repo").is_none() && plan["title"] == "G2", "{plan}");
    }
}
