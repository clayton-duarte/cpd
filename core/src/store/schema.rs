//! The schema: the migrations, the generated history triggers and the schema
//! hashes (contract §4.3–§4.5).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Result, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::context::legacy;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::PrKey;

/// A legacy migration: applied once, in order, inside the migrating transaction.
pub(super) type Migration = fn(&Connection) -> Result<()>;

/// What a migration promises old binaries (contract §4.5 A1): `Additive` means
/// a binary that does not know it still reads and writes the store correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Compat {
    Additive,
    Breaking,
}

impl Compat {
    /// As `schema_migrations.compat` spells it.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Compat::Additive => "additive",
            Compat::Breaking => "breaking",
        }
    }
}

/// A legacy migration (contract §4.5 N2): its name, its compat, the text of its
/// `.sql` (its checksum is that text's) and the Rust function that applies it.
pub(super) struct Legacy {
    pub(super) name: &'static str,
    pub(super) compat: Compat,
    pub(super) sql: &'static str,
    pub(super) apply: Migration,
}

/// The legacy set, frozen: `LEGACY[i]` is what integer-era binaries called
/// migration `i + 1`, and `PRAGMA user_version` of such a store is how many of
/// them it has. A numbered migration is never added again.
pub(super) const LEGACY: &[Legacy] = &[
    Legacy { name: "0001-initial", compat: Compat::Breaking, sql: include_str!("migrations/0001-initial.sql"), apply: migration_1 },
    Legacy { name: "0002-cockpit-parity", compat: Compat::Additive, sql: include_str!("migrations/0002-cockpit-parity.sql"), apply: migration_2 },
    Legacy { name: "0003-archive", compat: Compat::Breaking, sql: include_str!("migrations/0003-archive.sql"), apply: migration_3 },
    Legacy { name: "0004-cross-repo", compat: Compat::Breaking, sql: include_str!("migrations/0004-cross-repo.sql"), apply: migration_4 },
    Legacy { name: "0005-plan-reopen", compat: Compat::Breaking, sql: include_str!("migrations/0005-plan-reopen.sql"), apply: migration_5 },
    Legacy { name: "0006-pr-poller", compat: Compat::Breaking, sql: include_str!("migrations/0006-pr-poller.sql"), apply: migration_6 },
    Legacy { name: "0007-context-records", compat: Compat::Breaking, sql: include_str!("migrations/0007-context-records.sql"), apply: migration_7 },
    Legacy { name: "0008-phase-deploy", compat: Compat::Breaking, sql: include_str!("migrations/0008-phase-deploy.sql"), apply: migration_8 },
    Legacy { name: "0009-session-message", compat: Compat::Breaking, sql: include_str!("migrations/0009-session-message.sql"), apply: migration_9 },
    Legacy { name: "0010-manager", compat: Compat::Breaking, sql: include_str!("migrations/0010-manager.sql"), apply: migration_10 },
    Legacy { name: "0011-pr-mergeable", compat: Compat::Breaking, sql: include_str!("migrations/0011-pr-mergeable.sql"), apply: migration_11 },
    Legacy { name: "0012-inbox", compat: Compat::Breaking, sql: include_str!("migrations/0012-inbox.sql"), apply: migration_12 },
    Legacy { name: "0013-pr-inbox", compat: Compat::Breaking, sql: include_str!("migrations/0013-pr-inbox.sql"), apply: migration_13 },
    Legacy { name: "0014-register", compat: Compat::Breaking, sql: include_str!("migrations/0014-register.sql"), apply: migration_14 },
];

/// The legacy version whose migration (`0007-context-records`) imports the old
/// git stores of the Contexts: the runner calls [`import_old_stores`] right
/// after applying it and moves the folders after the commit (`store/mod.rs`).
/// The import is not part of [`Legacy::apply`], so the reference schema
/// (`verdict.rs`) never reads `$CPD_HOME`.
pub(super) const IMPORTS_OLD_STORES: usize = 7;

/// A named migration (contract §4.5 N1–N4): a file of `migrations/`, listed by
/// `build.rs`. `run` is the Rust beside its SQL, called after the SQL.
#[derive(Clone, Copy)]
pub(super) struct Named {
    pub(super) name: &'static str,
    pub(super) compat: Compat,
    pub(super) sql: &'static str,
    pub(super) run: Option<fn(&Connection, &CpdHome) -> std::result::Result<(), CpdError>>,
}

include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

/// The legacy schema hash of each legacy version (§4.5 N2): `SCHEMA_HASHES[i]`
/// is version `i + 1`'s. Only the conversion uses them (C2 step 2). A test
/// asserts that migrating an empty database gives each of them.
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
    "2d346012b74ad0cba8d2f1658e1b9178942c0f9b6dc0d1014c0a2b722d62c599",
];

const _: () = assert!(SCHEMA_HASHES.len() == LEGACY.len(), "one legacy schema hash per legacy migration");

/// `PRAGMA user_version` of every named store (§4.5): it never changes again,
/// and means only "this store uses named migrations".
pub const VERSION: i64 = 1000;

/// The canonical schema hash (§4.3) of a fresh store that applied exactly this
/// binary's migrations, pinned here and never recorded in the store (hs-d178
/// D2). A test asserts it and prints the new value when it is stale.
pub const SCHEMA_HASH: &str = "e7ec648b7a430362a39350b98f2b820a71d59824c649d61d5af8a33eeff82e4a";

/// The runner's own table (§4.4): created with the first migration it records,
/// never by a migration's `.sql`.
pub(super) const RUNNER_DDL: &str = "\
CREATE TABLE schema_migrations (
  name      TEXT    PRIMARY KEY CHECK (length(name) BETWEEN 6 AND 54),
  compat    TEXT    NOT NULL CHECK (compat IN ('additive', 'breaking')),
  checksum  TEXT    NOT NULL CHECK (length(checksum) = 64),      -- sha256 (hex) of the migration's text
  seq       INTEGER NOT NULL UNIQUE CHECK (seq >= 1),           -- order of application on this store
  txn       INTEGER NOT NULL REFERENCES txns(id),               -- the migrating transaction
  converted INTEGER NOT NULL CHECK (converted IN (0, 1))        -- 1: a legacy migration recorded by the conversion (§4.5)
) STRICT;
CREATE TRIGGER schema_migrations_own BEFORE INSERT ON schema_migrations WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'schema_migrations.txn must be cpd_txn()'); END;
CREATE TRIGGER schema_migrations_no_update BEFORE UPDATE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;
CREATE TRIGGER schema_migrations_no_delete BEFORE DELETE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;
";

/// The tables migration 1 gives history triggers; frozen with it. Every later
/// table is listed in `history_tables` (§4.4), which `kind-tables` created.
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
    conn.execute_batch(LEGACY[0].sql)?;
    for table in VERSION_1_HISTORY_TABLES {
        create_history_triggers(conn, table, *table != "dependencies")?;
    }
    Ok(())
}

/// Cockpit parity (§4.5): `plans` gains `pr_title`, `pr_review_decision` and
/// `pr_checks`, so its history triggers are dropped and generated again to list them.
fn migration_2(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[1].sql)?;
    conn.execute_batch("DROP TRIGGER plans_hist_ins; DROP TRIGGER plans_hist_upd; DROP TRIGGER plans_no_delete;")?;
    create_history_triggers(conn, "plans", true)
}

/// Archive and rename (§4.5): `archived_at` on `contexts`, `plans` and `sessions`
/// and `events` rebuilt with five more kinds, so the three tables' history
/// triggers are dropped and generated again to list the new column.
fn migration_3(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[2].sql)?;
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
    let (tables, rebuild) = LEGACY[3].sql
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
    conn.execute_batch(LEGACY[4].sql)
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
    let (create, rebuild) = LEGACY[5].sql
        .split_once(HISTORY)
        .expect("0006.sql has its `-- @history` line");
    conn.execute_batch(create)?;
    create_history_triggers(conn, "pull_requests", false)?;
    conn.execute_batch(rebuild)?;
    create_history_triggers(conn, "prs", true)
}

/// Context records only in the database (§4.5 legacy migration 0007, hs-a1):
/// drops `contexts.sync_failing` (its history triggers are generated again).
/// The runner then imports the old stores ([`import_old_stores`], [`IMPORTS_OLD_STORES`]).
fn migration_7(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[6].sql)?;
    create_history_triggers(conn, "contexts", true)
}

/// The Plan phase `pr` is called `deploy` (§4.5 migration 0008, hs-d76): `plans`
/// is rebuilt, `plans_phase` and the `plan-phase` Events are rewritten; the
/// history triggers of `plans` are generated again.
fn migration_8(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[7].sql)?;
    create_history_triggers(conn, "plans", true)
}

/// The Event kind `session-message` (§4.5 migration 0009, hs-d133): `events` is
/// rebuilt with the kind in its check. No history triggers: `events` has none.
fn migration_9(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[8].sql)
}

/// The Manager role and the `brief` record (§4.5 migration 0010, hs-d92, hs-d129):
/// `sessions`, `context_records` and `contexts` are rebuilt, their history
/// triggers generated again, and every grounded Lead Session (a Lead with no
/// Plan) archived with one `session-archived` Event each, in the migration's
/// `txns` row, as `session archive` would write it.
fn migration_10(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[9].sql)?;
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

/// The import of legacy migration 0007: every Context and record of
/// `$CPD_HOME/contexts/<id>/` the database does not hold yet; a folder that is
/// not a valid store fails the migration. With the row values and Events the engine's
/// earlier `import_all` wrote: a `contexts` row and `context-created` for a
/// Context the database lacks, and for each record whose `(context, id)` it
/// lacks the row and `context-record`. Idempotent. The Events are written in
/// SQL: they belong to this migration's `txns` row, whatever the Event writer
/// of later versions does.
pub(super) fn import_old_stores(conn: &Connection, dir: &Path) -> Result<()> {
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
    conn.execute_batch(LEGACY[10].sql)?;
    create_history_triggers(conn, "pull_requests", false)
}

/// The Inbox (§4.5 legacy migration 0012, hs-d168): `inbox_items`, and `events`
/// rebuilt with the four `inbox-*` kinds (`0012-inbox.sql`).
fn migration_12(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[11].sql)?;
    create_history_triggers(conn, "inbox_items", true)
}

/// The PR poll's Inbox kinds (§4.5 legacy migration 0013, hs-d165): `authored` and
/// `review_requested` on `pull_requests`, whose history triggers are written again
/// to list every column, and `inbox_items` rebuilt with the four pull request kinds
/// (`0013-pr-inbox.sql` drops and writes `pull_requests_hist_del`; the other history
/// triggers and `inbox_items_no_delete` are generated).
fn migration_13(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[12].sql)?;
    create_history_triggers(conn, "pull_requests", false)?;
    create_history_triggers(conn, "inbox_items", true)
}

/// Stub drafts (§4.5 legacy migration 0014, hs-d241, R1): `jobs` rebuilt with the Tier and
/// the Spec fields nullable while the Job is a `draft` or `cancelled` (`0014-register.sql`
/// writes its triggers `jobs_locked`, `jobs_pr_dependencies` and `jobs_status`;
/// its history triggers and `jobs_no_delete` are generated).
fn migration_14(conn: &Connection) -> Result<()> {
    conn.execute_batch(LEGACY[13].sql)?;
    create_history_triggers(conn, "jobs", true)
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

/// What a generated `_hist_upd` trigger writes in `before` and `after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateRow {
    /// The whole row, twice: what the legacy migrations created, frozen with
    /// them (their schema hashes, §4.5 N2, include the triggers' text).
    Whole,
    /// The key and only the columns that changed (§4.4 "History row shape",
    /// hs-d234, sql-F2): what [`reconcile`] makes and the schema check expects
    /// since the named migration `history-changed`.
    Changed,
}

/// Creates `<table>_hist_ins` and `<table>_hist_upd`, and `<table>_no_delete`
/// when `no_delete`, from the table's current columns and primary key, as the
/// legacy migrations did (`_hist_upd` with the whole row, [`UpdateRow::Whole`]).
/// The legacy migrations that add or rename columns drop these and call this
/// again; named migrations never create them: [`reconcile`] does (§4.5 K4).
fn create_history_triggers(conn: &Connection, table: &str, no_delete: bool) -> Result<()> {
    for (_, sql) in history_triggers(conn, table, no_delete, UpdateRow::Whole, false)? {
        conn.execute_batch(&format!("{sql};"))?;
    }
    Ok(())
}

/// The table's columns `(name, pk)` in column order, empty when it does not exist.
fn table_columns(conn: &Connection, table: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare("SELECT name, pk FROM pragma_table_info(?1) ORDER BY cid")?;
    stmt.query_map([table], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
}

/// `P(x)` of contract §4.4 "The delete exception": the Plan `x` is deleted.
fn plan_deleted(plan: &str) -> String {
    format!("(SELECT deleted_at FROM plans WHERE id = {plan}) IS NOT NULL")
}

/// `C(x)` of §4.4: the Context `x` is deleted.
fn context_deleted(context: &str) -> String {
    format!("(SELECT deleted_at FROM contexts WHERE id = {context}) IS NOT NULL")
}

/// The `<owner>` of the delete exception of `table` (§4.4 "The delete exception"): true when the row's
/// owner is deleted. `None` for a table no delete reaches.
fn delete_owner(table: &str) -> Option<String> {
    let plan_row = || format!("{} OR (OLD.plan IS NULL AND {})", plan_deleted("OLD.plan"), context_deleted("OLD.context"));
    let session_row = |column: &str| {
        format!(
            "EXISTS (SELECT 1 FROM sessions s WHERE s.id = OLD.{column} AND ({} OR (s.plan IS NULL AND {})))",
            plan_deleted("s.plan"),
            context_deleted("s.context")
        )
    };
    Some(match table {
        "prs" | "pr_needs" | "plan_reviews" | "jobs" | "dependencies" | "attempts" | "escalations" => plan_deleted("OLD.plan"),
        "context_records" | "record_retirements" | "record_curations" => context_deleted("OLD.context"),
        "events" | "inbox_items" | "sessions" => plan_row(),
        "plannotator_reviews" => session_row("session"),
        "board_posts" => session_row("to_session"),
        _ => return None,
    })
}

/// The generated triggers of `table` as `(name, sql)`, `sql` as `sqlite_schema`
/// stores it (no closing `;`), with `_hist_upd` in the shape `update`. With
/// `tombstone` (the generator since `delete-tombstone`, §4.4 "Triggers") the
/// `_no_delete` trigger holds the table's delete exception and `_hist_del` is
/// written; the legacy migrations create neither.
fn history_triggers(conn: &Connection, table: &str, no_delete: bool, update: UpdateRow, tombstone: bool) -> Result<Vec<(String, String)>> {
    let columns = table_columns(conn, table)?;
    if columns.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(format!("no table {table}")));
    }
    let mut key: Vec<&(String, i64)> = columns.iter().filter(|(_, n)| *n > 0).collect();
    key.sort_by_key(|(_, n)| *n);
    if key.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(format!("table {table} has no primary key")));
    }
    let pk = |row: &str| -> String {
        let cols: Vec<String> = key.iter().map(|(name, _)| format!("{row}.{name}")).collect();
        format!("json_array({})", cols.join(", "))
    };
    let object = |row: &str| -> String {
        let pairs: Vec<String> = columns.iter().map(|(name, _)| format!("'{name}', {row}.{name}")).collect();
        format!("json_object({})", pairs.join(", "))
    };
    // §4.4 "History row shape": a `json_group_object` of the key columns and of
    // each other column whose value changed, one line per column.
    let changed = |row: &str| -> String {
        let keys: Vec<String> = key
            .iter()
            .enumerate()
            .map(|(i, (name, _))| if i == 0 { format!("SELECT '{name}' AS k, {row}.{name} AS v") } else { format!("SELECT '{name}', {row}.{name}") })
            .collect();
        let mut lines = vec![keys.join(" UNION ALL ")];
        for (name, n) in &columns {
            if *n == 0 {
                lines.push(format!("UNION ALL SELECT '{name}', {row}.{name} WHERE OLD.{name} IS NOT NEW.{name}"));
            }
        }
        format!("(SELECT json_group_object(k, v) FROM (\n       {}\n    ))", lines.join("\n       "))
    };
    let (before, after) = match update {
        UpdateRow::Whole => (object("OLD"), object("NEW")),
        UpdateRow::Changed => (changed("OLD"), changed("NEW")),
    };
    let mut triggers = vec![
        (
            format!("{table}_hist_ins"),
            format!(
                "CREATE TRIGGER {table}_hist_ins AFTER INSERT ON {table} BEGIN\n  \
                 INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), '{table}',\n    \
                 {}, 'insert', NULL,\n    {});\nEND",
                pk("NEW"),
                object("NEW"),
            ),
        ),
        (
            format!("{table}_hist_upd"),
            format!(
                "CREATE TRIGGER {table}_hist_upd AFTER UPDATE ON {table} BEGIN\n  \
                 INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), '{table}',\n    \
                 {}, 'update',\n    {},\n    {});\nEND",
                pk("NEW"),
                before,
                after,
            ),
        ),
    ];
    if no_delete && tombstone {
        triggers.push((
            format!("{table}_hist_del"),
            format!(
                "CREATE TRIGGER {table}_hist_del AFTER DELETE ON {table} BEGIN\n  \
                 INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), '{table}',\n    \
                 {}, 'delete',\n    {}, NULL);\nEND",
                pk("OLD"),
                object("OLD"),
            ),
        ));
    }
    if no_delete {
        let when = match delete_owner(table).filter(|_| tombstone) {
            Some(owner) => format!("\nWHEN NOT ({owner})\nBEGIN\n"),
            None => " BEGIN\n  ".to_owned(),
        };
        triggers.push((
            format!("{table}_no_delete"),
            format!(
                "CREATE TRIGGER {table}_no_delete BEFORE DELETE ON {table}{when}\
                 SELECT RAISE(ABORT, 'append_only: {table}');\nEND"
            ),
        ));
    }
    Ok(triggers)
}

/// The `sql` of the trigger `name`, when the store has it.
fn stored_trigger(conn: &Connection, name: &str) -> Result<Option<String>> {
    conn.query_row("SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1", [name], |r| r.get(0)).optional()
}

/// The tables with generated history triggers in a store before `kind-tables`
/// (it has no `history_tables` yet): migration 1's and the ones the legacy
/// migrations created. Frozen with the legacy set.
const LEGACY_HISTORY_TABLES: &[&str] = &[
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

/// `(table, no_delete)` for every table whose history triggers are generated:
/// the rows of `history_tables` (§4.4), by name; a store that does not have the
/// table yet (before `kind-tables`) has [`LEGACY_HISTORY_TABLES`]. `no_delete` 0
/// means no generated `_no_delete` (`dependencies`, `pr_needs` and
/// `pull_requests` write their own rule).
fn history_tables(conn: &Connection) -> Result<Vec<(String, bool)>> {
    let listed: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'history_tables')",
        [],
        |r| r.get(0),
    )?;
    if !listed {
        let mut tables: Vec<_> = LEGACY_HISTORY_TABLES
            .iter()
            .map(|t| (t.to_string(), !matches!(*t, "dependencies" | "pr_needs" | "pull_requests")))
            .collect();
        tables.sort();
        return Ok(tables);
    }
    let mut stmt = conn.prepare("SELECT name, no_delete FROM history_tables ORDER BY name")?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1)))?.collect()
}

/// §4.5 K4: after the last migration of a migrating transaction, makes every
/// generated trigger equal to what the generator makes from its table's
/// current columns: a trigger that is identical stays, any other is dropped and
/// created again. The result depends only on the final columns, so not on the
/// order in which additive migrations added them.
pub(super) fn reconcile(conn: &Connection) -> Result<()> {
    for (table, no_delete) in history_tables(conn)? {
        for (name, sql) in history_triggers(conn, &table, no_delete, UpdateRow::Changed, true)? {
            let stored = stored_trigger(conn, &name)?;
            if stored.as_deref() == Some(sql.as_str()) {
                continue;
            }
            if stored.is_some() {
                conn.execute_batch(&format!("DROP TRIGGER {name};"))?;
            }
            conn.execute_batch(&format!("{sql};"))?;
        }
    }
    Ok(())
}

/// The legacy schema hash (§4.5 N2): sha256 (hex) of the JSON array of
/// `[type, name, sql]` rows of `sqlite_schema`, as integer-era binaries computed
/// it. Used only to check a legacy store before the conversion.
pub(super) fn legacy_schema_hash(conn: &Connection) -> Result<String> {
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
    Ok(sha256_hex(&serde_json::to_string(&rows).expect("JSON values always serialise")))
}

/// Lowercase hex sha256.
pub(super) fn sha256_hex(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

// ---------- the canonical schema hash (§4.3) ----------

/// The end (exclusive) of the quoted run that starts at `start`, or `None` when
/// `chars[start]` opens none: `'…'`, `"…"` and `` `…` `` (the quote doubled
/// inside is an escape) and `[…]`. An unterminated run ends the text.
fn quoted_end(chars: &[char], start: usize) -> Option<usize> {
    let open = *chars.get(start)?;
    let close = match open {
        '\'' | '"' | '`' => open,
        '[' => ']',
        _ => return None,
    };
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == close {
            if open != '[' && chars.get(i + 1) == Some(&close) {
                i += 2;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    Some(chars.len())
}

/// `N(text)` (§4.3 item 2): quoted runs unchanged; outside them every comment
/// is deleted, every run of whitespace and deleted comments becomes one space,
/// and the ends are trimmed.
fn normalize(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut space = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            space = true;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            space = true;
        } else if c.is_ascii_whitespace() {
            space = true;
            i += 1;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            let end = quoted_end(&chars, i).unwrap_or(i + 1);
            out.extend(&chars[i..end]);
            i = end;
        }
    }
    out
}

/// A table's `sql` split as §4.3 item 3 says: its columns as `(key, N(item))`
/// sorted by key, its constraints in source order, and its options.
struct TableParts {
    columns: Vec<(String, String)>,
    constraints: Vec<String>,
    options: String,
}

/// The first word of `item`: up to the first whitespace or `(`.
fn first_word(item: &[char]) -> &[char] {
    let end = item.iter().position(|c| c.is_ascii_whitespace() || *c == '(').unwrap_or(item.len());
    &item[..end]
}

/// A column item's key: its first token without quotes (a doubled quote
/// character becomes one), ASCII-lowercased.
fn column_key(item: &[char]) -> String {
    let token: String = match quoted_end(item, 0) {
        Some(end) => {
            let (open, inner) = (item[0], &item[1..end.max(2) - 1]);
            let mut key = String::new();
            let mut i = 0;
            while i < inner.len() {
                key.push(inner[i]);
                i += if open != '[' && inner.get(i + 1) == Some(&open) { 2 } else { 1 };
            }
            key
        }
        None => first_word(item).iter().collect(),
    };
    token.to_ascii_lowercase()
}

fn table_parts(sql: &str) -> Option<TableParts> {
    let text: Vec<char> = normalize(sql).chars().collect();
    let mut i = 0;
    let open = loop {
        if i >= text.len() {
            return None;
        }
        if let Some(end) = quoted_end(&text, i) {
            i = end;
        } else if text[i] == '(' {
            break i;
        } else {
            i += 1;
        }
    };
    let mut items: Vec<&[char]> = Vec::new();
    let (mut depth, mut item_start, mut i) = (0, open + 1, open + 1);
    let close = loop {
        if i >= text.len() {
            return None;
        }
        if let Some(end) = quoted_end(&text, i) {
            i = end;
            continue;
        }
        match text[i] {
            '(' => depth += 1,
            ')' if depth == 0 => break i,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&text[item_start..i]);
                item_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    };
    items.push(&text[item_start..close]);
    let (mut columns, mut constraints) = (Vec::new(), Vec::new());
    for item in items {
        let item = normalize(&item.iter().collect::<String>());
        let chars: Vec<char> = item.chars().collect();
        let word: String = first_word(&chars).iter().collect();
        if ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"].iter().any(|k| word.eq_ignore_ascii_case(k)) {
            constraints.push(item);
        } else {
            columns.push((column_key(&chars), item));
        }
    }
    columns.sort();
    Some(TableParts { columns, constraints, options: normalize(&text[close + 1..].iter().collect::<String>()) })
}

/// The canonical entries (§4.3 items 1 to 4), sorted by `(type, name)`: every
/// object but the generated history triggers, a table as
/// `["table", name, columns, constraints, options]`, any other as
/// `[type, name, sql]`.
pub(super) fn canonical_entries(conn: &Connection) -> Result<Vec<Value>> {
    let generated: std::collections::HashSet<String> = history_tables(conn)?
        .into_iter()
        .flat_map(|(t, no_delete)| {
            [format!("{t}_hist_ins"), format!("{t}_hist_upd")]
                .into_iter()
                .chain(no_delete.then(|| format!("{t}_hist_del")))
                .chain(no_delete.then(|| format!("{t}_no_delete")))
        })
        .collect();
    let mut stmt = conn
        .prepare("SELECT type, name, sql FROM sqlite_schema WHERE substr(name, 1, 7) <> 'sqlite_' ORDER BY type, name")?;
    let rows: Vec<(String, String, Option<String>)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_>>()?;
    let mut entries = Vec::new();
    for (kind, name, sql) in rows {
        if kind == "trigger" && generated.contains(&name) {
            continue;
        }
        let parts = if kind == "table" { sql.as_deref().and_then(table_parts) } else { None };
        let entry = match parts {
            Some(t) => Value::from(vec![
                Value::from("table"),
                Value::from(name),
                Value::from(t.columns.into_iter().map(|(_, c)| c).collect::<Vec<_>>()),
                Value::from(t.constraints),
                Value::from(t.options),
            ]),
            _ => Value::from(vec![Value::from(kind), Value::from(name), sql.map_or(Value::Null, Value::from)]),
        };
        entries.push(entry);
    }
    Ok(entries)
}

/// The canonical schema hash (§4.3 item 6): sha256 (hex) of the compact JSON of
/// [`canonical_entries`]. It depends on which migrations were applied and not
/// on their order.
pub fn canonical_hash(conn: &Connection) -> Result<String> {
    let entries = canonical_entries(conn)?;
    Ok(sha256_hex(&serde_json::to_string(&entries).expect("JSON values always serialise")))
}

/// §4.3 item 5: the first generated history trigger that is missing or that
/// differs from what the generator makes from its table's current columns, as
/// `(name, expected sql, stored sql)`.
pub(super) fn generated_mismatch(conn: &Connection) -> Result<Option<(String, String, Option<String>)>> {
    for (table, no_delete) in history_tables(conn)? {
        if table_columns(conn, &table)?.is_empty() {
            return Ok(Some((format!("{table}_hist_ins"), format!("a table {table}"), None)));
        }
        for (name, sql) in history_triggers(conn, &table, no_delete, UpdateRow::Changed, true)? {
            let stored = stored_trigger(conn, &name)?;
            if stored.as_deref() != Some(sql.as_str()) {
                return Ok(Some((name, sql, stored)));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod blue_tests;
#[cfg(test)]
mod cyan_tests;
