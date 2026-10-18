//! The schema: migrations, the generated history triggers and the schema hash
//! (contract §4.3–§4.5).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Result, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::context::legacy;
use crate::home::CpdHome;
use crate::ids::PrKey;

/// A migration: applied once, in order, inside the migrating transaction. It
/// gets `$CPD_HOME` for the files an earlier version left there (migration 7).
pub(super) type Migration = fn(&Connection, &CpdHome) -> Result<()>;

/// Migrations `1..=N`, in order; `MIGRATIONS[i]` takes the schema to version `i + 1`.
pub(super) const MIGRATIONS: &[Migration] = &[
    |conn, _| migration_1(conn),
    |conn, _| migration_2(conn),
    |conn, _| migration_3(conn),
    |conn, _| migration_4(conn),
    |conn, _| migration_5(conn),
    |conn, _| migration_6(conn),
    migration_7,
    |conn, _| migration_8(conn),
    |conn, _| migration_9(conn),
    |conn, _| migration_10(conn),
    |conn, _| migration_11(conn),
    |conn, _| migration_12(conn),
];

/// The first migration that imports the old git stores of the Contexts; the
/// code that applies it moves the folders afterwards (`store/mod.rs`).
pub(super) const IMPORTS_OLD_STORES: usize = 7;

/// The schema hash of each version (§4.3): `SCHEMA_HASHES[i]` is version `i + 1`'s.
/// A test asserts that migrating an empty database gives each of them.
pub(super) const SCHEMA_HASHES: &[&str] = &[
    "0c73adb58e9a2027348333015d1aaf952d3bc3b0923ea27bcd41cac1079dcced",
    "37449f2445f3d621405857abab17a5f3760382270a1b6a55f46a116792998646",
    "c4529a6f3cd787333b8115bade42de35504eb4902773945cfbde80ba93acfc5f",
    "62a082772ccbcd0c989f5181e3b934ba96a3ab3f1d9db81cf8e39cef5ef89579",
    "c4ac18dd07a1a89318a0ef2191ef37a3f78c758dc7ccf2ea62d17c7544a4f672",
    "a1b657a955520d2dd6dd71e143cee1fd8aba88fafc9284a7e8ab89dfc05c82af",
    "6377d30a1c0e58581cb8576410c5187c6cfcb48eacf4ec193e5b24d0eacdea77",
    "21897859b666aff79e8c16dcb58e9ad2f7ddde4c880fa019738e2904ca28009d",
    "3ea4336e93cb740321d1c5c989863c2e018d1348e4e2a22a84feee158f5bee98",
    "2f8e238fbc3b0d5f768fe3c5e6c118b26eda5581d93841449563979a51144168",
    "d2623567062d0367ddcf3dcd0b96c6d7e2b45d447e3b841f77d0249129736277",
    "ae8ea2361df62ed1a89912b6e39227f4afd043475f27ac7faf78ce847e208cbb",
];

const _: () = assert!(SCHEMA_HASHES.len() == MIGRATIONS.len(), "one schema hash per migration");

/// The schema version this binary writes: `N`.
pub const VERSION: i64 = MIGRATIONS.len() as i64;

/// The tables whose rows are copied into `history` by generated triggers (§4.4),
/// at the current version. `dependencies` and `pr_needs` get no generated
/// `_no_delete`: their migrations write their own (a draft Job's Dependency,
/// and a PR's need before the lock, may be deleted) and their `_hist_del`.
/// `pull_requests` gets no `_no_delete` either: its migration writes its
/// `_hist_del` and `pull_requests_owned` (an unowned row may be deleted).
pub const HISTORY_TABLES: &[&str] = &[
    "contexts",
    "context_records",
    "plans",
    "pull_requests",
    "prs",
    "pr_needs",
    "plan_reviews",
    "jobs",
    "dependencies",
    "attempts",
    "escalations",
    "sessions",
    "inbox_items",
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

/// The line of `0004.sql` that splits it into the part before the PRs are
/// filled and the part after.
const FILL_PRS: &str = "\n-- @fill-prs\n";

/// Plans across repos (§4.5, R34 A6, B2), steps 2 to 4; step 1 (foreign keys
/// off, `legacy_alter_table` on) and step 5 (`foreign_key_check`) are the
/// caller's, around the migrating transaction. Needs the migration's `txns`
/// row: each migrated PR's `created_txn` and history row name it.
fn migration_4(conn: &Connection) -> Result<()> {
    let (tables, rebuild) = include_str!("migrations/0004.sql")
        .split_once(FILL_PRS)
        .expect("0004.sql has its `-- @fill-prs` line");
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

/// Reopening a Plan (§4.5, hs-d118): `plans_phase` allows `pr>dispatch` and
/// `done>dispatch`, `prs_locked` allows the legacy Lead Branch of a merged PR to
/// move to `cpd/<plan>/<key>`. No table changes.
fn migration_5(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0005.sql"))
}

/// The line of `0006.sql` that splits it into the part that creates
/// `pull_requests` and the part that fills it and rebuilds `prs`.
const HISTORY: &str = "\n-- @history\n";

/// The PR poller (§4.5 migration 6, D5, D6), steps 2 to 5; step 1 and step 6
/// are the caller's, as for migration 4. `pull_requests` gets its history
/// triggers before the back-fill, so each back-filled row has an `insert`
/// history row in the migration's `txns` row; the rebuild of `prs` writes
/// none, and its history triggers are generated after it.
fn migration_6(conn: &Connection) -> Result<()> {
    let (create, rebuild) = include_str!("migrations/0006.sql")
        .split_once(HISTORY)
        .expect("0006.sql has its `-- @history` line");
    conn.execute_batch(create)?;
    create_history_triggers(conn, "pull_requests", false)?;
    conn.execute_batch(rebuild)?;
    create_history_triggers(conn, "prs", true)
}

/// Context records only in the database (§4.5 migration 0007, hs-a1): drops
/// `contexts.sync_failing` (its history triggers are generated again), then
/// imports every Context and record of `$CPD_HOME/contexts/<id>/` the database
/// does not hold yet. A folder that is not a valid store fails the migration.
/// The folders are moved after the commit, by the caller.
fn migration_7(conn: &Connection, home: &CpdHome) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0007.sql"))?;
    create_history_triggers(conn, "contexts", true)?;
    import_old_stores(conn, &home.old_contexts_dir())
}

/// The Plan phase `pr` is called `deploy` (§4.5 migration 0008, hs-d76): `plans`
/// is rebuilt, `plans_phase` and the `plan-phase` Events are rewritten; the
/// history triggers of `plans` are generated again.
fn migration_8(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0008.sql"))?;
    create_history_triggers(conn, "plans", true)
}

/// The Event kind `session-message` (§4.5 migration 0009, hs-d133): `events` is
/// rebuilt with the kind in its check. No history triggers: `events` has none.
fn migration_9(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0009.sql"))
}

/// The Manager role and the `brief` record (§4.5 migration 0010, hs-d92, hs-d129):
/// `sessions`, `context_records` and `contexts` are rebuilt, their history
/// triggers generated again, and every grounded Lead Session (a Lead with no
/// Plan) archived with one `session-archived` Event each, in the migration's
/// `txns` row, as `session archive` would write it.
fn migration_10(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0010.sql"))?;
    for table in ["sessions", "context_records", "contexts"] {
        create_history_triggers(conn, table, true)?;
    }
    conn.execute_batch(
        "INSERT INTO events (txn, at, kind, context, session, text, data)
           SELECT t.id, t.at, 'session-archived', s.context, s.id, 'Session ' || s.name || ' archived.', '{\"archived\":true}'
           FROM sessions s, txns t
           WHERE t.id = cpd_txn() AND s.role = 'lead' AND s.plan IS NULL AND s.archived_at IS NULL
           ORDER BY s.created_at, s.id;
         UPDATE sessions SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn())
           WHERE role = 'lead' AND plan IS NULL AND archived_at IS NULL;",
    )
}

/// The import of migration 0007, with the row values and Events the engine's
/// earlier `import_all` wrote: a `contexts` row and `context-created` for a
/// Context the database lacks, and for each record whose `(context, id)` it
/// lacks the row and `context-record`. Idempotent. The Events are written in
/// SQL: they belong to this migration's `txns` row, whatever the Event writer
/// of later versions does.
fn import_old_stores(conn: &Connection, dir: &Path) -> Result<()> {
    let refused = |message: String| rusqlite::Error::UserFunctionError(message.into());
    let event = |kind: &str, context: &str, text: String, data: Value| {
        conn.execute(
            "INSERT INTO events (txn, at, kind, context, text, data)
             SELECT id, at, ?1, ?2, ?3, ?4 FROM txns WHERE id = cpd_txn()",
            params![kind, context, text, data.to_string()],
        )
    };
    for folder in legacy::folders(dir).map_err(refused)? {
        let store = legacy::read(&folder).map_err(refused)?;
        let manifest = &store.manifest;
        let id = manifest.id.as_str();
        let added = conn.execute(
            "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO NOTHING",
            params![id, manifest.title, manifest.jira, manifest.created_at, manifest.created_on.as_str()],
        )?;
        if added == 1 {
            event(
                "context-created",
                id,
                format!("Context {id} \"{}\" exists. Next: cpd context show --context {id}", manifest.title),
                json!({"title": manifest.title, "jira": manifest.jira, "created_on": manifest.created_on}),
            )?;
        }
        for record in &store.records {
            // Looked up first: a conflicting insert would still use up an index position.
            let held = conn
                .query_row("SELECT 1 FROM context_records WHERE context = ?1 AND id = ?2", params![id, record.id.to_string()], |_| Ok(()))
                .optional()?
                .is_some();
            if held || !record.insert(conn, &manifest.id)? {
                continue;
            }
            let seq = conn.last_insert_rowid();
            let snippet: String = record.text.chars().take(80).map(|c| if c.is_control() { ' ' } else { c }).collect();
            event(
                "context-record",
                id,
                format!(
                    "{} {} imported in Context {id}: {snippet}. Next: cpd context show --context {id} --since {}",
                    record.kind().as_str(),
                    record.id,
                    seq - 1
                ),
                json!({"record": record.id, "kind": record.kind().as_str(), "machine": record.machine(), "seq": seq}),
            )?;
        }
    }
    Ok(())
}

/// The PR poll's mergeable state (§4.5 migration 0011, hs-d188): two columns of
/// `pull_requests`, whose history triggers are dropped and written again to
/// list every column (`0011.sql` drops them and writes `_hist_del`; the other
/// two are generated).
fn migration_11(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0011.sql"))?;
    create_history_triggers(conn, "pull_requests", false)
}

/// The Inbox (§4.5 migration 12, hs-d168): `inbox_items` and `events` rebuilt
/// with the four `inbox-*` kinds (`0012.sql`).
fn migration_12(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0012.sql"))?;
    create_history_triggers(conn, "inbox_items", true)
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

/// The key of the PR migration 4 makes from a Plan (§4.5): the base name of
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
    /// `Store::open` sets around them (§4.5 migration 4, step 1), in a
    /// `$CPD_HOME` that holds no old Context store.
    fn at_version(conn: &Connection, n: usize) {
        let home = CpdHome::resolve(Some("/nonexistent/cpd".into()), None).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
        for migration in &MIGRATIONS[..n] {
            migration(conn, &home).unwrap();
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
           VALUES ('pay', 'ctx', 'Pay', '/repo', 'main', 'cpd/pay', '/wt', 'pr', 'l-pay', 7, 'https://github.com/acme/pay/pull/7',
                   '2026-01-05T00:00:00.000Z', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');";

    #[test]
    fn a_version_1_database_migrates_and_its_pr_gets_a_row_titled_like_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        database_at(&home, 1, VERSION_1_PLAN);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        type Row = (String, i64, String, String, String, String);
        let (version, row, migrations): (i64, Row, i64) = store
            .read(|c| {
                Ok((
                    c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                    c.query_row(
                        "SELECT r.key, r.pr_number, p.repo, p.title, p.review_decision, p.checks
                           FROM prs r JOIN pull_requests p ON p.id = r.pull_request WHERE r.plan = 'pay'",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                    )?,
                    c.query_row("SELECT count(*) FROM txns WHERE command = 'migrate'", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(version, VERSION);
        let expected: Row = ("repo".into(), 7, "acme/pay".into(), "Pay".into(), "none".into(), "none".into());
        assert_eq!(row, expected, "no title was polled: the Plan's title, and no review or checks");
        assert_eq!(migrations, 2, "the old migration's row and the new one");
    }

    #[test]
    fn pull_requests_history_lists_every_column_and_their_checks_hold() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        database_at(&home, 1, VERSION_1_PLAN);
        let mut store = Store::open(&home, 5000).unwrap();

        store
            .write("test", |tx| {
                tx.execute(
                    "UPDATE pull_requests SET title = 'T', review_decision = 'approved', checks = 'passed', draft = 1, mergeable = 'conflicting', merge_state = 'dirty' WHERE number = 7",
                    [],
                )?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
        let after: String = store
            .read(|c| Ok(c.query_row("SELECT after FROM history WHERE tbl = 'pull_requests' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))?))
            .unwrap();
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["title"], "T");
        assert_eq!(after["review_decision"], "approved");
        assert_eq!(after["checks"], "passed");
        assert_eq!(after["draft"], 1);
        assert_eq!((&after["mergeable"], &after["merge_state"]), (&Value::from("conflicting"), &Value::from("dirty")));

        for bad in ["title = ''", "review_decision = 'maybe'", "checks = 'green'", "state = 'merged'", "repo = 'x'", "number = 0", "draft = 2", "mergeable = 'maybe'", "merge_state = 'ready'"] {
            let sql = format!("UPDATE pull_requests SET {bad} WHERE number = 7");
            let err = store.write("test", |tx| Ok::<(), CpdError>({ tx.execute(&sql, [])?; })).unwrap_err();
            assert!(err.to_string().contains("CHECK") || format!("{err:?}").contains("CHECK"), "{bad}: {err:?}");
        }
    }

    // ---------- migration 3 ----------

    /// A version 2 database with Events, as a real `cpd.db` is; the Event ids are 1, 2 and 5
    /// (not contiguous), so the next one must be above 5.
    fn version_2_with_events(home: &CpdHome) {
        let events: String = [1, 2, 5]
            .iter()
            .map(|id| {
                format!(
                    "INSERT INTO events (id, txn, at, kind, context, plan, text, data)
                       VALUES ({id}, 1, '2026-01-01T00:00:00.000Z', 'plan-created', 'ctx', 'pay', 'event {id}', '{{}}');"
                )
            })
            .collect();
        database_at(home, 2, &format!("{VERSION_1_PLAN}\n{events}"));
    }

    const NEW_KINDS: [&str; 5] =
        ["context-renamed", "plan-renamed", "context-archived", "plan-archived", "session-archived"];

    #[test]
    fn a_version_2_database_migrates_through_3_keeping_every_row_and_event_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_2_with_events(&home);

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
        assert_eq!(version, VERSION);
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
        version_2_with_events(&home);
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
        version_2_with_events(&home);
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

    // ---------- migration 4 ----------

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
                7, 'https://github.com/acme/pay/pull/7', '2026-01-03T00:00:00.004Z', '{{\"checks\":{{\"ci\":\"SUCCESS\"}},\"reviewIds\":[\"r1\"],\"commentIds\":[]}}',
                '2026-01-04T00:00:00.004Z', 'Open PR', 'approved', 'passed', '2026-01-03T00:00:00.004Z', '2026-01-01T00:00:00.004Z', '2026-01-02T00:00:00.004Z');
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                                pr_number, pr_url, pr_opened_at, created_at, locked_at, done_at) VALUES
               ('dn', 'ctx', 'Done', '/src/web/', 'develop', 'cpd/dn', '/src/web.dn', 'done', 'l-dn',
                9, 'https://github.com/acme/long/pull/9', '2026-01-03T00:00:00.005Z', '2026-01-01T00:00:00.005Z', '2026-01-02T00:00:00.005Z', '2026-01-05T00:00:00.005Z');
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
    fn a_version_2_database_migrates_through_3_4_and_5_to_6_with_one_pr_per_plan() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated(&dir);
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        let migrate = count(&mut store, "SELECT max(id) FROM txns WHERE command = 'migrate'");
        assert_eq!(migrate, 2);

        let prs = rows(
            &mut store,
            "SELECT plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                    pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at
             FROM prs ORDER BY created_at",
        );
        let n = Value::Null;
        assert_eq!(
            prs,
            vec![
                serde_json::json!(["gr", "pay", "/src/pay", "main", "waiting", "cpd/gr", n, n, n, n, n, n, n,
                                   migrate, "2026-01-01T00:00:00.001Z", n, n]),
                serde_json::json!(["pl", "my-repo-git", "/src/My Repo.git", "main", "waiting", "cpd/pl", n, n, n, n, n, n, n,
                                   migrate, "2026-01-01T00:00:00.002Z", n, n]),
                serde_json::json!(["di", "pr-2fa", "/src/2fa", "main", "dispatch", "cpd/di", "/src/2fa.di", n, n, n, n, n, n,
                                   migrate, "2026-01-01T00:00:00.003Z", "2026-01-02T00:00:00.003Z", n]),
                serde_json::json!(["pr", "a-very-long-repository-n", "/src/a-very-long-repository-name-indeed", "main", "open",
                                   "cpd/pr", "/src/long.pr", 7, "https://github.com/acme/pay/pull/7", "2026-01-03T00:00:00.004Z",
                                   2, "2026-01-03T00:00:00.004Z", n,
                                   migrate, "2026-01-01T00:00:00.004Z", "2026-01-02T00:00:00.004Z", n]),
                serde_json::json!(["dn", "web", "/src/web/", "develop", "merged", "cpd/dn", "/src/web.dn", 9,
                                   "https://github.com/acme/long/pull/9", "2026-01-03T00:00:00.005Z", 1, n, n,
                                   migrate, "2026-01-01T00:00:00.005Z", "2026-01-02T00:00:00.005Z", "2026-01-05T00:00:00.005Z"]),
            ]
        );
        // One pull request row for each PR with a number, in Plan and key order (migration 6).
        assert_eq!(
            rows(&mut store, "SELECT id, repo, number, title, draft, state, review_decision, checks, created_at, merged_at, changed_at FROM pull_requests ORDER BY id"),
            vec![
                serde_json::json!([1, "acme/long", 9, "Done", 0, "merged", "none", "none", "2026-01-03T00:00:00.005Z", "2026-01-05T00:00:00.005Z", "2026-01-03T00:00:00.005Z"]),
                serde_json::json!([2, "acme/pay", 7, "Open PR", 0, "open", "approved", "passed", "2026-01-03T00:00:00.004Z", n, "2026-01-03T00:00:00.004Z"]),
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
                serde_json::json!(["pr", "deploy", "2026-01-02T00:00:00.004Z", n]),
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
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_table_info('plans')"), 10);
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_table_info('context_records') WHERE name = 'plan_repo'"), 0);

        // History: one insert per migrated PR (migration 4) and one per back-filled pull request
        // (migration 6) in the migration's txn; the copies write none.
        let mut expected: Vec<Value> = ["di", "dn", "gr", "pl", "pr"]
            .iter()
            .map(|plan| {
                let key = prs.iter().find(|p| p[0] == *plan).unwrap()[1].clone();
                serde_json::json!([2, "prs", serde_json::json!([plan, key]).to_string(), "insert"])
            })
            .collect();
        expected.push(serde_json::json!([2, "pull_requests", "[1]", "insert"]));
        expected.push(serde_json::json!([2, "pull_requests", "[2]", "insert"]));
        assert_eq!(rows(&mut store, "SELECT txn, tbl, pk, op FROM history WHERE txn = 2 ORDER BY id"), expected);

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

    /// A version 3 database as tree-menu's migration 3 leaves it (the User's
    /// real case): archived rows and the Events of the archive kinds.
    fn version_3(home: &CpdHome) {
        let sql = format!(
            "INSERT INTO contexts (id, title, created_at, created_on, archived_at) VALUES ('ctx', 'Ctx', '{T}', 'm1', '2026-01-06T00:00:00.000Z');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, archived_at) VALUES
               ('l-ar', 'lead', 'ctx', 'ar', 'lead', '/src', 'O3', 'stopped', '{T}', '{T}', '2026-01-06T00:00:00.001Z'),
               ('l-li', 'lead', 'ctx', 'li', 'lead', '/src', 'O3', 'idle', '{T}', '{T}', NULL);
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, archived_at) VALUES
               ('ar', 'ctx', 'Archived', '/src/pay', 'main', 'cpd/ar', NULL, 'plan', 'l-ar', '2026-01-01T00:00:00.001Z', '2026-01-06T00:00:00.002Z'),
               ('li', 'ctx', 'Live', '/src/web', 'main', 'cpd/li', NULL, 'grounding', 'l-li', '2026-01-01T00:00:00.002Z', NULL);
             INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES
               (1, '{T}', 'plan-renamed', 'ctx', 'ar', NULL, 'x', '{{}}'),
               (1, '{T}', 'plan-archived', 'ctx', 'ar', NULL, 'x', '{{}}'),
               (1, '{T}', 'session-archived', 'ctx', NULL, 'l-ar', 'x', '{{}}'),
               (1, '{T}', 'context-archived', 'ctx', NULL, NULL, 'x', '{{}}');"
        );
        database_at(home, 3, &sql);
    }

    #[test]
    fn a_tree_menu_version_3_database_migrates_to_6_keeping_archived_at() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_3(&home);
        // The starting schema is exactly the one tree-menu's migration 3 installs.
        assert_eq!(schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[2]);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        let n = Value::Null;
        assert_eq!(
            rows(&mut store, "SELECT id, phase, archived_at FROM plans ORDER BY id"),
            vec![
                serde_json::json!(["ar", "plan", "2026-01-06T00:00:00.002Z"]),
                serde_json::json!(["li", "grounding", n]),
            ]
        );
        assert_eq!(rows(&mut store, "SELECT id, archived_at FROM contexts"), vec![serde_json::json!(["ctx", "2026-01-06T00:00:00.000Z"])]);
        assert_eq!(
            rows(&mut store, "SELECT id, archived_at FROM sessions ORDER BY id"),
            vec![serde_json::json!(["l-ar", "2026-01-06T00:00:00.001Z"]), serde_json::json!(["l-li", n])]
        );
        assert_eq!(
            rows(&mut store, "SELECT plan, key, repo, status, lead_branch FROM prs ORDER BY plan"),
            vec![
                serde_json::json!(["ar", "pay", "/src/pay", "waiting", "cpd/ar"]),
                serde_json::json!(["li", "web", "/src/web", "waiting", "cpd/li"]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT id, kind FROM events ORDER BY id"),
            vec![
                serde_json::json!([1, "plan-renamed"]),
                serde_json::json!([2, "plan-archived"]),
                serde_json::json!([3, "session-archived"]),
                serde_json::json!([4, "context-archived"]),
            ]
        );
        // Both sides' Event kinds are accepted, and a Plan's history lists archived_at.
        write(&mut store, "INSERT INTO events (txn, at, kind, context, text, data) VALUES
                             (cpd_txn(), 't', 'plan-archived', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-started', 'ctx', 'x', '{}');
                           UPDATE plans SET archived_at = NULL WHERE id = 'ar'")
            .unwrap();
        let after = rows(&mut store, "SELECT after FROM history WHERE tbl = 'plans' ORDER BY id DESC LIMIT 1");
        let plan: Value = serde_json::from_str(after[0][0].as_str().unwrap()).unwrap();
        assert!(plan.as_object().unwrap().contains_key("archived_at") && plan["archived_at"].is_null(), "{plan}");
        assert_eq!(count(&mut store, "SELECT max(id) FROM events"), 6);
    }

    // ---------- migration 5 ----------

    /// A version 4 database, as the User's real one: Plans in `dispatch`, `pr`
    /// and `done`, each PR on its legacy Lead Branch `cpd/<plan>`; `dn` has its
    /// PR merged, `pr` an open one.
    fn version_4(home: &CpdHome) {
        let sql = format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
               ('l-di', 'lead', 'ctx', 'di', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-pr', 'lead', 'ctx', 'pr', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-dn', 'lead', 'ctx', 'dn', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES
               ('di', 'ctx', 'Dispatch', 'dispatch', 'l-di', '{T}', '{T}'),
               ('pr', 'ctx', 'Open', 'pr', 'l-pr', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at) VALUES
               ('dn', 'ctx', 'Done', 'done', 'l-dn', '{T}', '{T}', '2026-01-05T00:00:00.000Z');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                              created_txn, created_at, started_at, merged_at) VALUES
               ('di', 'pay', '/src/pay', 'main', 'dispatch', 'cpd/di', '/src/pay.di', NULL, NULL, NULL, 1, '{T}', '{T}', NULL),
               ('pr', 'pay', '/src/pay', 'main', 'open', 'cpd/pr', '/src/pay.pr', 7, 'https://github.com/acme/pay/pull/7', '{T}', 1, '{T}', '{T}', NULL),
               ('dn', 'web', '/src/web', 'develop', 'merged', 'cpd/dn', '/src/web.dn', 9, 'https://github.com/acme/web/pull/9', '{T}', 1, '{T}', '{T}',
                '2026-01-05T00:00:00.000Z');
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, merged_sha) VALUES
               ('dn', 'web', 'w', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'def');"
        );
        database_at(home, 4, &sql);
    }

    /// `version_4`, migrated by `Store::open`.
    fn migrated_4(dir: &tempfile::TempDir) -> Store {
        let home = home(dir);
        version_4(&home);
        assert_eq!(schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[3]);
        let store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        store
    }

    #[test]
    fn a_version_4_database_with_a_done_plan_migrates_to_5_and_6_keeping_every_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated_4(&dir);
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        assert_eq!(
            rows(&mut store, "SELECT id, phase, done_at FROM plans ORDER BY id"),
            vec![
                serde_json::json!(["di", "dispatch", Value::Null]),
                serde_json::json!(["dn", "done", "2026-01-05T00:00:00.000Z"]),
                serde_json::json!(["pr", "deploy", Value::Null]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT plan, key, status, lead_branch FROM prs ORDER BY plan"),
            vec![
                serde_json::json!(["di", "pay", "dispatch", "cpd/di"]),
                serde_json::json!(["dn", "web", "merged", "cpd/dn"]),
                serde_json::json!(["pr", "pay", "open", "cpd/pr"]),
            ]
        );
        assert_eq!(count(&mut store, "SELECT count(*) FROM jobs"), 1);
        assert_eq!(count(&mut store, "SELECT count(*) FROM sessions"), 3);
    }

    #[test]
    fn plans_phase_allows_deploy_and_done_back_to_dispatch_and_refuses_the_other_backward_moves() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated_4(&dir);
        for sql in [
            "UPDATE plans SET phase = 'plan', locked_at = NULL, done_at = NULL WHERE id = 'dn'",
            "UPDATE plans SET phase = 'plan', locked_at = NULL WHERE id = 'pr'",
            "UPDATE plans SET phase = 'grounding', locked_at = NULL WHERE id = 'di'",
            "UPDATE plans SET phase = 'deploy', done_at = NULL WHERE id = 'dn'",
            "UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'di'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
        }
        // `done_at` must be cleared by the same update (the CHECK of §4.4).
        assert!(write(&mut store, "UPDATE plans SET phase = 'dispatch' WHERE id = 'dn'").is_err());
        write(&mut store, "UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'dn';
                           UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'pr'")
            .unwrap();
        assert_eq!(
            rows(&mut store, "SELECT id, phase, done_at FROM plans ORDER BY id"),
            vec![
                serde_json::json!(["di", "dispatch", Value::Null]),
                serde_json::json!(["dn", "dispatch", Value::Null]),
                serde_json::json!(["pr", "dispatch", Value::Null]),
            ]
        );
    }

    #[test]
    fn prs_locked_allows_only_the_legacy_lead_branch_rename_of_a_merged_pr() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated_4(&dir);
        for sql in [
            // another value
            "UPDATE prs SET lead_branch = 'cpd/dn/other' WHERE plan = 'dn'",
            "UPDATE prs SET lead_branch = 'cpd/dn/web/x' WHERE plan = 'dn'",
            "UPDATE prs SET lead_branch = 'cpd/pr/pay' WHERE plan = 'dn'",
            // the rename together with another guarded column
            "UPDATE prs SET lead_branch = 'cpd/dn/web', repo = '/src/other' WHERE plan = 'dn'",
            "UPDATE prs SET lead_branch = 'cpd/dn/web', base = 'main' WHERE plan = 'dn'",
            // the other guarded columns alone
            "UPDATE prs SET repo = '/src/other' WHERE plan = 'dn'",
            "UPDATE prs SET base = 'main' WHERE plan = 'dn'",
            // a PR that is not merged
            "UPDATE prs SET lead_branch = 'cpd/pr/pay' WHERE plan = 'pr'",
            "UPDATE prs SET lead_branch = 'cpd/di/pay' WHERE plan = 'di'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::PrLocked, "{sql}: {err:?}");
            assert_eq!(err.exit().code(), 3);
        }
        write(&mut store, "UPDATE prs SET lead_branch = 'cpd/dn/web' WHERE plan = 'dn'").unwrap();
        assert_eq!(rows(&mut store, "SELECT lead_branch FROM prs WHERE plan = 'dn'"), vec![serde_json::json!(["cpd/dn/web"])]);
        // And back is not allowed.
        let err = refused(&mut store, "UPDATE prs SET lead_branch = 'cpd/dn' WHERE plan = 'dn'");
        assert_eq!(err.code(), ErrorCode::PrLocked, "{err:?}");
    }

    // ---------- migration 6 ----------

    /// A version 5 database as plan-reopen's migration 5 leaves it: `op`'s PR is
    /// open on its legacy Lead Branch with a polled title, `dn`'s merged, `wa`'s
    /// waiting in a Plan in `dispatch`.
    fn version_5(home: &CpdHome) {
        let sql = format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
               ('l-op', 'lead', 'ctx', 'op', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-dn', 'lead', 'ctx', 'dn', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
               ('l-wa', 'lead', 'ctx', 'wa', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES
               ('op', 'ctx', 'Open', 'pr', 'l-op', '{T}', '{T}'),
               ('wa', 'ctx', 'Waiting', 'dispatch', 'l-wa', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at) VALUES
               ('dn', 'ctx', 'Done', 'done', 'l-dn', '{T}', '{T}', '2026-01-05T00:00:00.000Z');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                              pr_polled_at, pr_title, pr_review_decision, pr_checks, created_txn, created_at, started_at, merged_at) VALUES
               ('op', 'pay', '/src/pay', 'main', 'open', 'cpd/op', '/src/pay.op', 7, 'https://github.com/acme/pay/pull/7',
                '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z', 'Polled title', 'approved', 'failed', 1, '{T}', '{T}', NULL),
               ('dn', 'web', '/src/web', 'develop', 'merged', 'cpd/dn', '/src/web.dn', 9, 'https://github.com/acme/web/pull/9',
                '2026-01-02T00:00:00.000Z', NULL, NULL, NULL, NULL, 1, '{T}', '{T}', '2026-01-05T00:00:00.000Z'),
               ('wa', 'web', '/src/web', 'main', 'waiting', 'cpd/wa/web', NULL, NULL, NULL, NULL,
                NULL, NULL, NULL, NULL, 1, '{T}', NULL, NULL);"
        );
        database_at(home, 5, &sql);
    }

    /// The triggers on `prs`, `(name, sql)` by name.
    fn prs_triggers(conn: &Connection) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'prs' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_>>().unwrap()
    }

    /// `version_5`, migrated by `Store::open`, and the triggers on `prs` before.
    fn migrated_5(dir: &tempfile::TempDir) -> (Store, Vec<(String, String)>) {
        let home = home(dir);
        version_5(&home);
        let conn = Connection::open(home.db()).unwrap();
        assert_eq!(schema_hash(&conn).unwrap(), SCHEMA_HASHES[4]);
        let before = prs_triggers(&conn);
        drop(conn);
        let store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        (store, before)
    }

    #[test]
    fn a_version_5_database_migrates_to_6_with_the_pull_requests_back_filled() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = migrated_5(&dir);
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        let n = Value::Null;
        assert_eq!(
            rows(&mut store, "SELECT id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at
                              FROM pull_requests ORDER BY id"),
            vec![
                serde_json::json!([1, "acme/web", 9, "https://github.com/acme/web/pull/9", "Done", 0, "merged", "none", "none",
                                   "2026-01-02T00:00:00.000Z", "2026-01-05T00:00:00.000Z", "2026-01-02T00:00:00.000Z"]),
                serde_json::json!([2, "acme/pay", 7, "https://github.com/acme/pay/pull/7", "Polled title", 0, "open", "approved", "failed",
                                   "2026-01-03T00:00:00.000Z", n, "2026-01-03T00:00:00.000Z"]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT plan, key, status, lead_branch, pr_number, pull_request FROM prs ORDER BY plan"),
            vec![
                serde_json::json!(["dn", "web", "merged", "cpd/dn", 9, 1]),
                serde_json::json!(["op", "pay", "open", "cpd/op", 7, 2]),
                serde_json::json!(["wa", "web", "waiting", "cpd/wa/web", n, n]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT tbl, pk, op FROM history WHERE txn = 2 ORDER BY id"),
            vec![serde_json::json!(["pull_requests", "[1]", "insert"]), serde_json::json!(["pull_requests", "[2]", "insert"])]
        );
    }

    // ---------- migration 11: the PR poll's mergeable state ----------

    /// A version 10 database with two pull requests and no PR.
    fn version_10(home: &CpdHome) {
        database_at(
            home,
            10,
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at) VALUES
               ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'failed', '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z'),
               ('acme/web', 9, 'https://github.com/acme/web/pull/9', 'Web', 1, 'open', 'approved', 'passed', '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z');",
        );
    }

    #[test]
    fn a_version_10_database_migrates_to_11_with_its_rows_unknown_and_its_history_listing_both_columns() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_10(&home);
        assert_eq!(schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[9]);
        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        assert_eq!(
            rows(&mut store, "SELECT number, checks, mergeable, merge_state FROM pull_requests ORDER BY id"),
            vec![serde_json::json!([7, "failed", "unknown", "unknown"]), serde_json::json!([9, "passed", "unknown", "unknown"])]
        );
        assert_eq!(count(&mut store, "SELECT count(*) FROM history WHERE tbl = 'pull_requests'"), 2, "the migration wrote no history row");

        // An update, an insert and a delete: each history row lists both new columns.
        write(&mut store, "UPDATE pull_requests SET mergeable = 'conflicting', merge_state = 'dirty' WHERE number = 7").unwrap();
        write(
            &mut store,
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
             VALUES ('acme/new', 1, 'https://github.com/acme/new/pull/1', 'New', 0, 'open', 'none', 'none', 't', 't')",
        )
        .unwrap();
        write(&mut store, "DELETE FROM pull_requests WHERE number = 9").unwrap();
        let history = rows(&mut store, "SELECT op, before, after FROM history WHERE tbl = 'pull_requests' AND id > 2 ORDER BY id");
        let side = |row: &Value, n: usize| -> Value {
            row[n].as_str().map_or(Value::Null, |text| serde_json::from_str(text).unwrap())
        };
        let columns = |v: &Value| (v["mergeable"].clone(), v["merge_state"].clone());
        let unknown = || (Value::from("unknown"), Value::from("unknown"));
        assert_eq!(history.len(), 3);
        assert_eq!(history[0][0], "update");
        assert_eq!((columns(&side(&history[0], 1)), columns(&side(&history[0], 2))), (unknown(), (Value::from("conflicting"), Value::from("dirty"))));
        assert_eq!((history[1][0].as_str(), columns(&side(&history[1], 2))), (Some("insert"), unknown()));
        assert_eq!((history[2][0].as_str(), columns(&side(&history[2], 1))), (Some("delete"), unknown()));

        // `pull_requests_owned` is kept and `pull_requests` still gets no `_no_delete`.
        let triggers = rows(&mut store, "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'pull_requests' ORDER BY name");
        assert_eq!(
            triggers,
            ["pull_requests_hist_del", "pull_requests_hist_ins", "pull_requests_hist_upd", "pull_requests_owned"].map(|n| serde_json::json!([n]))
        );
    }

    #[test]
    fn the_checks_of_mergeable_and_merge_state_refuse_a_value_outside_their_sets() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_10(&home);
        let mut store = Store::open(&home, 5000).unwrap();
        for ok in ["mergeable = 'mergeable'", "mergeable = 'conflicting'", "merge_state = 'has_hooks'", "merge_state = 'behind'"] {
            write(&mut store, &format!("UPDATE pull_requests SET {ok} WHERE number = 7")).unwrap();
        }
        for bad in ["mergeable = 'MERGEABLE'", "mergeable = 'dirty'", "mergeable = NULL", "merge_state = 'DIRTY'", "merge_state = 'conflicting'", "merge_state = NULL"] {
            let err = refused(&mut store, &format!("UPDATE pull_requests SET {bad} WHERE number = 7"));
            assert!(format!("{err:?}").contains("CHECK") || format!("{err:?}").contains("NOT NULL"), "{bad}: {err:?}");
        }
    }

    #[test]
    fn migration_6_keeps_migration_5s_triggers_on_prs() {
        let dir = tempfile::tempdir().unwrap();
        let (store, before) = migrated_5(&dir);
        drop(store);
        let after = prs_triggers(&Connection::open(home(&dir).db()).unwrap());
        let names = |t: &[(String, String)]| t.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&after), names(&before), "the same triggers on prs");
        for ((name, old), (_, new)) in before.iter().zip(&after) {
            // The history triggers list the rebuilt table's columns; every other keeps its text.
            if name != "prs_hist_ins" && name != "prs_hist_upd" {
                assert_eq!(new, old, "{name}");
            }
        }
        let locked = &after.iter().find(|(name, _)| name == "prs_locked").unwrap().1;
        assert!(locked.contains("OLD.status = 'merged'"), "migration 5's prs_locked: {locked}");
    }

    #[test]
    fn after_migration_6_prs_locked_still_allows_only_the_legacy_lead_branch_rename_of_a_merged_pr() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = migrated_5(&dir);
        for sql in [
            "UPDATE prs SET lead_branch = 'cpd/op/pay' WHERE plan = 'op'",
            "UPDATE prs SET lead_branch = 'cpd/wa' WHERE plan = 'wa'",
            "UPDATE prs SET lead_branch = 'cpd/dn/web', base = 'main' WHERE plan = 'dn'",
            "UPDATE prs SET repo = '/src/other' WHERE plan = 'dn'",
        ] {
            let err = refused(&mut store, sql);
            assert_eq!(err.code(), ErrorCode::PrLocked, "{sql}: {err:?}");
        }
        write(&mut store, "UPDATE prs SET lead_branch = 'cpd/dn/web' WHERE plan = 'dn'").unwrap();
        assert_eq!(rows(&mut store, "SELECT lead_branch FROM prs WHERE plan = 'dn'"), vec![serde_json::json!(["cpd/dn/web"])]);
    }

    // ---------- migration 12 (the Inbox) ----------

    /// A version 6 database with a Context and two Events.
    fn version_6(home: &CpdHome) {
        let sql = format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO events (txn, at, kind, context, text, data) VALUES
               (1, '{T}', 'context-created', 'ctx', 'x', '{{}}'), (1, '{T}', 'pr-merged', 'ctx', 'x', '{{}}');"
        );
        database_at(home, 6, &sql);
    }

    #[test]
    fn a_version_6_database_migrates_to_12_with_inbox_items_and_the_four_inbox_event_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_6(&home);
        assert_eq!(schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[5]);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        assert_eq!(schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[VERSION as usize - 1]);
        assert_eq!(
            rows(&mut store, "SELECT id, kind FROM events ORDER BY id"),
            vec![serde_json::json!([1, "context-created"]), serde_json::json!([2, "pr-merged"])]
        );
        assert_eq!(count(&mut store, "SELECT count(*) FROM inbox_items"), 0);

        // The four kinds are accepted, an unknown one is not, and ids go on after the kept ones.
        write(&mut store, "INSERT INTO events (txn, at, kind, context, text, data) VALUES
                             (cpd_txn(), 't', 'inbox-opened', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'inbox-closed', 'ctx', 'x', '{}'),
                             (cpd_txn(), 't', 'inbox-snoozed', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'inbox-dismissed', 'ctx', 'x', '{}')")
            .unwrap();
        assert_eq!(count(&mut store, "SELECT max(id) FROM events"), 6);
        assert!(write(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'inbox-other', 'x', '{}')").is_err());
        // The rebuilt table kept its triggers.
        for sql in ["UPDATE events SET text = 'y'", "DELETE FROM events"] {
            assert_eq!(sqlite_message(&refused(&mut store, sql)), "append_only: events", "{sql}");
        }
    }

    #[test]
    fn inbox_items_keep_one_open_item_per_identity_and_write_history_and_never_delete() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_6(&home);
        let mut store = Store::open(&home, 5000).unwrap();
        let item = |urgency: &str| {
            format!("INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('dialog', '{urgency}', 'session:a', 'l', 't');")
        };
        write(&mut store, &item("red")).unwrap();
        // The same identity cannot be open twice; another urgency is another identity.
        assert!(write(&mut store, &item("red")).is_err());
        write(&mut store, &item("magenta")).unwrap();
        // Once closed, the identity is free again.
        write(&mut store, "UPDATE inbox_items SET closed_at = 't' WHERE urgency = 'red'").unwrap();
        write(&mut store, &item("red")).unwrap();
        assert_eq!(
            rows(&mut store, "SELECT pk, op FROM history WHERE tbl = 'inbox_items' ORDER BY id"),
            vec![
                serde_json::json!(["[1]", "insert"]),
                serde_json::json!(["[2]", "insert"]),
                serde_json::json!(["[1]", "update"]),
                serde_json::json!(["[3]", "insert"]),
            ]
        );
        assert_eq!(sqlite_message(&refused(&mut store, "DELETE FROM inbox_items")), "append_only: inbox_items");
        // A kind the poll adds later is refused until its migration; `engine-down` is never stored.
        assert!(write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('engine-down', 'red', 'engine', 'l', 't')").is_err());
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
        assert_eq!(hash(&fresh_home), SCHEMA_HASHES[VERSION as usize - 1]);
    }

    // ---------- migration 7: the old Context stores ----------

    const NOTE_1: &str = r#"{"v":1,"id":"m1-n1","kind":"note","at":"2026-01-02T00:00:00.000Z","machine":"m1","by":{"session":null,"plan":null},"text":"Indexed"}"#;
    const NOTE_2: &str = r#"{"v":1,"id":"m1-n2","kind":"note","at":"2026-01-03T00:00:00.000Z","machine":"m1","by":{"session":null,"plan":null},"text":"Not indexed"}"#;
    const PLAN_1: &str = r#"{"v":1,"id":"m1-p1","kind":"plan","at":"2026-01-04T00:00:00.000Z","machine":"m1","by":{"session":null,"plan":null},"text":"Pay","plan":{"id":"pay","machine":"m1","repo":"/r/pay","jira":"PAY-1"}}"#;
    const OTHER_1: &str = r#"{"v":1,"id":"m2-n1","kind":"note","at":"2026-01-05T00:00:00.000Z","machine":"m2","by":{"session":null,"plan":null},"text":"From m2"}"#;

    /// `contexts/<id>/` as an earlier version wrote it, with its record files.
    fn old_store(home: &CpdHome, id: &str, files: &[(&str, &[&str])]) {
        let dir = home.old_contexts_dir().join(id);
        std::fs::create_dir_all(dir.join("records")).unwrap();
        std::fs::write(
            dir.join("context.json"),
            format!(r#"{{"v":1,"id":"{id}","title":"Title of {id}","jira":"ABC-1","created_at":"{T}","created_on":"m1"}}"#),
        )
        .unwrap();
        for (machine, lines) in files {
            let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
            std::fs::write(dir.join("records").join(format!("{machine}.jsonl")), text).unwrap();
        }
    }

    /// A version-6 database whose index holds Context `known` with its first note only.
    fn version_6_with_known_context(home: &CpdHome) {
        database_at(
            home,
            6,
            &format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('known', 'Known', '{T}', 'm1');
                 UPDATE contexts SET lead_session = NULL WHERE id = 'known';
                 INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('known', 'm1-n1', 'note', '{T}', 'm1', 'Indexed');"
            ),
        );
    }

    #[test]
    fn migration_7_imports_what_the_index_lacks_and_archives_the_folders_and_a_reopen_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_6_with_known_context(&home);
        old_store(&home, "known", &[("m1", &[NOTE_1, NOTE_2, PLAN_1]), ("m2", &[OTHER_1])]);
        old_store(&home, "fresh", &[("m1", &[NOTE_1])]);
        std::fs::write(home.old_contexts_dir().join("README"), "not a store").unwrap();

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_table_info('contexts') WHERE name = 'sync_failing'"), 0);
        // The Context the index had keeps its row; the index's own record keeps its seq.
        assert_eq!(
            rows(&mut store, "SELECT id, title, jira FROM contexts ORDER BY id"),
            vec![serde_json::json!(["fresh", "Title of fresh", "ABC-1"]), serde_json::json!(["known", "Known", null])]
        );
        assert_eq!(
            rows(&mut store, "SELECT context, id, seq FROM context_records ORDER BY seq"),
            vec![
                serde_json::json!(["known", "m1-n1", 1]),
                serde_json::json!(["fresh", "m1-n1", 2]),
                serde_json::json!(["known", "m1-n2", 3]),
                serde_json::json!(["known", "m1-p1", 4]),
                serde_json::json!(["known", "m2-n1", 5]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT plan_id, plan_machine, plan_jira FROM context_records WHERE id = 'm1-p1'"),
            vec![serde_json::json!(["pay", "m1", "PAY-1"])]
        );
        // `context-created` for the new Context, `context-record` for each imported record, in the migration's txn.
        assert_eq!(
            rows(&mut store, "SELECT kind, context, json_extract(data, '$.record'), txn FROM events ORDER BY id"),
            vec![
                serde_json::json!(["context-created", "fresh", null, 2]),
                serde_json::json!(["context-record", "fresh", "m1-n1", 2]),
                serde_json::json!(["context-record", "known", "m1-n2", 2]),
                serde_json::json!(["context-record", "known", "m1-p1", 2]),
                serde_json::json!(["context-record", "known", "m2-n1", 2]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT text FROM events WHERE kind = 'context-created'"),
            vec![serde_json::json!(["Context fresh \"Title of fresh\" exists. Next: cpd context show --context fresh"])]
        );

        // The folders are archived, nothing else is touched.
        let archived = home.archived_contexts_dir();
        for id in ["known", "fresh"] {
            assert!(!home.old_contexts_dir().join(id).exists(), "{id} is still in contexts/");
            assert!(archived.join(id).join("context.json").is_file(), "{id} is not archived");
        }
        assert!(archived.join("known/records/m2.jsonl").is_file());
        assert!(home.old_contexts_dir().join("README").is_file(), "a file that is no store stays");

        // A second open changes nothing.
        let before = (
            count(&mut store, "SELECT count(*) FROM context_records"),
            count(&mut store, "SELECT max(id) FROM events"),
            count(&mut store, "SELECT max(id) FROM txns"),
        );
        drop(store);
        let mut store = Store::open(&home, 5000).unwrap();
        let after = (
            count(&mut store, "SELECT count(*) FROM context_records"),
            count(&mut store, "SELECT max(id) FROM events"),
            count(&mut store, "SELECT max(id) FROM txns"),
        );
        assert_eq!(before, after);
        assert!(archived.join("fresh/context.json").is_file());
    }

    #[test]
    fn the_import_of_migration_7_is_idempotent_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        old_store(&home, "again", &[("m1", &[NOTE_1, NOTE_2])]);
        let conn = connection(&dir.path().join("x.db"));
        at_version(&conn, 6);
        conn.execute_batch(&format!(
            "INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '{T}', 'migrate', 1, NULL, '/')"
        ))
        .unwrap();
        migration_7(&conn, &home).unwrap();
        let counts = |conn: &Connection| -> (i64, i64, i64) {
            let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
            (n("SELECT count(*) FROM contexts"), n("SELECT count(*) FROM context_records"), n("SELECT count(*) FROM events"))
        };
        assert_eq!(counts(&conn), (1, 2, 3));
        conn.execute_batch("DROP TRIGGER contexts_hist_ins; DROP TRIGGER contexts_hist_upd; DROP TRIGGER contexts_no_delete;")
            .unwrap();
        conn.execute_batch("ALTER TABLE contexts ADD COLUMN sync_failing INTEGER NOT NULL DEFAULT 0").unwrap();
        import_old_stores(&conn, &home.old_contexts_dir()).unwrap();
        assert_eq!(counts(&conn), (1, 2, 3));
    }

    #[test]
    fn an_existing_destination_is_never_overwritten_the_folder_stays_and_the_open_fails_loud() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_6_with_known_context(&home);
        old_store(&home, "blocked", &[("m1", &[NOTE_1])]);
        old_store(&home, "moves", &[("m1", &[NOTE_1])]);
        let taken = home.archived_contexts_dir().join("blocked");
        std::fs::create_dir_all(&taken).unwrap();
        std::fs::write(taken.join("mine.txt"), "keep").unwrap();

        let err = Store::open(&home, 5000).err().unwrap();
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
        assert!(err.message().contains("blocked"), "{}", err.message());
        assert!(home.old_contexts_dir().join("blocked/context.json").is_file(), "the folder stays");
        assert_eq!(std::fs::read_to_string(taken.join("mine.txt")).unwrap(), "keep");
        assert!(!taken.join("context.json").exists(), "nothing was written over it");
        assert!(home.archived_contexts_dir().join("moves/context.json").is_file(), "the others are moved");

        // The database did migrate, so the next open works; the folder still stays.
        let mut store = Store::open(&home, 5000).unwrap();
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        assert_eq!(count(&mut store, "SELECT count(*) FROM context_records WHERE context = 'blocked'"), 1);
        assert!(home.old_contexts_dir().join("blocked/context.json").is_file());
    }

    #[test]
    fn a_folder_that_is_not_a_valid_store_fails_the_migration_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_6_with_known_context(&home);
        old_store(&home, "good", &[("m1", &[NOTE_1])]);
        old_store(&home, "bad", &[("m1", &[NOTE_1, "{not json"])]);
        let err = Store::open(&home, 5000).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
        assert!(err.message().contains("Context bad") && err.message().contains("line 2"), "{}", err.message());
        let conn = Connection::open(home.db()).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, 6);
        assert_eq!(schema_hash(&conn).unwrap(), SCHEMA_HASHES[5]);
        let contexts: i64 = conn.query_row("SELECT count(*) FROM contexts", [], |r| r.get(0)).unwrap();
        assert_eq!(contexts, 1);
        assert!(home.old_contexts_dir().join("good/context.json").is_file(), "no folder is moved");
        assert!(!home.archived_contexts_dir().exists());
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

    // ---------- the triggers of §4.4 after migration 4 ----------

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
                           INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                             VALUES ('acme/di', 11, 'u', 'D', 0, 'open', 'none', 'none', 't', 't');
                           UPDATE prs SET status = 'open', pr_number = 11, pr_url = 'u', pr_opened_at = 't',
                                  pull_request = (SELECT id FROM pull_requests WHERE number = 11) WHERE plan = 'di';
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
                           UPDATE prs SET all_merged_at = 't' WHERE plan = 'pr'")
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
            "UPDATE plans SET phase = 'deploy', locked_at = 't' WHERE id = 'pl'",
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

    // ---------- migration 8 ----------

    /// A version-7 database with a Plan in each phase and `plan-phase` Events that say `pr`.
    fn version_7_with_a_plan_in_pr(home: &CpdHome) {
        let plan = |id: &str, phase: &str, locked: &str, done: &str| {
            format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('l-{id}', 'lead', 'ctx', '{id}', 'lead', '/repo', 'O3', 'idle', '{T}', '{T}');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                   VALUES ('{id}', 'ctx', 'Plan {id}', '{phase}', 'l-{id}', '{T}', {locked}, {done});"
            )
        };
        let event = |plan: &str, from: &str, to: &str| {
            format!(
                "INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES
                   (1, '{T}', 'plan-phase', 'ctx', '{plan}', 'Plan {plan} is in phase pr. Next: cpd pr poll',
                    '{{\"from\":\"{from}\",\"to\":\"{to}\",\"prs\":[{{\"key\":\"pr\"}}]}}');"
            )
        };
        let locked = format!("'{T}'");
        database_at(
            home,
            7,
            &format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
                 {} {} {} {} {}
                 {} {} {} {}",
                plan("gr", "grounding", "NULL", "NULL"),
                plan("pl", "plan", "NULL", "NULL"),
                plan("di", "dispatch", &locked, "NULL"),
                plan("pr", "pr", &locked, "NULL"),
                plan("dn", "done", &locked, &locked),
                event("di", "plan", "dispatch"),
                event("pr", "dispatch", "pr"),
                event("pr", "pr", "dispatch"),
                event("dn", "pr", "done"),
            ),
        );
    }

    #[test]
    fn a_version_7_database_migrates_the_phase_pr_to_deploy_everywhere_and_keeps_the_event_text() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_7_with_a_plan_in_pr(&home);
        let events_before = {
            let conn = connection(&home.db());
            conn.prepare("SELECT id, kind, plan, text FROM events ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok(serde_json::json!([r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?])))
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap()
        };

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        assert_eq!(
            rows(&mut store, "SELECT id, phase FROM plans ORDER BY created_at, id"),
            vec![
                serde_json::json!(["di", "dispatch"]),
                serde_json::json!(["dn", "done"]),
                serde_json::json!(["gr", "grounding"]),
                serde_json::json!(["pl", "plan"]),
                serde_json::json!(["pr", "deploy"]),
            ]
        );
        assert_eq!(
            rows(&mut store, "SELECT plan, json_extract(data, '$.from'), json_extract(data, '$.to') FROM events ORDER BY id"),
            vec![
                serde_json::json!(["di", "plan", "dispatch"]),
                serde_json::json!(["pr", "dispatch", "deploy"]),
                serde_json::json!(["pr", "deploy", "dispatch"]),
                serde_json::json!(["dn", "deploy", "done"]),
            ]
        );
        // Only `from` and `to` change: the rest of `data` and the text stay.
        assert_eq!(count(&mut store, "SELECT count(*) FROM events WHERE json_extract(data, '$.prs[0].key') = 'pr'"), 4);
        assert_eq!(
            rows(&mut store, "SELECT id, kind, plan, text FROM events ORDER BY id"),
            events_before
        );
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
        // The guards are back; the new phase graph is in force.
        assert_eq!(
            sqlite_message(&refused(&mut store, "UPDATE events SET text = 'x'")),
            "append_only: events"
        );
        assert_eq!(
            sqlite_message(&refused(&mut store, "DELETE FROM events")),
            "append_only: events"
        );
        write(&mut store, "UPDATE plans SET phase = 'done', done_at = '2026-01-09T00:00:00.000Z' WHERE id = 'pr'").unwrap();
        assert_eq!(
            sqlite_message(&refused(&mut store, "UPDATE plans SET phase = 'pr' WHERE id = 'di'")),
            "illegal_transition"
        );
        assert_eq!(
            sqlite_message(&refused(&mut store, "UPDATE plans SET phase = 'deploy' WHERE id = 'gr'")),
            "illegal_transition"
        );
    }

    // ---------- migration 9 ----------

    #[test]
    fn a_version_8_database_migrates_to_9_keeping_every_event_and_accepts_session_message() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        let events: String = [
            "context-created", "context-record", "context-grounded", "plan-created", "plan-phase", "plan-review", "plan-done",
            "job-added", "job-edited", "job-merged", "job-failed", "job-retried", "job-cancel-requested", "job-cancelled",
            "attempt-started", "attempt-status", "attempt-retry", "attempt-nudge", "all-merged", "pr-feedback", "pr-added",
            "pr-edited", "pr-cancelled", "pr-started", "pr-opened", "pr-merged", "escalation", "escalation-answered",
            "escalation-expired", "session-registered", "session-state", "session-moved", "session-renamed", "context-renamed",
            "plan-renamed", "context-archived", "plan-archived", "session-archived", "watch-started", "watch-stopped", "error",
        ]
        .iter()
        .map(|kind| format!("INSERT INTO events (txn, at, kind, text, data) VALUES (1, '{T}', '{kind}', 'text of {kind}', '{{\"k\":1}}');"))
        .collect();
        database_at(&home, 8, &events);
        let before = {
            let conn = connection(&home.db());
            conn.prepare("SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id")
                .unwrap()
                .query_map([], |r| (0..11).map(|i| r.get::<_, rusqlite::types::Value>(i).map(|v| format!("{v:?}"))).collect::<Result<Vec<_>>>())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(before.len(), 41);

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        let after = rows(&mut store, "SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id");
        assert_eq!(after.len(), before.len());
        for (i, row) in after.iter().enumerate() {
            assert_eq!((row[0].as_i64(), row[1].as_i64()), (Some(i as i64 + 1), Some(1)));
            assert_eq!(row[9], format!("text of {}", row[3].as_str().unwrap()));
        }
        assert_eq!((after[0][3].as_str(), after[40][3].as_str()), (Some("context-created"), Some("error")));
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
        // The sequence never goes back, and the new kind is accepted; an unknown one is not.
        write(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'session-message', 'x', '{\"from\":null,\"text\":\"hi\"}')")
            .unwrap();
        assert_eq!(count(&mut store, "SELECT max(id) FROM events"), 42);
        assert!(sqlite_message(&refused(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'session-mail', 'x', '{}')"))
            .contains("CHECK constraint failed"));
        // The indexes and guards are back.
        assert_eq!(count(&mut store, "SELECT count(*) FROM sqlite_schema WHERE tbl_name = 'events' AND type = 'index' AND name LIKE 'events_%'"), 3);
        assert_eq!(sqlite_message(&refused(&mut store, "UPDATE events SET text = 'x'")), "append_only: events");
        assert_eq!(sqlite_message(&refused(&mut store, "DELETE FROM events")), "append_only: events");
        assert_eq!(
            sqlite_message(&refused(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (1, 't', 'error', 'x', '{}')")),
            "events.txn must be cpd_txn()"
        );
    }

    // ---------- migration 10 ----------

    /// A version-9 database: Context `ctx` with a grounded Lead (a Lead with no Plan),
    /// a Plan with its Lead, a second Context with no grounded Lead, records of every
    /// kind, and Events of kind `context-grounded`.
    fn version_9_with_a_grounded_lead(home: &CpdHome) {
        database_at(
            home,
            9,
            &format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1'), ('two', 'Two', '{T}', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, tmux_pane)
                   VALUES ('l-g', 'lead', 'ctx', NULL, 'grounded', '/r', 'O3', 'idle', '{T}', '2026-01-01T00:00:01.000Z', '%3'),
                          ('l-a', 'lead', 'ctx', 'aa', 'lead a', '/r', 'O3', 'idle', '{T}', '{T}', NULL);
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('aa', 'ctx', 'Plan A', 'grounding', 'l-a', '{T}');
                 UPDATE contexts SET lead_session = 'l-g', grounded_seq = 2 WHERE id = 'ctx';
                 INSERT INTO context_records (context, id, kind, at, machine, text, supersedes, question, plan_id, plan_machine, plan_jira) VALUES
                   ('ctx', 'm1-d1', 'decision', '{T}', 'm1', 'D', NULL, NULL, NULL, NULL, NULL),
                   ('ctx', 'm1-n1', 'note', '{T}', 'm1', 'N', NULL, NULL, NULL, NULL, NULL),
                   ('ctx', 'm1-q1', 'question', '{T}', 'm1', 'Q', NULL, NULL, NULL, NULL, NULL),
                   ('ctx', 'm1-a1', 'answer', '{T}', 'm1', 'A', NULL, 'm1-q1', NULL, NULL, NULL),
                   ('ctx', 'm1-p1', 'plan', '{T}', 'm1', 'Plan A', NULL, NULL, 'aa', 'm1', 'ABC-1');
                 INSERT INTO events (txn, at, kind, context, session, text, data) VALUES
                   (1, '{T}', 'context-grounded', 'ctx', 'l-g', 'Context ctx has a grounded Lead.', '{{\"seq\":2,\"forked_from\":\"l-a\"}}');"
            ),
        );
    }

    #[test]
    fn a_version_9_database_migrates_to_10_archiving_the_grounded_lead_and_dropping_its_columns() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        version_9_with_a_grounded_lead(&home);
        let records = "SELECT seq, context, id, kind, at, machine, text, supersedes, question, plan_id, plan_machine, plan_jira FROM context_records ORDER BY seq";
        let events = "SELECT id, txn, kind, context, plan, session, text, data FROM events WHERE kind <> 'session-archived' ORDER BY id";
        let dump = |sql: &str| -> Vec<Vec<String>> {
            let conn = connection(&home.db());
            let mut stmt = conn.prepare(sql).unwrap();
            let n = stmt.column_count();
            stmt.query_map([], |r| (0..n).map(|i| r.get::<_, rusqlite::types::Value>(i).map(|v| format!("{v:?}"))).collect())
                .unwrap()
                .collect::<Result<_>>()
                .unwrap()
        };
        let (records_before, events_before) = (dump(records), dump(events));

        let mut store = Store::open(&home, 5000).unwrap();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
        assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
        drop(store);
        assert_eq!(dump(records), records_before);
        assert_eq!(dump(events), events_before);
        let mut store = Store::open(&home, 5000).unwrap();
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
        assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_table_info('contexts') WHERE name IN ('lead_session', 'grounded_seq')"), 0);
        assert_eq!(rows(&mut store, "SELECT id, title, archived_at FROM contexts ORDER BY id").len(), 2);

        // The grounded Lead is archived, once, with its Event; the Plan's Lead is not.
        assert_eq!(
            rows(&mut store, "SELECT id, role, plan, archived_at IS NOT NULL, tmux_pane FROM sessions ORDER BY id"),
            vec![serde_json::json!(["l-a", "lead", "aa", 0, Value::Null]), serde_json::json!(["l-g", "lead", Value::Null, 1, "%3"])]
        );
        assert_eq!(
            rows(&mut store, "SELECT kind, context, plan, session, text, data FROM events WHERE kind = 'session-archived'"),
            vec![serde_json::json!(["session-archived", "ctx", Value::Null, "l-g", "Session grounded archived.", r#"{"archived":true}"#])]
        );
        assert_eq!(count(&mut store, "SELECT count(*) FROM history WHERE tbl = 'sessions' AND op = 'update'"), 1);

        // A `brief` record needs its Plan reference; positions continue after the old ones.
        let brief = |plan: &str| {
            format!("INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine) VALUES ('ctx', 'm1-b1', 'brief', 't', 'm1', 'B', {plan})")
        };
        assert!(sqlite_message(&refused(&mut store, &brief("NULL, NULL"))).contains("CHECK constraint failed"));
        write(&mut store, &brief("'aa', 'm1'")).unwrap();
        assert_eq!(count(&mut store, "SELECT seq FROM context_records WHERE id = 'm1-b1'"), 6);
        assert_eq!(
            sqlite_message(&refused(&mut store, "UPDATE context_records SET text = 'x'")),
            "append_only: context_records"
        );

        // One live Manager per Context, none with a Plan, and a second after the first is archived.
        let manager = |id: &str, context: &str, plan: &str| {
            format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('{id}', 'manager', '{context}', {plan}, 'm', '/', 'O5', 'stopped', 't', 't')"
            )
        };
        write(&mut store, &manager("m-1", "ctx", "NULL")).unwrap();
        let second = refused(&mut store, &manager("m-2", "ctx", "NULL"));
        assert!(sqlite_message(&second).contains("UNIQUE constraint failed"), "{second:?}");
        assert!(sqlite_message(&refused(&mut store, &manager("m-3", "two", "'aa'"))).contains("CHECK constraint failed"));
        write(&mut store, &manager("m-4", "two", "NULL")).unwrap();
        write(&mut store, "UPDATE sessions SET archived_at = 't' WHERE id = 'm-1'").unwrap();
        write(&mut store, &manager("m-2", "ctx", "NULL")).unwrap();
        assert_eq!(
            sqlite_message(&refused(&mut store, "DELETE FROM sessions")),
            "append_only: sessions"
        );
        // A Worker still needs an Attempt, a Lead none.
        assert!(sqlite_message(&refused(
            &mut store,
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('w', 'worker', 'ctx', 'aa', 'w', '/', 'S3', 'stopped', 't', 't')"
        ))
        .contains("CHECK constraint failed"));
    }

    #[test]
    fn a_version_9_database_with_no_grounded_lead_migrates_to_10_with_no_new_event() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        database_at(&home, 9, &format!("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');"));
        let mut store = Store::open(&home, 5000).unwrap();
        assert_eq!(count(&mut store, "SELECT count(*) FROM events"), 0);
        assert!(store.schema_mismatch().is_none());
    }
}
