//! The schema: the migrations, the generated history triggers and the schema
//! hashes (contract §4.3–§4.5).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

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
];

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
];

const _: () = assert!(SCHEMA_HASHES.len() == LEGACY.len(), "one legacy schema hash per legacy migration");

/// `PRAGMA user_version` of every named store (§4.5): it never changes again,
/// and means only "this store uses named migrations".
pub const VERSION: i64 = 1000;

/// The canonical schema hash (§4.3) of a fresh store that applied exactly this
/// binary's migrations, pinned here and never recorded in the store (hs-d178
/// D2). A test asserts it and prints the new value when it is stale.
pub const SCHEMA_HASH: &str = "d2999fd6cb4643cba49dc67ba57a650dd665a20d0d5092dc7eff0827e3d65bce";

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
/// shape of §4.4's `escalations` example. The legacy migrations that add or
/// rename columns drop these and call this again; named migrations never
/// create them: [`reconcile`] does (§4.5 K4).
fn create_history_triggers(conn: &Connection, table: &str, no_delete: bool) -> Result<()> {
    for (_, sql) in history_triggers(conn, table, no_delete)? {
        conn.execute_batch(&format!("{sql};"))?;
    }
    Ok(())
}

/// The table's columns `(name, pk)` in column order, empty when it does not exist.
fn table_columns(conn: &Connection, table: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare("SELECT name, pk FROM pragma_table_info(?1) ORDER BY cid")?;
    stmt.query_map([table], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
}

/// The generated triggers of `table` as `(name, sql)`, `sql` as `sqlite_schema`
/// stores it (no closing `;`).
fn history_triggers(conn: &Connection, table: &str, no_delete: bool) -> Result<Vec<(String, String)>> {
    let columns = table_columns(conn, table)?;
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
                object("OLD"),
                object("NEW"),
            ),
        ),
    ];
    if no_delete {
        triggers.push((
            format!("{table}_no_delete"),
            format!(
                "CREATE TRIGGER {table}_no_delete BEFORE DELETE ON {table} BEGIN\n  \
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

/// `(table, no_delete)` for every table whose history triggers are generated.
/// Until `kind-tables` moves the list into the store (`history_tables`), it is
/// [`HISTORY_TABLES`]; `dependencies`, `pr_needs` and `pull_requests` have no
/// generated `_no_delete`.
fn history_tables() -> impl Iterator<Item = (&'static str, bool)> {
    HISTORY_TABLES.iter().map(|t| (*t, !matches!(*t, "dependencies" | "pr_needs" | "pull_requests")))
}

/// §4.5 K4: after the last migration of a migrating transaction, makes every
/// generated trigger equal to what the generator makes from its table's
/// current columns: a trigger that is identical stays, any other is dropped and
/// created again. The result depends only on the final columns, so not on the
/// order in which additive migrations added them.
pub(super) fn reconcile(conn: &Connection) -> Result<()> {
    for (table, no_delete) in history_tables() {
        for (name, sql) in history_triggers(conn, table, no_delete)? {
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
fn canonical_entries(conn: &Connection) -> Result<Vec<Value>> {
    let generated: std::collections::HashSet<String> = history_tables()
        .flat_map(|(t, no_delete)| {
            [format!("{t}_hist_ins"), format!("{t}_hist_upd")].into_iter().chain(no_delete.then(|| format!("{t}_no_delete")))
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
    for (table, no_delete) in history_tables() {
        if table_columns(conn, table)?.is_empty() {
            return Ok(Some((format!("{table}_hist_ins"), format!("a table {table}"), None)));
        }
        for (name, sql) in history_triggers(conn, table, no_delete)? {
            let stored = stored_trigger(conn, &name)?;
            if stored.as_deref() != Some(sql.as_str()) {
                return Ok(Some((name, sql, stored)));
            }
        }
    }
    Ok(None)
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
    /// `Store::open` sets around them (§4.5 migration 4, step 1).
    fn at_version(conn: &Connection, n: usize) {
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
        for legacy in &LEGACY[..n] {
            (legacy.apply)(conn).unwrap();
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
            assert_eq!(legacy_schema_hash(&conn).unwrap(), *expected, "version {}", n + 1);
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
                    "UPDATE pull_requests SET title = 'T', review_decision = 'approved', checks = 'passed', draft = 1 WHERE number = 7",
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

        for bad in ["title = ''", "review_decision = 'maybe'", "checks = 'green'", "state = 'merged'", "repo = 'x'", "number = 0", "draft = 2"] {
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
        database_at(home, 2, &version_2_sql());
    }

    /// The rows of [`version_2`].
    fn version_2_sql() -> String {
        format!(
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
        )
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
        assert_eq!(legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[2]);

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
        assert_eq!(legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(), SCHEMA_HASHES[3]);
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
                serde_json::json!(["pr", "pr", Value::Null]),
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
    fn plans_phase_allows_pr_and_done_back_to_dispatch_and_refuses_the_other_backward_moves() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = migrated_4(&dir);
        for sql in [
            "UPDATE plans SET phase = 'plan', locked_at = NULL, done_at = NULL WHERE id = 'dn'",
            "UPDATE plans SET phase = 'plan', locked_at = NULL WHERE id = 'pr'",
            "UPDATE plans SET phase = 'grounding', locked_at = NULL WHERE id = 'di'",
            "UPDATE plans SET phase = 'pr', done_at = NULL WHERE id = 'dn'",
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
        assert_eq!(legacy_schema_hash(&conn).unwrap(), SCHEMA_HASHES[4]);
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

    #[test]
    fn a_migrated_database_has_the_schema_of_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        drop(migrated(&dir));
        let fresh = tempfile::tempdir().unwrap();
        let fresh_home = home(&fresh);
        drop(Store::open(&fresh_home, 5000).unwrap());
        let hash = |home: &CpdHome| canonical_hash(&Connection::open(home.db()).unwrap()).unwrap();
        assert_eq!(hash(&home(&dir)), hash(&fresh_home));
        assert_eq!(hash(&fresh_home), SCHEMA_HASH);
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
        assert_eq!(legacy_schema_hash(&conn).unwrap(), SCHEMA_HASHES[1]);
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

    // ---------- named migrations: the conversion (§4.5 C1–C6) ----------

    /// A legacy store at version `n` with a row in every table, as the
    /// integer-era binary left it: `version_2`'s rows (without the columns
    /// migration 2 adds, for version 1), taken through the legacy migrations
    /// to `n`, and a tier and a need.
    fn legacy_store(home: &CpdHome, n: usize) {
        let tier = format!(
            "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0', 'S', 'm', '[]', '{T}');"
        );
        if n == 1 {
            let sql = version_2_sql();
            let v1 = sql
                .replace("pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks,", "pr_seen, pr_polled_at,")
                .replace("'Open PR', 'approved', 'passed', ", "");
            assert_ne!(v1, sql);
            return database_at(home, 1, &format!("{v1}{tier}"));
        }
        database_at(home, 2, &format!("{}{tier}", version_2_sql()));
        let conn = connection(&home.db());
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
        for legacy in &LEGACY[2..n] {
            conn.execute_batch("BEGIN IMMEDIATE").unwrap();
            (legacy.apply)(&conn).unwrap();
            conn.execute_batch("COMMIT").unwrap();
        }
        conn.pragma_update(None, "legacy_alter_table", "OFF").unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.pragma_update(None, "user_version", n as i64).unwrap();
        if n >= 4 {
            conn.execute_batch(&format!("BEGIN; {WEB} INSERT INTO pr_needs (plan, pr, needs) VALUES ('gr', 'web', 'pay'); COMMIT;")).unwrap();
        }
    }

    /// Every row of every table, as JSON text, by table, in rowid order.
    fn dump(conn: &Connection) -> std::collections::BTreeMap<String, Vec<String>> {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        tables
            .into_iter()
            .map(|table| {
                let columns: Vec<String> = table_columns(conn, &table).unwrap().into_iter().map(|(c, _)| format!("\"{c}\"")).collect();
                let sql = format!("SELECT json_array({}) FROM \"{table}\" ORDER BY rowid", columns.join(", "));
                let rows = conn.prepare(&sql).unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_>>().unwrap();
                (table, rows)
            })
            .collect()
    }

    fn rowids(conn: &Connection, table: &str) -> Vec<i64> {
        conn.prepare(&format!("SELECT rowid FROM \"{table}\" ORDER BY rowid"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap()
    }

    fn fresh_canonical_hash() -> String {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        drop(Store::open(&home, 5000).unwrap());
        canonical_hash(&Connection::open(home.db()).unwrap()).unwrap()
    }

    #[test]
    fn a_legacy_store_of_every_version_converts_keeping_every_row_and_id() {
        let fresh = fresh_canonical_hash();
        for n in 1..=LEGACY.len() {
            let dir = tempfile::tempdir().unwrap();
            let home = home(&dir);
            legacy_store(&home, n);
            let before = Connection::open(home.db()).unwrap();
            let rows_before = dump(&before);
            let ids_before: Vec<(String, Vec<i64>)> =
                rows_before.keys().map(|t| (t.clone(), rowids(&before, t))).collect();
            let empty: Vec<&String> = ids_before.iter().filter(|(_, ids)| ids.is_empty()).map(|(t, _)| t).collect();
            assert!(empty.is_empty(), "version {n}: the fixture has no row in {empty:?}");
            drop(before);

            let store = Store::open(&home, 5000).unwrap_or_else(|e| panic!("version {n}: {e:?}"));
            assert!(store.schema_mismatch().is_none(), "version {n}: {:?}", store.schema_mismatch());
            drop(store);

            let conn = Connection::open(home.db()).unwrap();
            let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
            assert_eq!(version, VERSION, "version {n}");
            let violations: i64 =
                conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
            assert_eq!(violations, 0, "version {n}: foreign_key_check");
            for (table, ids) in &ids_before {
                let after = rowids(&conn, table);
                assert!(ids.iter().all(|id| after.contains(id)), "version {n}: {table} lost a row id");
            }
            let recorded: Vec<(String, String, i64, i64)> = conn
                .prepare("SELECT name, compat, seq, converted FROM schema_migrations ORDER BY seq")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_>>()
                .unwrap();
            let expected: Vec<(String, String, i64, i64)> = LEGACY
                .iter()
                .enumerate()
                .map(|(i, l)| (l.name.to_owned(), l.compat.as_str().to_owned(), i as i64 + 1, 1))
                .collect();
            assert_eq!(recorded, expected, "version {n}: six converted rows, one per legacy migration");
            assert_eq!(canonical_hash(&conn).unwrap(), fresh, "version {n}: the canonical hash of a fresh store");

            if n == LEGACY.len() {
                // Nothing but the runner's own rows changes in a store that is already at 6.
                let mut rows_after = dump(&conn);
                let mut rows_before = rows_before;
                assert_eq!(rows_after.remove("schema_migrations").map(|r| r.len()), Some(6));
                let (txns_before, txns_after) = (rows_before.remove("txns").unwrap(), rows_after.remove("txns").unwrap());
                assert_eq!(txns_after[..txns_before.len()], txns_before[..], "the migrating transaction only adds a txns row");
                assert_eq!(txns_after.len(), txns_before.len() + 1);
                assert_eq!(rows_after, rows_before);
            }

            // Converting twice is a no-op.
            let snapshot = dump(&conn);
            drop(conn);
            drop(Store::open(&home, 5000).unwrap());
            assert_eq!(dump(&Connection::open(home.db()).unwrap()), snapshot, "version {n}: the second open writes nothing");
        }
    }

    #[test]
    fn a_fresh_store_records_its_legacy_migrations_as_not_converted() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        drop(Store::open(&home, 5000).unwrap());
        let conn = Connection::open(home.db()).unwrap();
        let converted: Vec<i64> = conn
            .prepare("SELECT converted FROM schema_migrations ORDER BY seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(converted, vec![0; LEGACY.len()]);
        let txns: i64 = conn.query_row("SELECT count(*) FROM schema_migrations s JOIN txns t ON t.id = s.txn", [], |r| r.get(0)).unwrap();
        assert_eq!(txns, LEGACY.len() as i64, "every row names the migrating transaction");
    }

    #[test]
    fn a_tampered_legacy_store_is_refused_and_left_as_it_was() {
        for n in [1, 3, LEGACY.len()] {
            for tamper in ["DROP TRIGGER contexts_no_delete", "CREATE INDEX stray ON contexts(title)"] {
                let dir = tempfile::tempdir().unwrap();
                let home = home(&dir);
                legacy_store(&home, n);
                let conn = connection(&home.db());
                conn.execute_batch(tamper).unwrap();
                let before = (dump(&conn), legacy_schema_hash(&conn).unwrap());
                drop(conn);

                let err = Store::open(&home, 5000).err().unwrap_or_else(|| panic!("version {n}, {tamper}: opened"));
                assert_eq!(err.code(), ErrorCode::StoreTampered, "version {n}, {tamper}: {err:?}");
                assert_eq!(err.details()["version"], n);

                let conn = connection(&home.db());
                let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
                assert_eq!(version, n as i64);
                assert_eq!((dump(&conn), legacy_schema_hash(&conn).unwrap()), before, "version {n}, {tamper}: changed");
            }
        }
    }

    // ---------- the canonical form (§4.3) ----------

    #[test]
    fn normalizing_deletes_comments_and_collapses_whitespace_outside_quoted_runs() {
        assert_eq!(normalize("  a  /* x */ b -- c\n d 'x  --y' [a  b] \"q\"\"  r\" `s  t`  "), "a b d 'x  --y' [a  b] \"q\"\"  r\" `s  t`");
        assert_eq!(normalize("a/**/b"), "a b");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn a_table_is_split_into_sorted_columns_ordered_constraints_and_options() {
        let parts = table_parts(
            "CREATE TABLE \"t\" (\n  \"Zed\" TEXT, -- a, comment (with parens\n  b TEXT DEFAULT 'x,y' CHECK (b IN ('a', 'b')),\n  \
             [a c] INTEGER,\n  \"check\" TEXT,\n  CONSTRAINT k UNIQUE (b, \"Zed\"),\n  check (length(b) > 0)\n) STRICT, WITHOUT ROWID",
        )
        .unwrap();
        let keys: Vec<&str> = parts.columns.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a c", "b", "check", "zed"]);
        assert_eq!(parts.columns[1].1, "b TEXT DEFAULT 'x,y' CHECK (b IN ('a', 'b'))");
        assert_eq!(parts.constraints, ["CONSTRAINT k UNIQUE (b, \"Zed\")", "check (length(b) > 0)"]);
        assert_eq!(parts.options, "STRICT, WITHOUT ROWID");
    }

    #[test]
    fn add_column_in_either_order_gives_one_canonical_table() {
        let canonical = |statements: &[&str]| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, CHECK (a <> 'x')) STRICT;").unwrap();
            for statement in statements {
                conn.execute_batch(statement).unwrap();
            }
            canonical_entries(&conn).unwrap()
        };
        let (b, c) = ("ALTER TABLE t ADD COLUMN b TEXT", "ALTER TABLE t ADD COLUMN c TEXT NOT NULL DEFAULT ''");
        assert_eq!(canonical(&[b, c]), canonical(&[c, b]));
        assert_ne!(canonical(&[b]), canonical(&[c]));
    }

    #[test]
    fn a_changed_generated_trigger_or_object_is_a_schema_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        for (tamper, trigger) in [
            ("DROP TRIGGER plans_no_delete", Some("plans_no_delete")),
            ("DROP TRIGGER plans_hist_upd; CREATE TRIGGER plans_hist_upd AFTER UPDATE ON plans BEGIN SELECT 1; END", Some("plans_hist_upd")),
            ("DROP TRIGGER plans_phase", None),
            ("CREATE INDEX stray ON contexts (title)", None),
            ("CREATE TRIGGER stray AFTER INSERT ON contexts BEGIN SELECT 1; END", None),
        ] {
            let home = CpdHome::resolve(Some(dir.path().join(format!("{}", tamper.len())).into()), None).unwrap();
            drop(Store::open(&home, 5000).unwrap());
            connection(&home.db()).execute_batch(tamper).unwrap();
            let store = Store::open(&home, 5000).unwrap();
            let mismatch = store.schema_mismatch().unwrap_or_else(|| panic!("{tamper}: no mismatch"));
            assert_eq!(mismatch.object.as_deref(), trigger, "{tamper}");
            let err = { let mut store = store; store.write("test", |_| Ok(())).unwrap_err() };
            assert_eq!(err.code(), ErrorCode::StoreTampered, "{tamper}");
        }
    }
}
