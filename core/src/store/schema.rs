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
    |conn, _| migration_13(conn),
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
    "7da9b792cadf9d12a54eb26ad99a78c48fdcbb2e4a220f767973a7760cd16c9a",
];

const _: () = assert!(SCHEMA_HASHES.len() == MIGRATIONS.len(), "one schema hash per migration");

/// The schema version this binary writes: `N`.
pub const VERSION: i64 = MIGRATIONS.len() as i64;

/// The tables migration 1 gave generated history triggers (§4.4); frozen with
/// it. Later migrations write their own triggers for the tables they add
/// (`pull_requests` has no `_no_delete`, an unowned row may be deleted;
/// `dependencies` and `pr_needs` have their own, a draft Job's Dependency and
/// a PR's need before the lock may be deleted). The tests derive the current
/// list from `sqlite_schema`: every table but `txns`, `history` and `events`.
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

/// The PR poll's Inbox kinds (§4.5 migration 0013, hs-d165): `authored` and
/// `review_requested` on `pull_requests`, whose history triggers are written again
/// to list every column, and `inbox_items` rebuilt with the four pull request kinds
/// (`0013.sql` drops and writes `pull_requests_hist_del`; the other history
/// triggers and `inbox_items_no_delete` are generated).
fn migration_13(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("migrations/0013.sql"))?;
    create_history_triggers(conn, "pull_requests", false)?;
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
mod tests;
