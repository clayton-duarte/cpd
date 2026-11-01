//! The migration rules, enforced (contract §4.5 "The checks", N5, O2):
//!
//! - the **additive test** (A2): every migration declared `additive` is run on
//!   the name-ordered prefix before it under an SQLite authorizer that allows
//!   only A1's statements, and the catalog diff shows that nothing that existed
//!   was dropped or changed;
//! - the **order test** (A3): `breaking after <n>` is the number of names that
//!   sort before it, and the declarations agree with the list `build.rs` made;
//! - the **frozen check** (N5): `scripts/check-migrations-frozen.sh`;
//! - `scripts/new-migration.sh` (A4);
//! - cpd names its columns (O2): no `SELECT *`, no `INSERT` without a column list.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use cpd_core::home::CpdHome;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::Connection;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("core/ has a parent").to_owned()
}

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/migrations")
}

// ---------------------------------------------------------------------------
// A2.1: the statement kinds
// ---------------------------------------------------------------------------

/// The tables an additive migration may insert into besides its own (A1).
const INSERTABLE: &[&str] = &["event_kinds", "record_kinds", "history_tables"];

/// `sqlite_master`, `sqlite_schema`, `sqlite_temp_master`, `sqlite_sequence`.
fn is_catalog(table: &str) -> bool {
    table.starts_with("sqlite_")
}

/// Runs `sql` on `conn` under an authorizer that allows only A1's actions:
/// `CREATE TABLE`, `CREATE INDEX`, `CREATE TRIGGER`, `ALTER TABLE`, `INSERT`
/// into [`INSERTABLE`] or a table the migration itself created, and reads.
/// Denies `UPDATE`, `DELETE`, every `DROP`, `PRAGMA` and anything else. The
/// actions of a trigger that fires (an `accessor`) are the store's own and are
/// not judged.
fn run_authorized(conn: &Connection, sql: &str) -> Result<(), String> {
    let created: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let denied: Arc<Mutex<Option<String>>> = Arc::default();
    let (created_by, denied_by) = (Arc::clone(&created), Arc::clone(&denied));
    conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if ctx.accessor.is_some() {
            return Authorization::Allow;
        }
        let verdict = match ctx.action {
            AuthAction::Select | AuthAction::Read { .. } | AuthAction::Function { .. } | AuthAction::Recursive => Ok(()),
            AuthAction::CreateTable { table_name } => {
                created_by.lock().unwrap().insert(table_name.to_owned());
                Ok(())
            }
            AuthAction::CreateIndex { .. } | AuthAction::CreateTrigger { .. } | AuthAction::AlterTable { .. } => Ok(()),
            AuthAction::Insert { table_name }
                if INSERTABLE.contains(&table_name)
                    || created_by.lock().unwrap().contains(table_name)
                    || is_catalog(table_name) =>
            {
                Ok(())
            }
            // SQLite rewrites its own catalog for CREATE, ALTER and DROP; a statement of the
            // migration cannot write it (`writable_schema` is a PRAGMA, denied).
            AuthAction::Update { table_name, .. } | AuthAction::Delete { table_name } if is_catalog(table_name) => Ok(()),
            // Also SQLite's own, for statements A1 allows: `CREATE INDEX` on a table with rows
            // reindexes it, and `ADD COLUMN` with a `CHECK` or `NOT NULL` runs `quick_check` on
            // the table. Both only read.
            AuthAction::Reindex { .. } | AuthAction::Pragma { pragma_name: "quick_check", .. } => Ok(()),
            other => Err(format!("{other:?}")),
        };
        match verdict {
            Ok(()) => Authorization::Allow,
            Err(action) => {
                denied_by.lock().unwrap().get_or_insert(action);
                Authorization::Deny
            }
        }
    }))
    .expect("the authorizer is set");
    let result = conn.execute_batch(sql);
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).expect("the authorizer is cleared");
    match (result, denied.lock().unwrap().take()) {
        (_, Some(action)) => Err(format!("the statement kinds: {action} is not allowed in an additive migration (A1)")),
        (Err(e), None) => Err(format!("the migration failed to run: {e}")),
        (Ok(()), None) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// A2.2: the catalog diff
// ---------------------------------------------------------------------------

/// One column as `pragma_table_xinfo` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Column {
    name: String,
    declared_type: String,
    not_null: bool,
    default: Option<String>,
    pk: i64,
    hidden: i64,
}

/// What `sqlite_schema` and the pragmas say about a database.
struct Catalog {
    /// `(type, name)` to `(tbl_name, sql)`.
    objects: BTreeMap<(String, String), (String, Option<String>)>,
    columns: BTreeMap<String, Vec<Column>>,
    /// Index name to whether it is `UNIQUE`.
    unique: BTreeMap<String, bool>,
}

fn read_catalog(conn: &Connection) -> Catalog {
    let mut objects = BTreeMap::new();
    let mut stmt = conn
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE substr(name, 1, 7) <> 'sqlite_'")
        .unwrap();
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?))).unwrap() {
        let (kind, name, table, sql) = row.unwrap();
        objects.insert((kind, name), (table, sql));
    }
    let mut columns = BTreeMap::new();
    let mut unique = BTreeMap::new();
    for (kind, name) in objects.keys() {
        if kind != "table" {
            continue;
        }
        let mut stmt = conn
            .prepare("SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?1) ORDER BY cid")
            .unwrap();
        let cols = stmt
            .query_map([name], |r| {
                Ok(Column {
                    name: r.get(0)?,
                    declared_type: r.get(1)?,
                    not_null: r.get::<_, i64>(2)? != 0,
                    default: r.get(3)?,
                    pk: r.get(4)?,
                    hidden: r.get(5)?,
                })
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        columns.insert(name.clone(), cols);
        let mut stmt = conn.prepare("SELECT name, \"unique\" FROM pragma_index_list(?1)").unwrap();
        for row in stmt.query_map([name], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))).unwrap() {
            let (index, is_unique) = row.unwrap();
            unique.insert(index, is_unique);
        }
    }
    Catalog { objects, columns, unique }
}

/// A2.2: what `after` may differ from `before` by, for an additive migration.
fn catalog_diff(before: &Catalog, after: &Catalog) -> Result<(), String> {
    for (table, old) in &before.columns {
        let Some(new) = after.columns.get(table) else {
            return Err(format!("the catalog diff: table {table} was dropped"));
        };
        if new.len() < old.len() || new[..old.len()] != old[..] {
            let at = old
                .iter()
                .zip(new)
                .position(|(a, b)| a != b)
                .map_or_else(|| "its end".to_owned(), |i| format!("column {}", old[i].name));
            return Err(format!("the catalog diff: the columns of table {table} are not an unchanged prefix (differs at {at})"));
        }
        for column in &new[old.len()..] {
            if column.not_null && column.default.is_none() {
                return Err(format!("the catalog diff: new column {table}.{} is NOT NULL without a DEFAULT", column.name));
            }
        }
    }
    for ((kind, name), (table, sql)) in &before.objects {
        let Some((after_table, after_sql)) = after.objects.get(&(kind.clone(), name.clone())) else {
            return Err(format!("the catalog diff: {kind} {name} was dropped"));
        };
        if after_table != table {
            return Err(format!("the catalog diff: {kind} {name} moved from table {table} to {after_table}"));
        }
        // A table's SQL may grow, and only by ADD COLUMN: the columns are checked above.
        let grew = kind == "table" && after.columns[name].len() > before.columns[name].len();
        if after_sql != sql && !grew {
            return Err(format!("the catalog diff: the SQL of {kind} {name} changed"));
        }
    }
    for ((kind, name), (table, _)) in &after.objects {
        if before.objects.contains_key(&(kind.clone(), name.clone())) {
            continue;
        }
        let on_old_table = before.objects.contains_key(&("table".to_owned(), table.clone()));
        match kind.as_str() {
            "trigger" if on_old_table => {
                return Err(format!("the catalog diff: new trigger {name} is on existing table {table}"));
            }
            "index" if on_old_table && after.unique[name] => {
                return Err(format!("the catalog diff: new index {name} on existing table {table} is UNIQUE"));
            }
            "view" => return Err(format!("the catalog diff: new view {name}")),
            _ => {}
        }
    }
    Ok(())
}

/// A2: runs the additive migration `sql` on `base` and judges it.
fn check_additive(base: &Connection, sql: &str) -> Result<(), String> {
    let before = read_catalog(base);
    run_authorized(base, sql)?;
    catalog_diff(&before, &read_catalog(base))
}

// ---------------------------------------------------------------------------
// The list's own tests (A2, A3)
// ---------------------------------------------------------------------------

fn home_in(dir: &tempfile::TempDir) -> CpdHome {
    CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
}

/// The first line of a migration: `Ok(None)` for `additive`, `Ok(Some(n))` for `breaking after n`.
fn declaration(sql: &str) -> Result<Option<usize>, String> {
    let first = sql.lines().next().unwrap_or_default();
    match first.strip_prefix("-- cpd-migration: ") {
        Some("additive") => Ok(None),
        Some(rest) => rest
            .strip_prefix("breaking after ")
            .and_then(|n| n.parse().ok())
            .map(Some)
            .ok_or_else(|| format!("malformed declaration {first:?}")),
        None => Err(format!("no declaration, the first line is {first:?}")),
    }
}

#[test]
fn every_additive_migration_is_additive_on_its_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_in(&dir);
    for (index, (name, compat, sql)) in named_migrations().into_iter().enumerate() {
        if compat != "additive" {
            continue;
        }
        let prefix = prefix_database(index, &home).unwrap();
        if let Err(why) = check_additive(&prefix, sql) {
            panic!("{name} is declared additive but is not: {why}. Declare it `breaking after <n>` (A1) or change it.");
        }
    }
}

/// O3: a breaking migration's `breaking after n` is checked against the names
/// that sort before it. A migration already on `origin/main` has a historical
/// header: it may keep a stale `n` (one no larger than the count), but a
/// migration the PR adds must count the names before it exactly. `on_main`
/// answers whether a name is in `origin/main`.
fn check_breaking_headers(
    list: &[(&str, &str, &str)],
    legacy: usize,
    on_main: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    for (index, (name, compat, sql)) in list.iter().enumerate() {
        match declaration(sql).map_err(|why| format!("{name}: {why}"))? {
            None => {
                if *compat != "additive" {
                    return Err(format!("{name}: the list says {compat}, the header says additive"));
                }
            }
            Some(n) => {
                if *compat != "breaking" {
                    return Err(format!("{name}: the list says {compat}, the header says breaking"));
                }
                let expected = legacy + index;
                if on_main(name) {
                    if n > expected {
                        return Err(format!(
                            "{name} says `breaking after {n}` but only {expected} migrations sort before it. \
                             A migration already on main has a historical header and is never renamed; it may \
                             not claim more names than exist (O3)."
                        ));
                    }
                } else if n != expected {
                    return Err(format!(
                        "{name} says `breaking after {n}` but {expected} migrations sort before it. A migration \
                         the PR adds may be renumbered to a newer timestamp before it lands \
                         (scripts/new-migration.sh); re-check its rebuild against what landed, and write the new \
                         count (O3)."
                    ));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn every_breaking_migration_counts_the_names_before_it_and_the_headers_match_the_list() {
    let list = named_migrations();
    let names: Vec<&str> = list.iter().map(|(name, ..)| *name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "the list is sorted by name (N4)");

    // The legacy files `NNNN-<slug>.sql` sort before every named one (N2).
    let legacy = fs::read_dir(migrations_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|f| f.ends_with(".sql") && f.split_once('-').is_some_and(|(n, _)| n.len() == 4 && n.bytes().all(|c| c.is_ascii_digit())))
        .count();
    let on_main = |name: &str| {
        Command::new("git")
            .args(["cat-file", "-e", &format!("origin/main:core/src/store/migrations/{name}.sql")])
            .current_dir(repo_root())
            .status()
            .is_ok_and(|status| status.success())
    };
    check_breaking_headers(&list, legacy, &on_main).unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn a_pr_added_breaking_migration_must_count_the_names_before_it() {
    let list = [("20300101T0000-add-a", "breaking", "-- cpd-migration: breaking after 0\nSELECT 1;\n")];
    let why = check_breaking_headers(&list, 2, &|_| false).unwrap_err();
    assert!(why.contains("breaking after 0") && why.contains("2 migrations sort before it"), "{why}");
}

#[test]
fn a_breaking_migration_on_main_may_keep_a_stale_count() {
    let list = [("20261025T2300-queue-failed", "breaking", "-- cpd-migration: breaking after 2\nSELECT 1;\n")];
    check_breaking_headers(&list, 3, &|_| true).unwrap();
}

#[test]
fn a_declaration_is_parsed_exactly() {
    assert_eq!(declaration("-- cpd-migration: additive\nSELECT 1;"), Ok(None));
    assert_eq!(declaration("-- cpd-migration: breaking after 7\n"), Ok(Some(7)));
    for bad in ["", "SELECT 1;", "-- cpd-migration: breaking\n", "-- cpd-migration: breaking after x\n", "-- cpd-migration: other\n"] {
        assert!(declaration(bad).is_err(), "{bad:?}");
    }
}

// ---------------------------------------------------------------------------
// The checks catch what they are for (fake migrations)
// ---------------------------------------------------------------------------

/// Today's schema: every migration of the list applied.
fn current_schema(dir: &tempfile::TempDir) -> Connection {
    prefix_database(named_migrations().len(), &home_in(dir)).unwrap()
}

/// A small database with the lookup tables A1 names, for the cases that need them.
fn small() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE event_kinds (name TEXT PRIMARY KEY) STRICT;
         CREATE TABLE history_tables (name TEXT PRIMARY KEY, no_delete INTEGER NOT NULL) STRICT;
         CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, b INTEGER NOT NULL DEFAULT 0) STRICT;
         CREATE INDEX t_a ON t (a);",
    )
    .unwrap();
    conn
}

fn refused(base: &Connection, sql: &str, containing: &str) {
    match check_additive(base, sql) {
        Ok(()) => panic!("expected a refusal containing {containing:?} for:\n{sql}"),
        Err(why) => assert!(why.contains(containing), "refused for another reason, wanted {containing:?}: {why}"),
    }
}

fn diff_of(before: &str, after: &str) -> Result<(), String> {
    let (b, a) = (Connection::open_in_memory().unwrap(), Connection::open_in_memory().unwrap());
    b.execute_batch(before).unwrap();
    a.execute_batch(after).unwrap();
    catalog_diff(&read_catalog(&b), &read_catalog(&a))
}

#[test]
fn a_well_formed_additive_migration_passes_on_today_s_schema() {
    let dir = tempfile::tempdir().unwrap();
    let conn = current_schema(&dir);
    let sql = "-- cpd-migration: additive
        CREATE TABLE fake_notes (id INTEGER PRIMARY KEY, plan TEXT NOT NULL REFERENCES plans(id), text TEXT NOT NULL) STRICT;
        CREATE INDEX fake_notes_plan ON fake_notes (plan);
        CREATE UNIQUE INDEX fake_notes_text ON fake_notes (text);
        CREATE TRIGGER fake_notes_ai AFTER INSERT ON fake_notes BEGIN SELECT 1; END;
        INSERT INTO fake_notes (plan, text) SELECT id, 'x' FROM plans;
        ALTER TABLE plans ADD COLUMN fake_note TEXT;
        ALTER TABLE jobs ADD COLUMN fake_count INTEGER NOT NULL DEFAULT 0 CHECK (fake_count >= 0);
        CREATE INDEX plans_fake_note ON plans (fake_note);";
    check_additive(&conn, sql).unwrap();
}

#[test]
fn an_additive_migration_that_drops_a_column_is_refused() {
    refused(&small(), "ALTER TABLE t DROP COLUMN b;", "not an unchanged prefix");
    // The rebuild that drops columns of a real table needs DROP TABLE.
    let dir = tempfile::tempdir().unwrap();
    refused(
        &current_schema(&dir),
        "CREATE TABLE plans_new (id TEXT PRIMARY KEY) STRICT;
         INSERT INTO plans_new (id) SELECT id FROM plans;
         DROP TABLE plans;
         ALTER TABLE plans_new RENAME TO plans;",
        "DropTable",
    );
}

#[test]
fn an_additive_migration_that_renames_or_retypes_a_column_is_refused() {
    refused(&small(), "ALTER TABLE t RENAME COLUMN b TO c;", "not an unchanged prefix");
    let dir = tempfile::tempdir().unwrap();
    refused(
        &current_schema(&dir),
        "CREATE TABLE sessions_new (id INTEGER PRIMARY KEY) STRICT;
         DROP TABLE sessions;
         ALTER TABLE sessions_new RENAME TO sessions;",
        "DropTable",
    );
    // A retype is only possible by a rebuild; the catalog diff sees it even where DROP is not caught.
    let why = diff_of("CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER)", "CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT)")
        .unwrap_err();
    assert!(why.contains("not an unchanged prefix") && why.contains("column a"), "{why}");
}

#[test]
fn the_catalog_diff_sees_a_changed_constraint_a_dropped_object_and_a_required_new_column() {
    let base = "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER); CREATE INDEX t_a ON t (a);";
    let why = diff_of(base, "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER CHECK (a > 0)); CREATE INDEX t_a ON t (a);").unwrap_err();
    assert!(why.contains("the SQL of table t changed"), "{why}");
    let why = diff_of(base, "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER);").unwrap_err();
    assert!(why.contains("index t_a was dropped"), "{why}");
    let why = diff_of(base, "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, c TEXT NOT NULL); CREATE INDEX t_a ON t (a);").unwrap_err();
    assert!(why.contains("NOT NULL without a DEFAULT"), "{why}");
}

#[test]
fn an_additive_migration_may_not_write_or_remove_existing_rows_or_objects() {
    let conn = small();
    for (sql, action) in [
        ("UPDATE t SET a = 'x';", "Update"),
        ("DELETE FROM t;", "Delete"),
        ("DROP TABLE t;", "DropTable"),
        ("DROP INDEX t_a;", "DropIndex"),
        ("INSERT INTO t (a) VALUES ('x');", "Insert"),
        ("PRAGMA user_version = 7;", "Pragma"),
        ("CREATE VIEW v AS SELECT a FROM t;", "CreateView"),
        ("CREATE TEMP TABLE scratch (x);", "CreateTempTable"),
    ] {
        refused(&conn, sql, action);
    }
}

#[test]
fn an_additive_migration_may_insert_kinds_and_history_tables_rows() {
    check_additive(
        &small(),
        "INSERT INTO event_kinds (name) VALUES ('fake-kind');
         CREATE TABLE fake (id INTEGER PRIMARY KEY) STRICT;
         INSERT INTO history_tables (name, no_delete) VALUES ('fake', 1);",
    )
    .unwrap();
}

#[test]
fn an_additive_migration_may_not_add_a_required_column_a_unique_index_or_a_trigger_on_an_existing_table() {
    let conn = small();
    // SQLite accepts a NOT NULL column without a DEFAULT on an empty table; old code would not.
    refused(&conn, "ALTER TABLE t ADD COLUMN c TEXT NOT NULL;", "NOT NULL without a DEFAULT");
    refused(&conn, "CREATE UNIQUE INDEX t_a_unique ON t (a);", "is UNIQUE");
    refused(&conn, "CREATE TRIGGER t_ai AFTER INSERT ON t BEGIN SELECT 1; END;", "is on existing table t");
}

// ---------------------------------------------------------------------------
// N5: scripts/check-migrations-frozen.sh
// ---------------------------------------------------------------------------

/// The breaking migration's text: the header plus a body that a renumber may not touch.
/// The body is long enough that git still reads the file as renamed when only its header
/// (the `after n` count) changes, as the N5 exception allows.
const BREAKING_TEXT: &str = "\
-- cpd-migration: breaking after 1
DROP TRIGGER jobs_status;
CREATE TRIGGER jobs_status AFTER UPDATE ON jobs BEGIN
  SELECT RAISE(ABORT, 'illegal_transition');
END;
";

/// The base tree: a legacy migration, a breaking one and a named additive one.
const MAIN_FILES: [(&str, &str); 3] = [
    ("0001-initial.sql", "CREATE TABLE a (x);\n"),
    ("20260101T0000-kind-tables.sql", BREAKING_TEXT),
    ("20260202T0000-additive.sql", "-- cpd-migration: additive\n"),
];

/// An edit of a scratch clone.
type CloneEdit = fn(&Clone);

/// A scratch clone (`work/`) of a bare `origin.git` whose `main` has [`MAIN_FILES`];
/// HEAD is a branch off it.
struct Clone {
    dir: tempfile::TempDir,
}

/// An environment in which `git` ignores the user's configuration.
fn git_env(home: &Path) -> Vec<(&'static str, String)> {
    let home = home.display().to_string();
    vec![
        ("HOME", home.clone()),
        ("XDG_CONFIG_HOME", home),
        ("GIT_CONFIG_NOSYSTEM", "1".to_owned()),
        ("GIT_AUTHOR_NAME", "t".to_owned()),
        ("GIT_AUTHOR_EMAIL", "t@t".to_owned()),
        ("GIT_COMMITTER_NAME", "t".to_owned()),
        ("GIT_COMMITTER_EMAIL", "t@t".to_owned()),
    ]
}

fn git_in(root: &Path, cwd: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(cwd).envs(git_env(root)).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

impl Clone {
    fn git(&self, args: &[&str]) {
        git_in(self.dir.path(), &self.work(), args);
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn migrations(&self) -> PathBuf {
        self.work().join("core/src/store/migrations")
    }

    fn new() -> Clone {
        let dir = tempfile::tempdir().unwrap();
        git_in(dir.path(), dir.path(), &["init", "--quiet", "--bare", "--initial-branch=main", "origin.git"]);
        git_in(dir.path(), dir.path(), &["clone", "--quiet", "origin.git", "work"]);
        let clone = Clone { dir };
        fs::create_dir_all(clone.migrations()).unwrap();
        for (name, text) in MAIN_FILES {
            fs::write(clone.migrations().join(name), text).unwrap();
        }
        clone.git(&["add", "-A"]);
        clone.git(&["commit", "--quiet", "-m", "main"]);
        clone.git(&["push", "--quiet", "origin", "main"]);
        clone.git(&["checkout", "--quiet", "-b", "branch"]);
        clone
    }

    fn commit(&self) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "--quiet", "-m", "branch"]);
    }

    /// Runs the script from the clone: `Ok` when it passes, else its stderr.
    fn check(&self) -> Result<(), String> {
        let out = Command::new(repo_root().join("scripts/check-migrations-frozen.sh"))
            .current_dir(self.work())
            .envs(git_env(self.dir.path()))
            .output()
            .unwrap();
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
    }
}

#[test]
fn the_frozen_check_passes_on_no_change_and_on_a_new_migration() {
    let clone = Clone::new();
    clone.check().unwrap();
    fs::write(clone.migrations().join("20260202T0000-more.sql"), "-- cpd-migration: additive\n").unwrap();
    clone.commit();
    clone.check().unwrap();
}

/// The module is named `migrations` (not `tests`) so nextest prefixes each test with the
/// file: `migrations::<fn>` is the name the Job's acceptance lists.
mod migrations {
    use super::*;

    /// hs-d697: a breaking migration may be restored to the name a live store applied with
    /// its body unchanged, even when that older name sorts before its current one.
    #[test]
    fn the_frozen_check_passes_on_a_breaking_restore_to_an_older_name() {
        let clone = Clone::new();
        let old = clone.migrations().join(MAIN_FILES[1].0);
        let new = clone.migrations().join("20251201T0000-kind-tables.sql");
        // The body is untouched; the older name still has the one legacy migration before it.
        fs::rename(&old, &new).unwrap();
        clone.commit();
        clone.check().unwrap();
    }

    /// hs-d700: when a restore shifts the count, a breaking migration's `breaking after <n>`
    /// header may be corrected in place while every line after the first stays byte-identical.
    #[test]
    fn the_frozen_check_passes_on_a_header_only_correction_of_a_breaking_migration() {
        let clone = Clone::new();
        let file = clone.migrations().join(MAIN_FILES[1].0);
        fs::write(&file, BREAKING_TEXT.replace("breaking after 1", "breaking after 2")).unwrap();
        clone.commit();
        clone.check().unwrap();
    }

    /// hs-d704: a breaking migration that is already on the base may not be renumbered to
    /// a newer name, even with its body untouched; only the hs-d700 restore (back to an
    /// older name a live store already applied) is allowed.
    #[test]
    fn the_frozen_check_refuses_a_renumber_of_a_migration_on_the_base() {
        let clone = Clone::new();
        let old = clone.migrations().join(MAIN_FILES[1].0);
        let new = clone.migrations().join("20260301T0000-kind-tables.sql");
        fs::rename(&old, &new).unwrap();
        clone.commit();
        let why = clone.check().expect_err("a base migration may not be renumbered");
        assert!(why.contains("frozen check failed") && why.contains(MAIN_FILES[1].0), "{why}");
    }

    /// hs-d704: a migration the branch adds is not on the base, so renumbering it to a
    /// newer name before it lands is not a change to a frozen file.
    #[test]
    fn the_frozen_check_passes_on_a_renumber_of_a_migration_the_pr_adds() {
        let clone = Clone::new();
        let added = clone.migrations().join("20260301T0000-added.sql");
        fs::write(&added, "-- cpd-migration: breaking after 3\nSELECT 1;\n").unwrap();
        clone.commit();
        let renamed = clone.migrations().join("20260401T0000-added.sql");
        fs::rename(&added, &renamed).unwrap();
        clone.commit();
        clone.check().unwrap();
    }

    /// pro-d120: the one-off exception: the breaking 20261031T1000-deploy-kind may be renamed
    /// forward to 20261102T0900-deploy-kind when only its first line changes.
    #[test]
    fn the_frozen_check_passes_on_the_one_off_forward_rename_of_deploy_kind() {
        let clone = Clone::new();
        let body: String = (0..20).map(|i| format!("SELECT {i} FROM jobs;\n")).collect();
        let old = clone.migrations().join("20261031T1000-deploy-kind.sql");
        fs::write(&old, format!("-- cpd-migration: breaking after 63\n{body}")).unwrap();
        clone.commit();
        // The file is on the base: main now holds it.
        clone.git(&["push", "--quiet", "origin", "branch:main"]);
        let new = clone.migrations().join("20261102T0900-deploy-kind.sql");
        fs::rename(&old, &new).unwrap();
        fs::write(&new, format!("-- cpd-migration: breaking after 66\n{body}")).unwrap();
        clone.commit();
        clone.check().unwrap();
    }

    const NEW_BREAKING: &str = "-- cpd-migration: breaking after 2\nSELECT 1;\n";
    const DIR: &str = "core/src/store/migrations";

    fn refused_for(why: &str, new: &str) {
        assert!(why.contains(new) && why.contains("20260202T0000-additive"), "{why}");
    }

    /// pro-d122 (#526): a new breaking migration must sort after every name on the base.
    #[test]
    fn the_frozen_check_refuses_a_new_breaking_migration_that_sorts_before_a_name_on_the_base() {
        let clone = Clone::new();
        fs::write(clone.migrations().join("20260202T0000-aaa-breaking.sql"), NEW_BREAKING).unwrap();
        clone.commit();
        let why = clone.check().expect_err("sorts before a base name");
        refused_for(&why, "20260202T0000-aaa-breaking");
    }

    /// pro-d122: the same when the breaking migration is renamed in to a too-old name.
    #[test]
    fn the_frozen_check_refuses_a_renamed_in_breaking_migration_that_sorts_before_a_name_on_the_base() {
        let clone = Clone::new();
        fs::write(clone.migrations().join("20260301T0000-aaa-breaking.sql"), NEW_BREAKING).unwrap();
        clone.commit();
        clone.git(&["mv", &format!("{DIR}/20260301T0000-aaa-breaking.sql"), &format!("{DIR}/20260202T0000-aaa-breaking.sql")]);
        clone.commit();
        let why = clone.check().expect_err("renamed before a base name");
        refused_for(&why, "20260202T0000-aaa-breaking");
    }

    /// The remedy: a name after every base name passes.
    #[test]
    fn the_frozen_check_passes_a_new_breaking_migration_that_sorts_after_every_name_on_the_base() {
        let clone = Clone::new();
        fs::write(clone.migrations().join("20260303T0000-zzz-breaking.sql"), NEW_BREAKING).unwrap();
        clone.commit();
        clone.check().unwrap();
    }

    /// O2: additive migrations may sort anywhere.
    #[test]
    fn the_frozen_check_passes_a_new_additive_migration_that_sorts_before_a_name_on_the_base() {
        let clone = Clone::new();
        fs::write(clone.migrations().join("20260202T0000-aaa-additive.sql"), "-- cpd-migration: additive\n").unwrap();
        clone.commit();
        clone.check().unwrap();
    }
}

/// Everything that is not one of those allowances still fails: an edit, a delete, and a
/// rename of a migration without the breaking header (legacy or additive).
#[test]
fn the_frozen_check_still_fails_on_an_edit_a_delete_or_an_additive_rename() {
    let cases: [(&str, &str, CloneEdit); 4] = [
        ("modified", MAIN_FILES[0].0, |c| {
            fs::write(c.migrations().join(MAIN_FILES[0].0), "CREATE TABLE a (y);\n").unwrap()
        }),
        ("deleted", MAIN_FILES[0].0, |c| fs::remove_file(c.migrations().join(MAIN_FILES[0].0)).unwrap()),
        ("legacy renamed", MAIN_FILES[0].0, |c| {
            fs::rename(c.migrations().join(MAIN_FILES[0].0), c.migrations().join("20260202T0000-initial.sql")).unwrap()
        }),
        ("additive renamed", MAIN_FILES[2].0, |c| {
            fs::rename(c.migrations().join(MAIN_FILES[2].0), c.migrations().join("20260302T0000-additive.sql")).unwrap()
        }),
    ];
    for (what, file, change) in cases {
        let clone = Clone::new();
        change(&clone);
        clone.commit();
        let why = clone.check().expect_err(what);
        assert!(why.contains("frozen check failed") && why.contains(file), "{what}: {why}");
    }
}

/// A breaking rename that changes a body line is refused, however it is renamed.
#[test]
fn the_frozen_check_fails_on_a_rename_that_changes_the_body() {
    let clone = Clone::new();
    let old = clone.migrations().join(MAIN_FILES[1].0);
    let new = clone.migrations().join("20260301T0000-kind-tables.sql");
    fs::rename(&old, &new).unwrap();
    fs::write(&new, BREAKING_TEXT.replace("DROP TRIGGER jobs_status;", "DROP TRIGGER jobs_status_old;")).unwrap();
    clone.commit();
    let why = clone.check().expect_err("body changed");
    assert!(why.contains("frozen check failed") && why.contains(MAIN_FILES[1].0), "body changed: {why}");
}

#[test]
fn the_frozen_check_fails_when_it_cannot_fetch_origin_main() {
    let clone = Clone::new();
    clone.git(&["remote", "set-url", "origin", "/nonexistent/origin.git"]);
    let why = clone.check().expect_err("a failed fetch fails the check");
    assert!(why.contains("repository"), "{why}");
}

// ---------------------------------------------------------------------------
// A4: scripts/new-migration.sh
// ---------------------------------------------------------------------------

/// A scratch tree with the script and the migration files `existing`.
fn script_tree(existing: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("scripts");
    let migrations = dir.path().join("core/src/store/migrations");
    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&migrations).unwrap();
    fs::copy(repo_root().join("scripts/new-migration.sh"), scripts.join("new-migration.sh")).unwrap();
    fs::copy(repo_root().join("scripts/sync-version.sh"), scripts.join("sync-version.sh")).unwrap();
    // sync-version.sh, which new-migration.sh runs, reads and rewrites these.
    fs::create_dir_all(dir.path().join("core/src/events")).unwrap();
    for f in ["core/src/events/kind.rs", "Cargo.toml", "Cargo.lock"] {
        fs::copy(repo_root().join(f), dir.path().join(f)).unwrap();
    }
    for name in existing {
        fs::write(migrations.join(name), "-- cpd-migration: additive\n").unwrap();
    }
    (dir, migrations)
}

fn new_migration(dir: &Path, args: &[&str]) -> Result<PathBuf, String> {
    let out = Command::new("sh").arg(dir.join("scripts/new-migration.sh")).args(args).output().unwrap();
    if out.status.success() {
        Ok(PathBuf::from(String::from_utf8(out.stdout).unwrap().trim()))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `<YYYYMMDDTHHMM>-<slug>`, as `build.rs` accepts it.
fn is_named(stem: &str) -> bool {
    let Some((stamp, slug)) = stem.split_once('-') else { return false };
    let b = stamp.as_bytes();
    b.len() == 13
        && b[8] == b'T'
        && b.iter().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit())
        && (1..=40).contains(&slug.len())
        && slug.starts_with(|c: char| c.is_ascii_lowercase())
        && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[test]
fn new_migration_creates_a_named_file_with_its_declaration() {
    let (dir, migrations) = script_tree(&["0001-initial.sql", "0002-more.sql"]);
    for args in [&["plan-pins"][..], &["pins-two", "additive"]] {
        let path = new_migration(dir.path(), args).unwrap();
        assert_eq!(path.parent().unwrap(), migrations);
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert!(path.extension().is_some_and(|e| e == "sql") && is_named(stem), "{path:?}");
        assert!(stem.ends_with(args[0]), "{stem}");
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, "-- cpd-migration: additive\n");
        assert_eq!(declaration(&text), Ok(None));
    }
}

#[test]
fn new_migration_counts_the_names_before_it_for_a_breaking_one() {
    let (dir, _) = script_tree(&["0001-initial.sql", "0002-more.sql", "20200101T0000-old.sql", "99990101T0000-future.sql"]);
    let path = new_migration(dir.path(), &["rebuild", "breaking"]).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "-- cpd-migration: breaking after 3\n");
}

#[test]
fn new_migration_refuses_a_bad_slug_a_bad_kind_and_a_name_that_exists() {
    let (dir, migrations) = script_tree(&[]);
    for slug in ["", "Plan", "1plan", "plan_pins", "plan.pins", &"a".repeat(41)] {
        assert!(new_migration(dir.path(), &[slug]).is_err(), "slug {slug:?}");
    }
    assert!(new_migration(dir.path(), &["ok", "sideways"]).is_err());
    assert!(new_migration(dir.path(), &[]).is_err());
    assert_eq!(fs::read_dir(&migrations).unwrap().count(), 0, "a refusal writes nothing");
    let first = new_migration(dir.path(), &["same"]).unwrap();
    // The minute may roll over between the two runs; only a same-minute rerun collides.
    if let Err(why) = new_migration(dir.path(), &["same"]) {
        assert!(why.contains("already exists"), "{why}");
        assert!(first.exists());
    }
}

// ---------------------------------------------------------------------------
// O2: cpd names its columns
// ---------------------------------------------------------------------------

/// The Rust files under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every place in `text` where SQL reads every column (`SELECT *`, `SELECT t.*`,
/// `RETURNING *`) or writes a row without naming its columns (`INSERT [OR x]
/// INTO <table>` followed by anything but `(`). Returns `(line, why)`.
fn unnamed_columns(text: &str) -> Vec<(usize, &'static str)> {
    let lower = text.to_ascii_lowercase();
    let mut words: Vec<(usize, &str)> = Vec::new();
    let mut start = None;
    for (i, c) in lower.char_indices().chain(std::iter::once((lower.len(), ' '))) {
        let part = c.is_alphanumeric() || matches!(c, '_' | '{' | '}' | '.' | '*');
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                words.push((s, &lower[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    let line_of = |at: usize| lower[..at].matches('\n').count() + 1;
    let mut found = Vec::new();
    for (i, (at, word)) in words.iter().enumerate() {
        let next = words.get(i + 1);
        if matches!(*word, "select" | "returning") && next.is_some_and(|(_, n)| *n == "*" || n.ends_with(".*")) {
            found.push((line_of(*at), "reads every column with `*`"));
        }
        // `INSERT INTO`, `INSERT OR <action> INTO`.
        let inserts = *word == "into" && words[i.saturating_sub(3)..i].iter().any(|(_, w)| *w == "insert");
        if let (true, Some((table_at, table))) = (inserts, next)
            && !lower[table_at + table.len()..].trim_start().starts_with('(') {
                found.push((line_of(*at), "inserts without a column list"));
            }
    }
    found
}

#[test]
fn the_scanner_finds_what_it_is_for() {
    assert_eq!(unnamed_columns("let q = \"SELECT * FROM t\";").len(), 1);
    assert_eq!(unnamed_columns("select p.* from t p").len(), 1);
    assert_eq!(unnamed_columns("DELETE FROM t RETURNING *").len(), 1);
    assert_eq!(unnamed_columns("INSERT INTO t VALUES (1)").len(), 1);
    assert_eq!(unnamed_columns("INSERT OR IGNORE INTO t\n SELECT a FROM u").len(), 1);
    assert_eq!(unnamed_columns("INSERT INTO {table} SELECT 1").len(), 1);
    assert_eq!(unnamed_columns("INSERT INTO t (a, b) VALUES (1, 2)").len(), 0);
    assert_eq!(unnamed_columns("INSERT INTO t(a) VALUES (1)").len(), 0);
    assert_eq!(unnamed_columns("INSERT INTO t\n  (a) SELECT a FROM u").len(), 0);
    assert_eq!(unnamed_columns("SELECT count(*) FROM t; SELECT a, b FROM t").len(), 0);
    assert_eq!(unnamed_columns("INSERT INTO t (a) VALUES (1); SELECT a FROM t").len(), 0);
}

#[test]
fn no_code_selects_star_or_inserts_without_a_column_list() {
    let root = repo_root();
    let mut files = Vec::new();
    for krate in ["core", "cli", "tui"] {
        rust_files(&root.join(krate).join("src"), &mut files);
    }
    assert!(files.len() > 50, "found {} Rust files: the scan is looking in the wrong place", files.len());
    let mut offences = Vec::new();
    for file in &files {
        for (line, why) in unnamed_columns(&fs::read_to_string(file).unwrap()) {
            offences.push(format!("{}:{line}: {why}", file.strip_prefix(&root).unwrap().display()));
        }
    }
    assert!(
        offences.is_empty(),
        "cpd names its columns, so the order of columns that additive migrations add is harmless (§4.5 O2); name them:\n{}",
        offences.join("\n")
    );
}

// ---------------------------------------------------------------------------
// `policy-keys` (contract §4.5 "The policy keys"): the fill
// ---------------------------------------------------------------------------

/// A store before `policy-keys` with three Contexts: `one` (its PRs' pull requests are of `Acme`, mixed
/// case, so lowercased), `two` (pull requests of `acme` and `other`) and `none` (PRs without a pull request).
const BEFORE_POLICY_KEYS: &str = "
BEGIN;
INSERT INTO txns (id, at, command, pid, cwd) VALUES (1, 't', 'test', 1, '/');
INSERT INTO contexts (id, title, created_at, created_on) VALUES ('one', 'One', 't', 'm'), ('two', 'Two', 't', 'm'), ('none', 'None', 't', 'm');
INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
  ('l-p1', 'lead', 'one', 'p1', 'lead', '/', 'O3', 'idle', 't', 't'),
  ('l-p2', 'lead', 'two', 'p2', 'lead', '/', 'O3', 'idle', 't', 't'),
  ('l-p3', 'lead', 'none', 'p3', 'lead', '/', 'O3', 'idle', 't', 't');
INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
  ('p1', 'one', 'P1', 'grounding', 'l-p1', 't'), ('p2', 'two', 'P2', 'grounding', 'l-p2', 't'), ('p3', 'none', 'P3', 'grounding', 'l-p3', 't');
INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at) VALUES
  (1, 'Acme/Web', 1, 'u1', 'T', 0, 'open', 'none', 'none', 't', 't'),
  (2, 'acme/pay', 2, 'u2', 'T', 0, 'open', 'none', 'none', 't', 't'),
  (3, 'acme/x', 3, 'u3', 'T', 0, 'open', 'none', 'none', 't', 't'),
  (4, 'other/y', 4, 'u4', 'T', 0, 'open', 'none', 'none', 't', 't');
INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at) VALUES
  ('p1', 'web', 'web', 'main', 'open', 'cpd/p1/web', '/w', 1, 'u1', 't', 1, 1, 't', 't'),
  ('p1', 'pay', 'pay', 'main', 'open', 'cpd/p1/pay', '/w', 2, 'u2', 't', 2, 1, 't', 't'),
  ('p1', 'doc', 'doc', 'main', 'waiting', 'cpd/p1/doc', NULL, NULL, NULL, NULL, NULL, 1, 't', NULL),
  ('p2', 'x', 'x', 'main', 'open', 'cpd/p2/x', '/w', 3, 'u3', 't', 3, 1, 't', 't'),
  ('p2', 'y', 'y', 'main', 'open', 'cpd/p2/y', '/w', 4, 'u4', 't', 4, 1, 't', 't'),
  ('p3', 'z', 'z', 'main', 'waiting', 'cpd/p3/z', NULL, NULL, NULL, NULL, NULL, 1, 't', NULL);
COMMIT;";

#[test]
fn policy_keys_fills_the_owner_and_the_name_and_keeps_every_row() {
    let list = named_migrations();
    let index = list.iter().position(|(name, ..)| name.ends_with("-policy-keys")).expect("the migration exists");
    assert_eq!(list[index].1, "breaking");
    let dir = tempfile::tempdir().unwrap();
    let conn = prefix_database(index, &home_in(&dir)).unwrap();
    conn.execute_batch(BEFORE_POLICY_KEYS).unwrap();
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    let before = (count("SELECT count(*) FROM contexts"), count("SELECT count(*) FROM prs"), count("SELECT count(*) FROM pull_requests"));
    assert_eq!(before, (3, 6, 4));

    conn.execute_batch(list[index].2).unwrap();

    let after = (count("SELECT count(*) FROM contexts"), count("SELECT count(*) FROM prs"), count("SELECT count(*) FROM pull_requests"));
    assert_eq!(after, before, "every row kept");
    let strings = |sql: &str| -> Vec<(String, Option<String>)> {
        conn.prepare(sql).unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    let owner = |c: &str| (c.to_owned(), None::<String>);
    assert_eq!(
        strings("SELECT id, owner FROM contexts ORDER BY id"),
        [owner("none"), ("one".to_owned(), Some("acme".to_owned())), owner("two")],
        "one owner is filled lowercased; two owners and none stay NULL"
    );
    let name = |k: &str, n: Option<&str>| (k.to_owned(), n.map(str::to_owned));
    assert_eq!(
        strings("SELECT plan || '/' || key, name FROM prs ORDER BY plan, key"),
        [
            name("p1/doc", None),
            name("p1/pay", Some("pay")),
            name("p1/web", Some("web")),
            name("p2/x", Some("x")),
            name("p2/y", Some("y")),
            name("p3/z", None),
        ],
        "a PR with a pull request gets its lowercased name; one without keeps NULL"
    );
    assert_eq!(count("SELECT count(*) FROM event_kinds WHERE name = 'context-owner'"), 1);
    assert_eq!(count("SELECT count(*) FROM pragma_foreign_key_check"), 0);
}
