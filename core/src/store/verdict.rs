//! The compatibility window (contract §4.5 "The window and its one verdict"):
//! [`verdict`] decides open or refuse for every caller, from read-only state
//! (`user_version`, the `schema_migrations` rows and the binary's list of
//! migrations), and the schema check's in-window form (§4.3). Nothing else
//! compares schema numbers, checksums or migration names.

use std::collections::HashSet;
use std::sync::OnceLock;

use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;
use serde_json::Value;

use super::schema::{self, Compat, LEGACY, Named, RUNNER_DDL, VERSION};
use super::{Catalog, SchemaMismatch};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// Where an openable store starts (§4.5 O1, C1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// No schema yet: every legacy migration, then the named ones.
    Fresh,
    /// A legacy store at this `user_version`: the rest of the legacy set, the
    /// conversion (C2), then the named ones.
    Convert(i64),
    /// A named store: its unapplied named migrations.
    Named,
}

/// What a migrating transaction does: the verdict of a store that opens.
pub(super) struct Plan<'a> {
    pub(super) start: Start,
    /// The binary's named migrations the store has not applied, in name order.
    pub(super) pending: Vec<&'a Named>,
}

/// The one verdict (§4.5): whether this binary opens the store, and what it
/// would do. `unknown` and `pending` are filled for every named store, whether
/// or not it opens.
pub struct Verdict<'a> {
    /// The place the open starts at, or the refusal: `store_too_new`,
    /// `store_failed`, `migration_changed` or `migration_order`.
    pub outcome: Result<Start, CpdError>,
    /// The applied migration names this binary lacks, in name order. Inside
    /// the window they are all additive.
    pub unknown: Vec<String>,
    pending: Vec<&'a Named>,
}

impl std::fmt::Debug for Verdict<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Verdict")
            .field("outcome", &self.outcome)
            .field("unknown", &self.unknown)
            .field("pending", &self.pending_names())
            .finish()
    }
}

impl<'a> Verdict<'a> {
    /// Whether this binary can open the store, migrating or converting it if needed.
    pub fn opens(&self) -> bool {
        self.outcome.is_ok()
    }

    /// The refusal, when the store does not open.
    pub fn refusal(&self) -> Option<&CpdError> {
        self.outcome.as_ref().err()
    }

    /// The names of this binary's migrations the store has not applied, in the
    /// order they would be applied: for a store that is not named yet, the
    /// legacy names it lacks, then every named one.
    pub fn pending_names(&self) -> Vec<&'static str> {
        let legacy_from = match self.outcome {
            Ok(Start::Fresh) => 0,
            Ok(Start::Convert(version)) => version as usize,
            _ => LEGACY.len(),
        };
        LEGACY[legacy_from..].iter().map(|l| l.name).chain(self.pending.iter().map(|n| n.name)).collect()
    }

    /// What the migrating transaction does, or the refusal; `None` when the
    /// store needs nothing.
    pub(super) fn into_plan(self) -> Result<Option<Plan<'a>>, CpdError> {
        let start = self.outcome?;
        Ok((start != Start::Named || !self.pending.is_empty()).then_some(Plan { start, pending: self.pending }))
    }
}

/// The verdict of `conn` (opened read-only, or any connection: it only reads)
/// for this binary's migrations.
pub fn verdict(conn: &Connection) -> Result<Verdict<'static>, CpdError> {
    verdict_with(conn, &Catalog::BUILT_IN)
}

/// §4.5 W1–W3: the first row of the table that matches decides.
pub(super) fn verdict_with<'a>(conn: &Connection, catalog: &Catalog<'a>) -> Result<Verdict<'a>, CpdError> {
    let all_named = || catalog.named.iter().collect::<Vec<_>>();
    let version = super::user_version(conn)?;
    if version == 0 {
        return Ok(Verdict { outcome: Ok(Start::Fresh), unknown: Vec::new(), pending: all_named() });
    }
    if (1..=LEGACY.len() as i64).contains(&version) {
        return Ok(Verdict { outcome: Ok(Start::Convert(version)), unknown: Vec::new(), pending: all_named() });
    }
    if version != VERSION {
        return Ok(Verdict { outcome: Err(too_new(version, None, &[])), unknown: Vec::new(), pending: Vec::new() });
    }
    let table: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |r| r.get(0),
    )?;
    if table == 0 {
        let refusal = CpdError::new(
            ErrorCode::StoreFailed,
            format!("The database has user_version {VERSION} but no schema_migrations: it is not a store cpd wrote."),
        );
        return Ok(Verdict { outcome: Err(refusal), unknown: Vec::new(), pending: Vec::new() });
    }

    let mut stmt = conn.prepare("SELECT name, compat, checksum FROM schema_migrations ORDER BY seq")?;
    let applied: Vec<(String, String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    // The checksum of the text this binary embeds for `name`, when it knows the migration.
    let known = |name: &str| -> Option<String> {
        LEGACY
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.sql)
            .or_else(|| catalog.named.iter().find(|n| n.name == name).map(|n| n.sql))
            .map(schema::sha256_hex)
    };
    let mut unknown: Vec<String> = applied.iter().map(|(n, ..)| n.clone()).filter(|n| known(n).is_none()).collect();
    unknown.sort();
    let mut pending: Vec<&Named> =
        catalog.named.iter().filter(|n| !applied.iter().any(|(name, ..)| name == n.name)).collect();
    pending.sort_by_key(|n| n.name);

    let outcome = decide(version, &applied, &known, &unknown, &pending);
    Ok(Verdict { outcome, unknown, pending })
}

/// The rows of the table below "no schema_migrations": `applied` is
/// `(name, compat, checksum)` in `seq` order.
fn decide(
    version: i64,
    applied: &[(String, String, String)],
    known: &dyn Fn(&str) -> Option<String>,
    unknown: &[String],
    pending: &[&Named],
) -> Result<Start, CpdError> {
    // R3: an applied migration this binary knows, with another checksum.
    for (name, _, checksum) in applied {
        if let Some(embedded) = known(name)
            && *checksum != embedded {
                return Err(CpdError::new(
                    ErrorCode::MigrationChanged,
                    format!("The store applied migration {name} with other text than this cpd's: restore the migration's file from origin/main."),
                )
                .with("name", name.as_str())
                .with("recorded", checksum.as_str())
                .with("embedded", embedded));
            }
    }
    // R4: the floor is the breaking row with the highest `seq`. A binary that does not know it
    // lacks a breaking migration; one that does, lacks only additive ones (A1) and opens.
    let floor = applied.iter().rev().find(|(_, compat, _)| compat == Compat::Breaking.as_str()).map(|(n, ..)| n.as_str());
    if let Some(floor) = floor.filter(|f| known(f).is_none()) {
        return Err(too_new(version, Some(floor), unknown));
    }
    // O3: a breaking migration is a barrier.
    for named in pending.iter().filter(|n| n.compat == Compat::Breaking) {
        let after: Vec<&str> =
            applied.iter().map(|(n, ..)| n.as_str()).filter(|n| *n > named.name).collect();
        if !after.is_empty() || !unknown.is_empty() {
            return Err(order(named.name, &after, unknown));
        }
    }
    Ok(Start::Named)
}

/// `store_too_new` for a `user_version` this binary does not know the scheme
/// of: a numbered migration it lacks, or a later scheme. `events follow` calls
/// it on every poll; the rest of the verdict needs the migrations table.
pub fn refuse_user_version(version: i64) -> Result<(), CpdError> {
    if version != 0 && !(1..=LEGACY.len() as i64).contains(&version) && version != VERSION {
        return Err(too_new(version, None, &[]));
    }
    Ok(())
}

/// `store_too_new` (§4.5): the store has a breaking migration this cpd does not
/// know, or a later scheme. `floor` is the store's newest breaking migration; the
/// message names it when it is known, so an operator can see which one this binary lacks.
fn too_new(version: i64, floor: Option<&str>, unknown: &[String]) -> CpdError {
    let message = match floor {
        Some(floor) => format!(
            "The database was migrated by a newer cpd (schema {version}): it applied migration {floor}, which this cpd does not know. Upgrade cpd."
        ),
        None => format!("The database was migrated by a newer cpd (schema {version}): upgrade cpd."),
    };
    CpdError::new(ErrorCode::StoreTooNew, message)
    .with("user_version", version)
    .with("floor", floor)
    .with("unknown", unknown)
}

/// `migration_order` (§4.5 O3): `name` cannot be applied to this store; nothing is written.
fn order(name: &str, applied_after: &[&str], unknown: &[String]) -> CpdError {
    CpdError::new(
        ErrorCode::MigrationOrder,
        format!(
            "Migration {name} cannot be applied to this database: the database has applied migrations that sort after it \
             or that this cpd does not know, and a breaking migration was written without seeing them. Rename it to a \
             newer timestamp (git mv) and rebuild, or merge main into the branch."
        ),
    )
    .with("name", name)
    .with("applied_after", applied_after)
    .with("unknown", unknown)
}

// ---------- the schema check inside the window (§4.3) ----------

/// The schema check when the store has additive migrations this binary does
/// not know: **the canonical subset of the objects this binary's migrations
/// created** (§4.3). `None` when it passes. (a) every table of the reference
/// is in the store with the same constraints and options and with every
/// column of the reference as declared (a column the reference lacks belongs to
/// an unknown migration); (b) every other object of the reference is in the
/// store, equal; (c) no trigger on a table of the reference is neither
/// generated nor the reference's. Objects of unknown migrations are not checked.
pub(super) fn subset_mismatch(
    conn: &Connection,
    catalog: &Catalog<'_>,
    home: &CpdHome,
) -> Result<Option<SchemaMismatch>, CpdError> {
    let reference = reference_entries(catalog, home)?;
    let store = schema::canonical_entries(conn)?;
    let find = |kind: &Value, name: &Value| store.iter().find(|e| e[0] == *kind && e[1] == *name);
    let mismatch = |name: &Value, expected: String, found: String| {
        Ok(Some(SchemaMismatch { expected, found, object: Some(name.as_str().unwrap_or_default().to_owned()) }))
    };
    for entry in &reference {
        let Some(found) = find(&entry[0], &entry[1]) else { return mismatch(&entry[1], entry.to_string(), "missing".to_owned()) };
        let same = if entry[0] == "table" {
            let columns = |e: &Value| e[2].as_array().cloned().unwrap_or_default();
            let store_columns = columns(found);
            found[3] == entry[3] && found[4] == entry[4] && columns(entry).iter().all(|c| store_columns.contains(c))
        } else {
            found == entry
        };
        if !same {
            return mismatch(&entry[1], entry.to_string(), found.to_string());
        }
    }
    let tables: HashSet<&str> = reference.iter().filter(|e| e[0] == "table").filter_map(|e| e[1].as_str()).collect();
    let in_reference: HashSet<(&str, &str)> =
        reference.iter().filter_map(|e| Some((e[0].as_str()?, e[1].as_str()?))).collect();
    let mut stmt = conn.prepare("SELECT name, tbl_name, sql FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")?;
    let triggers: Vec<(String, String, Option<String>)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    for (name, table, sql) in triggers {
        // A generated trigger is not an entry: item 5 checks it.
        let is_entry = find(&Value::from("trigger"), &Value::from(name.as_str())).is_some();
        if is_entry && tables.contains(table.as_str()) && !in_reference.contains(&("trigger", name.as_str())) {
            return Ok(Some(SchemaMismatch {
                expected: "no trigger".to_owned(),
                found: sql.unwrap_or_default(),
                object: Some(name),
            }));
        }
    }
    Ok(None)
}

/// The reference *R* of §4.3: the canonical entries of a fresh in-memory
/// database that applied this binary's migrations in name order. Built once
/// per process for the built-in list.
fn reference_entries(catalog: &Catalog<'_>, home: &CpdHome) -> Result<Vec<Value>, CpdError> {
    static BUILT_IN: OnceLock<Vec<Value>> = OnceLock::new();
    let built_in = catalog.named.len() == schema::NAMED.len() && std::ptr::eq(catalog.named.as_ptr(), schema::NAMED.as_ptr());
    if built_in
        && let Some(entries) = BUILT_IN.get() {
            return Ok(entries.clone());
        }
    let entries = schema::canonical_entries(&reference_database(catalog, home)?)?;
    if built_in {
        let _ = BUILT_IN.set(entries.clone());
    }
    Ok(entries)
}

/// A fresh in-memory database after the legacy set, the runner's table, every
/// named migration and the reconcile: what a fresh store has (§4.5 O1).
/// `cpd_txn()` is a stand-in: no row is written.
pub(super) fn reference_database(catalog: &Catalog<'_>, home: &CpdHome) -> Result<Connection, CpdError> {
    let failed = |name: &str, e: &dyn std::fmt::Display| {
        CpdError::new(ErrorCode::MigrationFailed, format!("The reference schema (migration {name}) could not be built: {e}."))
            .with("migration", name)
            .with("sqlite_message", e.to_string())
    };
    let conn = Connection::open_in_memory()?;
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64))?;
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    conn.pragma_update(None, "legacy_alter_table", "ON")?;
    for legacy in LEGACY {
        (legacy.apply)(&conn).map_err(|e| failed(legacy.name, &e))?;
    }
    conn.execute_batch(RUNNER_DDL).map_err(|e| failed("schema_migrations", &e))?;
    for named in catalog.named {
        conn.execute_batch(named.sql).map_err(|e| failed(named.name, &e))?;
        if let Some(run) = named.run {
            run(&conn, home).map_err(|e| failed(named.name, &e.message()))?;
        }
    }
    schema::reconcile(&conn).map_err(|e| failed("reconcile", &e))?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use rusqlite::OpenFlags;
    use serde_json::json;

    use super::*;
    use crate::store::Store;
    use crate::store::tests::{ADD_A, ADD_B, applied, open_named, tmp_home};

    fn catalog(named: &[Named]) -> Catalog<'_> {
        Catalog { named, schema_hash: "" }
    }

    /// The verdict of `home`'s database for a binary whose list is `named`, read-only.
    fn judge<'a>(home: &CpdHome, named: &'a [Named]) -> Verdict<'a> {
        let conn = Connection::open_with_flags(home.db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        verdict_with(&conn, &catalog(named)).unwrap()
    }

    fn at_version(version: i64) -> Verdict<'static> {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        verdict_with(&conn, &Catalog::BUILT_IN).unwrap()
    }

    fn refusal(verdict: &Verdict<'_>) -> (ErrorCode, serde_json::Map<String, Value>) {
        let err = verdict.refusal().unwrap_or_else(|| panic!("expected a refusal: {verdict:?}"));
        (err.code(), err.details().clone())
    }

    fn breaking(named: Named) -> Named {
        Named { compat: Compat::Breaking, sql: "-- cpd-migration: breaking after 0\nSELECT 1;\n", ..named }
    }

    /// A tool that is not cpd, like the `sqlite3` shell.
    fn raw(home: &CpdHome, sql: &str) {
        Connection::open(home.db()).unwrap().execute_batch(sql).unwrap();
    }

    #[test]
    fn the_user_version_rows_of_the_table() {
        let legacy: Vec<&str> = LEGACY.iter().map(|l| l.name).collect();
        let named = || schema::NAMED.iter().map(|n| n.name);
        let fresh = at_version(0);
        assert_eq!(fresh.outcome.as_ref().ok(), Some(&Start::Fresh));
        assert_eq!(fresh.pending_names(), legacy.iter().copied().chain(named()).collect::<Vec<_>>());
        let convert = at_version(3);
        assert_eq!(convert.outcome.as_ref().ok(), Some(&Start::Convert(3)));
        assert_eq!(convert.pending_names(), legacy[3..].iter().copied().chain(named()).collect::<Vec<_>>());
        assert!(convert.unknown.is_empty());
        for numbered in [LEGACY.len() as i64 + 1, 99, 999, 1001] {
            let (code, details) = refusal(&at_version(numbered));
            assert_eq!(code, ErrorCode::StoreTooNew, "{numbered}");
            assert_eq!(details["user_version"], numbered);
            assert_eq!(details["floor"], Value::Null);
            assert_eq!(details["unknown"], json!([]));
        }
        let (code, _) = refusal(&at_version(VERSION));
        assert_eq!(code, ErrorCode::StoreFailed, "user_version 1000 without schema_migrations");
    }

    #[test]
    fn refuse_user_version_is_the_same_rows() {
        for version in [0, 1, 6, 7, 11, 12, 13, 14, 1000] {
            refuse_user_version(version).unwrap();
        }
        for version in [15, 999, 1001] {
            assert_eq!(refuse_user_version(version).unwrap_err().code(), ErrorCode::StoreTooNew);
        }
    }

    #[test]
    fn a_binary_opens_a_store_with_additive_migrations_it_does_not_know() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A, ADD_B]).unwrap());
        let older = [ADD_A];
        let verdict = judge(&home, &older);
        assert!(verdict.opens(), "{verdict:?}");
        assert_eq!(verdict.unknown, [ADD_B.name]);
        assert!(verdict.pending_names().is_empty());
        let verdict = judge(&home, &[]);
        assert!(verdict.opens(), "{verdict:?}");
        assert_eq!(verdict.unknown, [ADD_A.name, ADD_B.name]);

        // It reads and writes: the unknown column is nullable.
        let mut store = open_named(&home, &older).unwrap();
        assert_eq!(store.schema_mismatch(), None);
        store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let title: String = store.read(|c| Ok(c.query_row("SELECT title FROM contexts WHERE id = 'ctx'", [], |r| r.get(0))?)).unwrap();
        assert_eq!(title, "Ctx");
        assert_eq!(applied(&mut store).len(), LEGACY.len() + 2, "nothing was applied: the store has the legacy set and a and b");
    }

    #[test]
    fn a_binary_with_a_migration_of_its_own_applies_it_beside_unknown_ones() {
        // A Lead Branch build: `a` is pending and sorts before the applied `b`, which it lacks (O2).
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_B]).unwrap());
        let branch = [ADD_A];
        let verdict = judge(&home, &branch);
        assert!(verdict.opens(), "{verdict:?}");
        assert_eq!(verdict.unknown, [ADD_B.name]);
        assert_eq!(verdict.pending_names(), [ADD_A.name]);
        let mut store = open_named(&home, &branch).unwrap();
        assert_eq!(store.schema_mismatch(), None, "the generated triggers cover the unknown column, the subset check the known one");
        let names: Vec<String> = applied(&mut store).into_iter().skip(LEGACY.len()).map(|(n, ..)| n).collect();
        assert_eq!(names, [ADD_B.name, ADD_A.name]);
    }

    #[test]
    fn a_store_whose_floor_the_binary_lacks_is_store_too_new() {
        let (_dir, home) = tmp_home();
        let newer = breaking(ADD_B);
        drop(open_named(&home, &[ADD_A, newer]).unwrap());
        let older = [ADD_A];
        let (code, details) = refusal(&judge(&home, &older));
        assert_eq!(code, ErrorCode::StoreTooNew);
        assert_eq!(details["floor"], ADD_B.name);
        assert_eq!(details["unknown"], json!([ADD_B.name]));
        assert_eq!(details["user_version"], VERSION);
        // Every unknown name is listed, additive ones too; the answer comes with `unknown` filled.
        let verdict = judge(&home, &[]);
        assert_eq!(refusal(&verdict).1["unknown"], json!([ADD_A.name, ADD_B.name]));
        assert_eq!(verdict.unknown, [ADD_A.name, ADD_B.name]);
        // Store::open refuses with the verdict's code and writes nothing.
        let err = open_named(&home, &older).err().unwrap();
        assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
    }

    /// hs-d704: the `store_too_new` refusal names the floor migration, so an operator
    /// can see which migration this binary lacks.
    #[test]
    fn the_store_too_new_message_names_the_floor_migration() {
        let (_dir, home) = tmp_home();
        let newer = breaking(ADD_B);
        drop(open_named(&home, &[ADD_A, newer]).unwrap());
        let verdict = judge(&home, &[ADD_A]);
        let err = verdict.refusal().expect("a store whose floor the binary lacks is refused");
        assert_eq!(err.code(), ErrorCode::StoreTooNew);
        assert!(err.message().contains(ADD_B.name), "the message names the floor migration: {}", err.message());
        assert_eq!(err.details()["floor"], ADD_B.name);
    }

    #[test]
    fn the_floor_is_the_breaking_migration_with_the_highest_seq() {
        // `a` breaking, then `b` additive: a binary that knows `a` and lacks `b` opens.
        let (_dir, home) = tmp_home();
        let first = breaking(ADD_A);
        drop(open_named(&home, &[first, ADD_B]).unwrap());
        let older = [first];
        let verdict = judge(&home, &older);
        assert!(verdict.opens(), "{verdict:?}");
        assert_eq!(verdict.unknown, [ADD_B.name]);
        open_named(&home, &older).unwrap();
    }

    /// hs-d697: a live store applied the breaking migration under its old
    /// `20261025T2300-queue-failed` name, before the migrations that sort after it
    /// existed. Restoring that name (body unchanged) opens the store and does not
    /// re-apply the migration; before the restore this binary lacks the floor.
    #[test]
    fn a_store_whose_floor_is_the_restored_queue_failed_name_opens_and_is_not_reapplied() {
        let (_dir, home) = tmp_home();
        // Build a live store as what it was: applied only the migrations up to the restored name.
        // The prefix is ordered by name (contract §4.5 O1), so slice to the first migration after the floor.
        let upto = schema::NAMED.iter().position(|n| n.name > "20261025T2300-queue-failed").unwrap();
        drop(open_named(&home, &schema::NAMED[..upto]).unwrap());
        // Every later migration, breaking or not, is still pending whatever lands after it.
        let verdict = judge(&home, schema::NAMED);
        assert!(verdict.opens(), "{verdict:?}");
        assert!(verdict.unknown.is_empty(), "{verdict:?}");

        let mut store = open_named(&home, schema::NAMED).unwrap();
        let queue: Vec<String> =
            applied(&mut store).into_iter().map(|(name, ..)| name).filter(|name| name.contains("queue-failed")).collect();
        assert_eq!(queue, ["20261025T2300-queue-failed"], "the restored name stays applied; it is not applied again under a new name");
    }

    /// hs-d710 part 1: hs and mac-pro applied the queue-failed migration with
    /// `breaking after 47` (checksum 91fbcf72); origin/main moved its first line
    /// to 48 (checksum 2f41ed9d), so a binary built from main refuses those
    /// stores with `migration_changed`. Restoring the header opens them.
    #[test]
    fn a_store_that_applied_the_live_queue_failed_text_opens() {
        let (_dir, home) = tmp_home();
        let file = include_str!("migrations/20261025T2300-queue-failed.sql");
        let applied = format!("-- cpd-migration: breaking after 47\n{}", file.split_once('\n').unwrap().1);
        let mut named: Vec<Named> = schema::NAMED.to_vec();
        let entry = named.iter_mut().find(|n| n.name == "20261025T2300-queue-failed").expect("the queue-failed migration is in the list");
        *entry = Named { sql: Box::leak(applied.into_boxed_str()), ..*entry };
        drop(open_named(&home, &named).unwrap());
        let checksum: String = Connection::open(home.db())
            .unwrap()
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE name = '20261025T2300-queue-failed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(checksum, "91fbcf723d8e4961ca0d99fa1dec8fc6b27a4d7251e48f9755352c5a4b179a0d");
        let verdict = judge(&home, schema::NAMED);
        assert!(verdict.opens(), "{verdict:?}");
    }

    #[test]
    fn a_pending_breaking_migration_that_sorts_before_an_applied_one_is_migration_order() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_B]).unwrap());
        let before = breaking(ADD_A); // sorts before the applied b
        let list = [before, ADD_B];
        let (code, details) = refusal(&judge(&home, &list));
        assert_eq!(code, ErrorCode::MigrationOrder);
        assert_eq!(details["name"], ADD_A.name);
        assert_eq!(details["applied_after"], json!([ADD_B.name]));
        assert_eq!(details["unknown"], json!([]));
        assert_eq!(ErrorCode::MigrationOrder.exit().code(), 1);

        // Nothing is written: the same rows and no new `txns` row.
        let count = |home: &CpdHome| -> (i64, usize) {
            let mut store = open_named(home, &[ADD_B]).unwrap();
            let txns = store.read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap();
            (txns, applied(&mut store).len())
        };
        let (txns, migrations) = count(&home);
        let err = open_named(&home, &list).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationOrder, "{err:?}");
        assert_eq!(count(&home), (txns, migrations));
    }

    #[test]
    fn a_pending_breaking_migration_with_an_unknown_one_applied_is_migration_order() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A]).unwrap());
        let newer = [breaking(ADD_B)]; // sorts after a, which this binary lacks
        let (code, details) = refusal(&judge(&home, &newer));
        assert_eq!(code, ErrorCode::MigrationOrder);
        assert_eq!(details["name"], ADD_B.name);
        assert_eq!(details["applied_after"], json!([]));
        assert_eq!(details["unknown"], json!([ADD_A.name]));
        let err = open_named(&home, &newer).err().unwrap();
        assert_eq!(err.code(), ErrorCode::MigrationOrder, "{err:?}");
    }

    #[test]
    fn a_pending_breaking_migration_after_everything_applied_opens() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A]).unwrap());
        let list = [ADD_A, breaking(ADD_B)];
        let verdict = judge(&home, &list);
        assert!(verdict.opens(), "{verdict:?}");
        assert_eq!(verdict.pending_names(), [ADD_B.name]);
        open_named(&home, &list).unwrap();
    }

    #[test]
    fn a_pending_additive_migration_before_the_floor_opens_and_is_applied() {
        let (_dir, home) = tmp_home();
        let floor = breaking(ADD_B);
        drop(open_named(&home, &[floor]).unwrap());
        let list = [ADD_A, floor];
        let verdict = judge(&home, &list);
        assert!(verdict.opens(), "{verdict:?}");
        assert_eq!(verdict.pending_names(), [ADD_A.name]);
        let mut store = open_named(&home, &list).unwrap();
        let names: Vec<String> = applied(&mut store).into_iter().skip(LEGACY.len()).map(|(n, ..)| n).collect();
        assert_eq!(names, [floor.name, ADD_A.name], "the store applied the floor first, then the pending additive a by O2");
    }

    #[test]
    fn a_changed_applied_migration_is_migration_changed_before_anything_else() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A, breaking(ADD_B)]).unwrap());
        let edited = Named { sql: "-- cpd-migration: additive\nALTER TABLE plans ADD COLUMN note_a TEXT;\n", ..ADD_A };
        let (code, details) = refusal(&judge(&home, &[edited]));
        assert_eq!(code, ErrorCode::MigrationChanged, "the floor b is also unknown");
        assert_eq!(details["name"], ADD_A.name);
    }

    #[test]
    fn a_verdict_reads_and_never_writes() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A]).unwrap());
        let before = std::fs::read(home.db()).unwrap();
        let _ = judge(&home, &[ADD_B]);
        assert_eq!(std::fs::read(home.db()).unwrap(), before);
    }

    // ---------- the schema check inside the window (§4.3) ----------

    #[test]
    fn the_reference_of_the_built_in_list_has_the_pinned_hash() {
        let (_dir, home) = tmp_home();
        let conn = reference_database(&Catalog::BUILT_IN, &home).unwrap();
        assert_eq!(schema::canonical_hash(&conn).unwrap(), schema::SCHEMA_HASH);
        assert_eq!(reference_entries(&Catalog::BUILT_IN, &home).unwrap(), reference_entries(&Catalog::BUILT_IN, &home).unwrap());
    }

    /// Prints the hash a fresh store has, for `scripts/repin.sh` (§4.3), which writes it as `SCHEMA_HASH`.
    /// `cargo test -p cpd-core --lib -- --ignored --nocapture prints_the_schema_hash`
    #[test]
    #[ignore = "prints the value of SCHEMA_HASH for scripts/repin.sh"]
    fn prints_the_schema_hash() {
        let (_dir, home) = tmp_home();
        let conn = reference_database(&Catalog::BUILT_IN, &home).unwrap();
        println!("SCHEMA_HASH={}", schema::canonical_hash(&conn).unwrap());
    }

    fn mismatch_object(store: &Store) -> Option<&str> {
        store.schema_mismatch().and_then(|m| m.object.as_deref())
    }

    /// A store that applied `a` and `b`, tampered with by `sql`, opened by a binary that knows only `known`.
    fn tampered(sql: &str, known: &[Named]) -> (tempfile::TempDir, Store) {
        let (dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A, ADD_B]).unwrap());
        raw(&home, sql);
        let store = open_named(&home, known).unwrap();
        (dir, store)
    }

    #[test]
    fn inside_the_window_the_objects_this_binary_created_are_checked() {
        // This binary knows `b`: its table, index and column are in the reference; `a`'s columns are unknown.
        let known = [ADD_B];
        let (_d, store) = tampered("SELECT 1", &known);
        assert_eq!(store.schema_mismatch(), None);

        let (_d, store) = tampered("DROP INDEX b_things_label", &known);
        let found = store.schema_mismatch().unwrap();
        assert_eq!((mismatch_object(&store), found.found.as_str()), (Some("b_things_label"), "missing"));

        let (_d, store) = tampered(
            "DROP TABLE b_things; CREATE TABLE b_things (id INTEGER PRIMARY KEY, label TEXT) STRICT; CREATE INDEX b_things_label ON b_things (label);",
            &known,
        );
        assert_eq!(mismatch_object(&store), Some("b_things"), "a known column declared another way");

        let (_d, store) = tampered("DROP TABLE b_things", &known);
        // Dropping the table drops its index, which sorts first among the missing objects.
        assert_eq!(mismatch_object(&store), Some("b_things_label"));
    }

    #[test]
    fn inside_the_window_a_trigger_on_a_table_of_the_reference_is_a_mismatch() {
        let (_d, store) = tampered("CREATE TRIGGER sneaky AFTER UPDATE ON plans BEGIN SELECT 1; END", &[ADD_B]);
        assert_eq!(mismatch_object(&store), Some("sneaky"));
        assert_eq!(store.schema_mismatch().unwrap().expected, "no trigger");
        // ... and a trigger on a table of an unknown migration is not checked.
        let (_d, store) = tampered("CREATE TRIGGER fine AFTER INSERT ON b_things BEGIN SELECT 1; END", &[ADD_A]);
        assert_eq!(store.schema_mismatch(), None);
    }

    #[test]
    fn inside_the_window_the_objects_of_unknown_migrations_are_not_checked() {
        let known = [ADD_A];
        for sql in [
            "DROP INDEX b_things_label",
            "DROP TABLE b_things",
            "CREATE TABLE somebody_elses (id INTEGER PRIMARY KEY) STRICT",
            "ALTER TABLE events ADD COLUMN extra TEXT",
        ] {
            let (_d, store) = tampered(sql, &known);
            assert_eq!(store.schema_mismatch(), None, "{sql}");
        }
    }

    #[test]
    fn inside_the_window_the_generated_triggers_are_still_checked() {
        let (_d, store) = tampered("DROP TRIGGER plans_hist_upd", &[ADD_A]);
        assert_eq!(mismatch_object(&store), Some("plans_hist_upd"));
        let (_d, mut store) = tampered("DROP TRIGGER plans_hist_upd", &[ADD_A]);
        let err = store.write("test", |_| Ok(())).unwrap_err();
        assert_eq!(err.code(), ErrorCode::StoreTampered);
    }

    #[test]
    fn without_unknown_migrations_the_whole_schema_hash_is_checked() {
        let (_dir, home) = tmp_home();
        drop(open_named(&home, &[ADD_A]).unwrap());
        raw(&home, "CREATE TABLE somebody_elses (id INTEGER PRIMARY KEY) STRICT");
        let store = open_named(&home, &[ADD_A]).unwrap();
        let m = store.schema_mismatch().unwrap();
        assert_eq!(m.object, None, "the hash differs: {m:?}");
    }
}
