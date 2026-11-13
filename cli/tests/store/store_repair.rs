//! Store repair tests (bug b23, Plan `store-repair`).
//!
//! Tests mac-pro's exact diverged migration case, copy dry-run,
//! backup creation, ledger append, verdict acceptance, and clean install compatibility.

use std::fs;

use crate::home::{HasCpdHome, Home};
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::store::repair::{RepairOptions, is_table_reconciliation_supported, repair};
use cpd_core::store::{SCHEMA_HASH, Store, canonical_hash, verdict};
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

pub(crate) const DIVERGED_MIGRATION: &str = "20261110T2200-draft-waiting-on";
/// Mac-pro applied draft checksum (from bug b23 / pro-n444 / pro-d562 / pro-d568)
pub(crate) const MAC_PRO_RECORDED_CHECKSUM: &str =
    "d0e0badbbd9c0000000000000000000000000000000000000000000000000000";
/// Expected checksum on origin/main (with column jobs.waiting_on added)
pub(crate) const ORIGIN_MAIN_CHECKSUM: &str =
    "ce607fabfb0036c67828eb91f400d3bdf848b7da5ce221c1704a1367588bd2af";

fn home(env: &impl HasCpdHome) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// Sets up a database that applied all standard migrations plus the draft migration
/// with mac-pro's diverged checksum on a raw SQLite connection.
pub(crate) fn setup_diverged_store_conn(conn: &Connection) {
    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn))
        .unwrap();

    // Check if jobs already has waiting_on; if not, add it
    let has_waiting_on: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('jobs') WHERE name = 'waiting_on'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if !has_waiting_on {
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN waiting_on TEXT;")
            .unwrap();
    }

    // Now update or insert the row in schema_migrations with mac-pro's checksum
    // Note: schema_migrations has append-only triggers, so we drop them or insert
    let row_exists: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM schema_migrations WHERE name = ?1",
            [DIVERGED_MIGRATION],
            |r| r.get(0),
        )
        .unwrap();

    if row_exists {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS schema_migrations_no_update;
             UPDATE schema_migrations SET checksum = 'd0e0badbbd9c0000000000000000000000000000000000000000000000000000'
             WHERE name = '20261110T2200-draft-waiting-on';
             CREATE TRIGGER schema_migrations_no_update BEFORE UPDATE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;",
        ).unwrap();
    } else {
        conn.execute(
            "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
             VALUES (?1, 'additive', ?2, (SELECT max(seq) + 1 FROM schema_migrations), ?3, 0)",
            rusqlite::params![DIVERGED_MIGRATION, MAC_PRO_RECORDED_CHECKSUM, txn],
        )
        .unwrap();
    }
}

/// Sets up a database that applied all standard migrations plus the draft migration
/// with mac-pro's diverged checksum.
pub(crate) fn setup_mac_pro_diverged_store(env: &impl HasCpdHome) {
    let h = home(env);
    // Initialize standard database
    drop(Store::open(&h, 5000).unwrap());

    // Connect raw and simulate mac-pro's diverged draft migration application
    let conn = Connection::open(h.db()).unwrap();
    setup_diverged_store_conn(&conn);
}

/// Acceptance:
/// - Tests reproduce mac-pro's exact checksum divergence and verify that Store::open fails
///   with MigrationChanged before repair.
/// - Tests verify that repair creates a backup file before writing, appends to
///   schema_migration_repairs, and Store::open succeeds after repair.
#[test]
fn repair_diverged_draft_migration_mac_pro() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_diverged_store(&env);

    // Verify Store::open fails with MigrationChanged before repair
    let err = Store::open(&h, 5000)
        .err()
        .expect("store with diverged checksum must fail to open");
    assert_eq!(err.code(), ErrorCode::MigrationChanged, "error: {err:?}");
    assert_eq!(err.details()["name"], DIVERGED_MIGRATION);
    assert_eq!(
        err.details()["recorded"],
        MAC_PRO_RECORDED_CHECKSUM,
        "recorded checksum must match mac-pro"
    );
    assert_eq!(
        err.details()["embedded"],
        ORIGIN_MAIN_CHECKSUM,
        "embedded checksum must match origin/main"
    );

    // Verify read-only verdict also refuses before repair
    let conn = Connection::open(h.db()).unwrap();
    let v = verdict(&conn).unwrap();
    assert!(
        v.outcome.is_err(),
        "verdict must refuse before repair: {:?}",
        v.outcome
    );
    drop(conn);

    // Run repair with confirmation
    let options = RepairOptions {
        dry_run: false,
        confirmed: true,
        reason: "mac-pro b23 diverged draft repair".to_owned(),
    };
    let res = repair(&h, &options).expect("repair should succeed when implemented");
    assert!(res.repaired, "result must indicate repair was performed");

    // Verify backup file was created before writing to the live store
    let backup_path = res.backup_path.expect("repair must create a backup file");
    assert!(
        backup_path.exists(),
        "backup file must exist at {}",
        backup_path.display()
    );
    assert!(
        backup_path.to_string_lossy().contains("run/backups/"),
        "backup must be stored under run/backups/<ts>/: {}",
        backup_path.display()
    );

    // Verify schema_migration_repairs table was appended to
    let conn = Connection::open(h.db()).unwrap();
    let repair_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM schema_migration_repairs WHERE name = ?1 AND recorded_checksum = ?2 AND accepted_checksum = ?3",
            [DIVERGED_MIGRATION, MAC_PRO_RECORDED_CHECKSUM, ORIGIN_MAIN_CHECKSUM],
            |r| r.get(0),
        )
        .expect("schema_migration_repairs must exist and be readable");
    assert_eq!(
        repair_count, 1,
        "exactly one repair row must be appended for diverged migration"
    );

    // Verify Store::open succeeds after repair
    drop(conn);
    let mut store = Store::open(&h, 5000).unwrap();
    let count: i64 = store
        .read(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM schema_migration_repairs", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert!(count >= 1);
}

/// Acceptance:
/// - Tests verify that dry-run operates on a copy and does not alter the original store file.
#[test]
fn repair_dry_run_does_not_modify_live_db() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_diverged_store(&env);

    let original_bytes = fs::read(h.db()).unwrap();

    let options = RepairOptions {
        dry_run: true,
        confirmed: true,
        reason: "dry run test".to_owned(),
    };
    let res = repair(&h, &options).expect("dry run repair must succeed");
    assert!(res.repaired, "dry-run on copy should report repair success");

    // The original database file must remain byte-identical
    let after_bytes = fs::read(h.db()).unwrap();
    assert_eq!(
        original_bytes, after_bytes,
        "dry-run must not modify the live database file"
    );

    // Store::open must still fail because the live DB was not touched
    let err = Store::open(&h, 5000)
        .err()
        .expect("live store must still fail after dry-run");
    assert_eq!(err.code(), ErrorCode::MigrationChanged);
}

/// Acceptance:
/// - Tests verify that repair aborts without user confirmation.
#[test]
fn repair_aborts_without_user_confirmation() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_diverged_store(&env);

    let original_bytes = fs::read(h.db()).unwrap();

    let options = RepairOptions {
        dry_run: false,
        confirmed: false, // User did not confirm
        reason: "unconfirmed repair".to_owned(),
    };
    let err = repair(&h, &options).expect_err("repair must abort without user confirmation");
    // Verify an appropriate refusal or error is returned
    assert!(
        err.code() == ErrorCode::HostApprovalRequired
            || err.code() == ErrorCode::Usage
            || err.to_string().contains("confirm"),
        "error must indicate missing confirmation: {err:?}"
    );

    // Live store must remain untouched
    let after_bytes = fs::read(h.db()).unwrap();
    assert_eq!(
        original_bytes, after_bytes,
        "unconfirmed repair must not touch the database"
    );

    // No backup should be created
    let backups_dir = env.cpd_home().join("run/backups");
    if backups_dir.exists() {
        let entries = fs::read_dir(&backups_dir).unwrap().count();
        assert_eq!(entries, 0, "no backup directory should be created on abort");
    }
}

/// Acceptance:
/// - Tests verify that schema_migrations is never modified (append-only ledger preserved)
///   and schema_migration_repairs itself is append-only.
#[test]
fn repair_preserves_append_only_ledger() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_diverged_store(&env);

    let options = RepairOptions {
        dry_run: false,
        confirmed: true,
        reason: "preservation test".to_owned(),
    };
    repair(&h, &options).expect("repair must succeed");

    let conn = Connection::open(h.db()).unwrap();

    // Verify schema_migrations row for the diverged migration still holds the original recorded checksum
    let recorded: String = conn
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE name = ?1",
            [DIVERGED_MIGRATION],
            |r| r.get(0),
        )
        .expect("schema_migrations row must still exist");
    assert_eq!(
        recorded, MAC_PRO_RECORDED_CHECKSUM,
        "schema_migrations must NOT be modified by repair"
    );

    // Verify schema_migrations update trigger still active and rejects updates
    let update_err = conn.execute(
        "UPDATE schema_migrations SET checksum = '0000000000000000000000000000000000000000000000000000000000000000' WHERE name = ?1",
        [DIVERGED_MIGRATION],
    ).unwrap_err();
    assert!(
        update_err
            .to_string()
            .contains("append_only: schema_migrations"),
        "schema_migrations must remain append-only: {update_err}"
    );

    // Verify schema_migration_repairs has append-only triggers (rejects UPDATE and DELETE)
    let repair_update_err = conn
        .execute(
            "UPDATE schema_migration_repairs SET reason = 'tampered'",
            [],
        )
        .unwrap_err();
    assert!(
        repair_update_err
            .to_string()
            .contains("append_only: schema_migration_repairs"),
        "schema_migration_repairs must be append-only: {repair_update_err}"
    );

    let repair_delete_err = conn
        .execute("DELETE FROM schema_migration_repairs", [])
        .unwrap_err();
    assert!(
        repair_delete_err
            .to_string()
            .contains("append_only: schema_migration_repairs"),
        "schema_migration_repairs must reject deletes: {repair_delete_err}"
    );
}

/// Acceptance:
/// - Clean install compatibility: clean installation on empty store applies all migrations
///   without needing repair, and `repair` on a clean store is a no-op that reports not repaired
///   and creates no backup files.
#[test]
fn clean_install_compatibility_and_no_op_repair() {
    let env = Home::empty();
    let h = home(&env);

    // Clean install: Store::open initializes all migrations cleanly
    let store = Store::open(&h, 5000).expect("clean Store::open must succeed");
    drop(store);

    // Verify verdict on clean store is opens
    let conn = Connection::open(h.db()).unwrap();
    let v = verdict(&conn).unwrap();
    assert!(
        v.opens(),
        "clean install verdict must open: {:?}",
        v.outcome
    );
    drop(conn);

    // Run repair on clean store
    let options = RepairOptions {
        dry_run: false,
        confirmed: true,
        reason: "clean store check".to_owned(),
    };
    let res = repair(&h, &options).expect("repair on clean store must succeed as no-op");
    assert!(!res.repaired, "clean store must not require repair");
    assert!(
        res.backup_path.is_none(),
        "clean store repair must not create any backup"
    );

    // Backups directory should not exist or be empty
    let backups_dir = env.cpd_home().join("run/backups");
    if backups_dir.exists() {
        let count = fs::read_dir(&backups_dir).unwrap().count();
        assert_eq!(count, 0, "no backups should be created for clean store");
    }

    // Post-repair Store::open must still succeed cleanly
    let mut store2 = Store::open(&h, 5000).expect("Store::open must succeed after no-op repair");
    let count: i64 = store2
        .read(|c| Ok(c.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))?))
        .unwrap();
    assert!(count > 0, "schema_migrations must have entries");
}

/// Acceptance:
/// - Pre-repair backup file integrity and post-repair Store::open compatibility:
///   Verifies that the pre-repair backup file created at `run/backups/<ts>/cpd.db` is a consistent
///   valid SQLite database holding the pre-repair diverged state, and post-repair Store::open
///   and store read/write operations work without any errors.
#[test]
fn pre_repair_backup_integrity_and_post_repair_store_operations() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_diverged_store(&env);

    let pre_repair_schema = canonical_hash(&Connection::open(h.db()).unwrap()).unwrap();

    let options = RepairOptions {
        dry_run: false,
        confirmed: true,
        reason: "end-to-end lifecycle verification".to_owned(),
    };
    let res = repair(&h, &options).expect("repair must succeed");
    assert!(res.repaired);

    let backup_path = res.backup_path.expect("backup must be returned");
    assert!(backup_path.is_file(), "backup file must exist");

    // Pre-repair backup integrity:
    // SQLite may update header counters when creating a snapshot. Check the
    // schema, integrity and pre-repair records rather than raw file identity.
    let backup_conn = Connection::open(&backup_path).unwrap();
    assert_eq!(canonical_hash(&backup_conn).unwrap(), pre_repair_schema);
    assert_eq!(
        backup_conn
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    let backup_checksum: String = backup_conn
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE name = ?1",
            [DIVERGED_MIGRATION],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        backup_checksum, MAC_PRO_RECORDED_CHECKSUM,
        "backup file must retain the pre-repair diverged checksum"
    );
    // And backup has no schema_migration_repairs table yet
    let has_repairs: bool = backup_conn
        .query_row(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migration_repairs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !has_repairs,
        "backup database must not contain schema_migration_repairs"
    );
    drop(backup_conn);

    // Post-repair Store::open compatibility:
    // Can open Store, read, and write transactions normally
    let mut store = Store::open(&h, 5000).expect("Store::open must succeed post-repair");

    let repair_entry = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT name, recorded_checksum, accepted_checksum, reason FROM schema_migration_repairs WHERE name = ?1",
                [DIVERGED_MIGRATION],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(repair_entry.0, DIVERGED_MIGRATION);
    assert_eq!(repair_entry.1, MAC_PRO_RECORDED_CHECKSUM);
    assert_eq!(repair_entry.2, ORIGIN_MAIN_CHECKSUM);
    assert_eq!(repair_entry.3, "end-to-end lifecycle verification");

    // Perform a write transaction on post-repair store
    store
        .write("test post-repair write", |c| {
            c.execute(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx-test', 'Test Context', '2026-11-12T00:00:00Z', 'mac-pro')",
                [],
            )?;
            c.execute(
                "INSERT INTO plans (id, context, title, phase, created_at) VALUES ('plan-test', 'ctx-test', 'Test Plan', 'shaping', '2026-11-12T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .expect("write to post-repair store must succeed");

    let plan_title: String = store
        .read(|c| {
            Ok(
                c.query_row("SELECT title FROM plans WHERE id = 'plan-test'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(plan_title, "Test Plan");
}

/// Sets up mac-pro's drifted table schemas:
/// 1. `jobs` table has an unmerged CHECK constraint on `waiting_on` from draft migration 20261110T2200-draft-waiting-on:
///    `CHECK (waiting_on IS NULL OR waiting_on IN ('lead', 'user') OR waiting_on LIKE 'explore:%')`
/// 2. `incident` table has columns `(id, started_at, started_by, reason, started_txn, until_plan, until_workflow, lead)`
///    instead of main's `(id, started_at, started_by, reason, started_txn, lead, until_plan, until_workflow)`
///    and triggers emitting json_object matching that order.
/// 3. Populates rows in `jobs` and `incident`.
pub(crate) fn setup_mac_pro_drifted_store(env: &impl HasCpdHome) {
    let h = home(env);
    // Initialize standard database
    let mut store = Store::open(&h, 5000).unwrap();

    // Populate rows in standard store first
    store
        .write("populate baseline data", |conn| {
            conn.execute(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx-1', 'Context 1', '2026-11-12T00:00:00Z', 'mac-pro')",
                [],
            )?;
            conn.execute(
                "INSERT INTO plans (id, context, title, phase, created_at) VALUES ('plan-1', 'ctx-1', 'Plan 1', 'shaping', '2026-11-12T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES ('plan-1', 'wf-1', 'cpd', 'main', 'waiting', 'cpd/plan-1/wf-1', cpd_txn(), '2026-11-12T00:00:00Z')",
                [],
            )?;
            // Populate a draft job and a queued job
            conn.execute(
                "INSERT INTO jobs (seq, plan, workflow, key, title, status, appended, created_txn, created_at, kind, waiting_on) \
                 VALUES (1, 'plan-1', 'wf-1', 'draft-job', 'Draft Job', 'draft', 0, cpd_txn(), '2026-11-12T00:00:00Z', 'work', 'lead')",
                [],
            )?;
            conn.execute(
                "INSERT INTO jobs (seq, plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at, kind) \
                 VALUES (2, 'plan-1', 'wf-1', 'queued-job', 'Queued Job', 'S1', 'goal text', 'ctx text', '[\"cli/\"]', '[\"acc\"]', '[]', 'queued', 0, '1111111111111111111111111111111111111111111111111111111111111111', cpd_txn(), '2026-11-12T00:00:00Z', 'work')",
                [],
            )?;
            // Populate an incident row
            conn.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn, lead, until_plan, until_workflow) \
                 VALUES (1, '2026-11-12T00:00:00Z', 'session-mgr', 'Drift testing incident', cpd_txn(), NULL, 'plan-1', 'wf-1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    drop(store);

    // Now introduce schema drift on a raw SQLite connection with PRAGMA foreign_keys = OFF and legacy_alter_table = ON
    let conn = Connection::open(h.db()).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF; PRAGMA legacy_alter_table = ON;")
        .unwrap();

    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn))
        .unwrap();

    // 1. Rebuild `jobs` table with the unmerged CHECK constraint on `waiting_on`
    conn.execute_batch(
        "DROP VIEW IF EXISTS job_tenants;
         CREATE TABLE jobs_drifted (
           seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
           plan                TEXT NOT NULL REFERENCES plans(id),
           workflow            TEXT,
           review              INTEGER NOT NULL DEFAULT 0 CHECK (review IN (0, 1)),
           key                 TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 32),
           title               TEXT NOT NULL CHECK (length(title) >= 1),
           tier                TEXT CHECK (tier IS NULL OR (length(tier) = 2 AND substr(tier, 1, 1) IN ('O', 'S', 'H', 'G', 'J') AND (substr(tier, 2, 1) BETWEEN '0' AND '4' OR substr(tier, 2, 1) = 'X'))),
           goal                TEXT CHECK (goal IS NULL OR length(goal) >= 1),
           context             TEXT,
           touches             TEXT CHECK (touches IS NULL OR (json_valid(touches) AND json_type(touches) = 'array' AND json_array_length(touches) >= 1)),
           acceptance          TEXT CHECK (acceptance IS NULL OR (json_valid(acceptance) AND json_type(acceptance) = 'array' AND json_array_length(acceptance) >= 1)),
           out_of_scope        TEXT CHECK (out_of_scope IS NULL OR (json_valid(out_of_scope) AND json_type(out_of_scope) = 'array')),
           status              TEXT NOT NULL CHECK (status IN ('draft', 'queued', 'running', 'merged', 'failed', 'cancelled')),
           appended            INTEGER NOT NULL CHECK (appended IN (0, 1)),
           fingerprint         TEXT CHECK (fingerprint IS NULL OR length(fingerprint) = 64),
           created_txn         INTEGER NOT NULL REFERENCES txns(id),
           retry_tier          TEXT CHECK (retry_tier IS NULL OR (length(retry_tier) = 2 AND substr(retry_tier, 1, 1) IN ('O', 'S', 'H', 'G', 'J') AND (substr(retry_tier, 2, 1) BETWEEN '0' AND '4' OR substr(retry_tier, 2, 1) = 'X'))),
           cancel_reason       TEXT,
           cancel_requested_at TEXT,
           failed_cause        TEXT,
           merged_sha          TEXT,
           created_at          TEXT NOT NULL,
           urgent              INTEGER NOT NULL DEFAULT 0 CHECK (urgent IN (0, 1)),
           kind                TEXT NOT NULL DEFAULT 'work' CHECK (kind IN ('work', 'docs', 'test', 'impl', 'verify', 'open', 'update', 'scout', 'research')),
           tests               TEXT CHECK (tests IS NULL OR (json_valid(tests) AND json_type(tests) = 'array' AND json_array_length(tests) >= 1)),
           test_files          TEXT CHECK (test_files IS NULL OR (json_valid(test_files) AND json_type(test_files) = 'array' AND json_array_length(test_files) >= 1)),
           test_approved       TEXT CHECK (test_approved IS NULL OR (json_valid(test_approved) AND json_type(test_approved) = 'array')),
           resources           TEXT CHECK (resources IS NULL OR (json_valid(resources) AND json_type(resources) = 'array' AND json_array_length(resources) >= 1)),
           skipped_n           INTEGER CHECK (skipped_n IS NULL OR skipped_n >= 1),
           summary             TEXT CHECK (summary IS NULL OR length(summary) >= 1),
           touches_approved    TEXT CHECK (touches_approved IS NULL OR (json_valid(touches_approved) AND json_type(touches_approved) = 'array')),
           waiting_on          TEXT CHECK (waiting_on IS NULL OR waiting_on IN ('lead', 'user') OR waiting_on LIKE 'explore:%'),
           touches_waived      TEXT CHECK (touches_waived IS NULL OR (json_valid(touches_waived) AND json_type(touches_waived) = 'array')),
           UNIQUE (plan, key),
           FOREIGN KEY (plan, workflow) REFERENCES \"workflows\"(plan, key),
           CHECK (status <> 'draft' OR fingerprint IS NULL),
           CHECK (status IN ('draft', 'cancelled') OR fingerprint IS NOT NULL),
           CHECK (status IN ('draft', 'cancelled') OR (tier IS NOT NULL AND goal IS NOT NULL AND context IS NOT NULL AND touches IS NOT NULL AND acceptance IS NOT NULL AND out_of_scope IS NOT NULL)),
           CHECK ((goal IS NULL) = (context IS NULL) AND (goal IS NULL) = (touches IS NULL) AND (goal IS NULL) = (acceptance IS NULL) AND (goal IS NULL) = (out_of_scope IS NULL)),
           CHECK ((status = 'merged') = (merged_sha IS NOT NULL)),
           CHECK ((status = 'failed') = (failed_cause IS NOT NULL)),
           CHECK ((cancel_reason IS NULL) = (cancel_requested_at IS NULL)),
           CHECK (status <> 'cancelled' OR cancel_reason IS NOT NULL),
           CHECK ((workflow IS NULL) = (kind IN ('scout', 'research'))),
           CHECK ((kind IN ('scout', 'research') AND status = 'merged') = (summary IS NOT NULL))
         ) STRICT;
         INSERT INTO jobs_drifted SELECT * FROM jobs;
         DROP TABLE jobs;
         ALTER TABLE jobs_drifted RENAME TO jobs;
         CREATE INDEX jobs_plan_status ON jobs(plan, status);
         CREATE UNIQUE INDEX jobs_skipped_n ON jobs(skipped_n) WHERE skipped_n IS NOT NULL;
         CREATE VIEW job_tenants AS
           SELECT j.plan AS plan, j.key AS job, p.pool AS pool
             FROM jobs j JOIN job_pools p ON p.plan = j.plan AND p.job = j.key
            WHERE j.status NOT IN ('merged', 'cancelled')
               OR EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key AND a.status <> 'ended')
           UNION ALL
           SELECT j.plan, j.key, 'workers'
             FROM jobs j
            WHERE EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key);",
    )
    .unwrap();

    // Recreate triggers on `jobs`
    conn.execute_batch(
        "CREATE TRIGGER jobs_workflow_dependencies BEFORE UPDATE OF workflow ON jobs
         WHEN OLD.workflow <> NEW.workflow AND EXISTS (SELECT 1 FROM dependencies WHERE plan = NEW.plan AND (job = NEW.key OR needs = NEW.key))
         BEGIN SELECT RAISE(ABORT, 'needs_other_workflow'); END;

         CREATE TRIGGER jobs_status BEFORE UPDATE OF status ON jobs
         WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
           ('draft>queued', 'queued>running', 'running>merged',
            'running>queued', 'running>failed', 'failed>queued', 'queued>failed',
            'draft>cancelled', 'queued>cancelled', 'failed>cancelled', 'running>cancelled')
         BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

         CREATE TRIGGER jobs_kind_spec_ins BEFORE INSERT ON jobs
         WHEN (NEW.kind <> 'test' AND (NEW.tests IS NOT NULL OR NEW.test_files IS NOT NULL))
           OR ((NEW.tests IS NULL) <> (NEW.test_files IS NULL))
           OR (NEW.kind = 'test' AND NEW.tests IS NULL AND NEW.status NOT IN ('draft', 'cancelled'))
           OR (NEW.kind <> 'impl' AND NEW.test_approved IS NOT NULL)
           OR (NEW.review = 1 AND NEW.kind <> 'work')
         BEGIN SELECT RAISE(ABORT, 'spec_invalid'); END;

         CREATE TRIGGER jobs_kind_spec_upd BEFORE UPDATE OF kind, tests, test_files, test_approved, review, status ON jobs
         WHEN (NEW.kind <> 'test' AND (NEW.tests IS NOT NULL OR NEW.test_files IS NOT NULL))
           OR ((NEW.tests IS NULL) <> (NEW.test_files IS NULL))
           OR (NEW.kind = 'test' AND NEW.tests IS NULL AND NEW.status NOT IN ('draft', 'cancelled'))
           OR (NEW.kind <> 'impl' AND NEW.test_approved IS NOT NULL)
           OR (NEW.review = 1 AND NEW.kind <> 'work')
         BEGIN SELECT RAISE(ABORT, 'spec_invalid'); END;

         CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, workflow, review, kind, tests, test_files, resources ON jobs
         WHEN OLD.status <> 'draft'
          AND NOT (OLD.status = 'queued'
                   AND OLD.kind NOT IN ('open', 'update')
                   AND NOT EXISTS (SELECT 1 FROM attempts WHERE plan = OLD.plan AND job = OLD.key)
                   AND NEW.title IS OLD.title AND NEW.tier IS OLD.tier AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key
                   AND NEW.workflow IS OLD.workflow AND NEW.review IS OLD.review AND NEW.kind IS OLD.kind)
         BEGIN SELECT RAISE(ABORT, 'job_locked'); END;",
    )
    .unwrap();

    // 2. Rebuild `incident` table with mac-pro's column order:
    // (id, started_at, started_by, reason, started_txn, until_plan, until_workflow, lead)
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS incident_hist_ins;
         DROP TRIGGER IF EXISTS incident_hist_upd;
         CREATE TABLE incident_drifted (
           id             INTEGER PRIMARY KEY CHECK (id = 1),
           started_at     TEXT NOT NULL,
           started_by     TEXT NOT NULL,
           reason         TEXT NOT NULL CHECK (length(reason) >= 1),
           started_txn    INTEGER NOT NULL REFERENCES txns(id),
           until_plan     TEXT,
           until_workflow TEXT,
           lead           TEXT REFERENCES sessions(id)
         ) STRICT;
         INSERT INTO incident_drifted (id, started_at, started_by, reason, started_txn, until_plan, until_workflow, lead)
           SELECT id, started_at, started_by, reason, started_txn, until_plan, until_workflow, lead FROM incident;
         DROP TABLE incident;
         ALTER TABLE incident_drifted RENAME TO incident;",
    )
    .unwrap();

    // Recreate generated history triggers on incident matching mac-pro's column order
    conn.execute_batch(
        "CREATE TRIGGER incident_hist_ins AFTER INSERT ON incident BEGIN
           INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'incident',
             json_array(NEW.id), 'insert', NULL,
             json_object('id', NEW.id, 'started_at', NEW.started_at, 'started_by', NEW.started_by, 'reason', NEW.reason, 'started_txn', NEW.started_txn, 'until_plan', NEW.until_plan, 'until_workflow', NEW.until_workflow, 'lead', NEW.lead));
         END;

         CREATE TRIGGER incident_hist_upd AFTER UPDATE ON incident BEGIN
           INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'incident',
             json_array(NEW.id), 'update',
             json_object('id', OLD.id, 'started_at', OLD.started_at, 'started_by', OLD.started_by, 'reason', OLD.reason, 'started_txn', OLD.started_txn, 'until_plan', OLD.until_plan, 'until_workflow', OLD.until_workflow, 'lead', OLD.lead),
             json_object('id', NEW.id, 'started_at', NEW.started_at, 'started_by', NEW.started_by, 'reason', NEW.reason, 'started_txn', NEW.started_txn, 'until_plan', NEW.until_plan, 'until_workflow', NEW.until_workflow, 'lead', NEW.lead));
         END;",
    )
    .unwrap();

    // Set schema_migrations checksum to mac-pro's recorded checksum
    setup_diverged_store_conn(&conn);

    conn.execute_batch("PRAGMA legacy_alter_table = OFF; PRAGMA foreign_keys = ON;")
        .unwrap();
}

/// Acceptance:
/// - Reconciles drifted table schemas (jobs CHECK constraint, incident column order)
///   via cpd host repair --yes / store::repair into main's form.
/// - Validates schema hash and preserves data values.
#[test]
fn repair_reconciles_drifted_table_schemas() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_drifted_store(&env);

    // Verify that the drifted store deviates before repair:
    // 1. jobs has unmerged CHECK constraint on waiting_on
    let conn = Connection::open(h.db()).unwrap();
    let jobs_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'jobs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        jobs_sql.contains("waiting_on IN ('lead', 'user')"),
        "jobs table must carry drifted CHECK constraint before repair: {jobs_sql}"
    );

    // 2. incident has diverged column order: (..., until_plan, until_workflow, lead)
    let incident_cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('incident') ORDER BY cid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        incident_cols,
        vec![
            "id",
            "started_at",
            "started_by",
            "reason",
            "started_txn",
            "until_plan",
            "until_workflow",
            "lead"
        ],
        "incident column order must be drifted before repair"
    );

    // 3. canonical hash or generated mismatch reflects the drift
    let initial_hash = canonical_hash(&conn).unwrap();
    assert_ne!(
        initial_hash, SCHEMA_HASH,
        "canonical hash of drifted store must differ from main's SCHEMA_HASH"
    );
    drop(conn);

    // Run repair with confirmation
    let options = RepairOptions {
        dry_run: false,
        confirmed: true,
        reason: "reconcile drifted schemas".to_owned(),
    };
    let res = repair(&h, &options).expect("repair should succeed");
    assert!(res.repaired, "repair must report repaired: true");
    assert!(
        res.backup_path.is_some(),
        "repair must create a pre-repair backup"
    );

    // Post-repair verification:
    // Staged via stub check so that red test fails cleanly on stub until impl Job
    assert!(
        is_table_reconciliation_supported(),
        "store repair does not yet support table schema reconciliation (red stub)"
    );

    let conn = Connection::open(h.db()).unwrap();

    // Verify jobs table definition matches main (no CHECK constraint on waiting_on)
    let repaired_jobs_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'jobs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !repaired_jobs_sql.contains("waiting_on IN ('lead', 'user')"),
        "jobs table must not have unmerged CHECK on waiting_on after repair: {repaired_jobs_sql}"
    );

    // Verify incident column order matches main:
    // (id, started_at, started_by, reason, started_txn, lead, until_plan, until_workflow)
    let repaired_incident_cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('incident') ORDER BY cid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        repaired_incident_cols,
        vec![
            "id",
            "started_at",
            "started_by",
            "reason",
            "started_txn",
            "lead",
            "until_plan",
            "until_workflow"
        ],
        "incident column order must match main after repair"
    );

    // Verify canonical hash matches SCHEMA_HASH
    let post_repair_hash = canonical_hash(&conn).unwrap();
    assert_eq!(
        post_repair_hash, SCHEMA_HASH,
        "post-repair canonical hash must equal pinned SCHEMA_HASH"
    );

    // Verify Store::open opens cleanly without schema mismatch
    let store = Store::open(&h, 5000).expect("Store::open must succeed post-repair");
    assert_eq!(
        store.schema_mismatch(),
        None,
        "store must have no schema mismatch post-repair"
    );
}

/// Acceptance:
/// - Populates rows in drifted tables, runs repair, verifies data preservation,
///   and verifies subsequent cpd install copy migration succeeds.
#[test]
fn repair_preserves_rows_and_allows_install_copy_migration() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_drifted_store(&env);

    // Verify row count before repair
    let conn = Connection::open(h.db()).unwrap();
    let jobs_count_before: i64 = conn
        .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
        .unwrap();
    let incident_count_before: i64 = conn
        .query_row("SELECT count(*) FROM incident", [], |r| r.get(0))
        .unwrap();
    assert_eq!(jobs_count_before, 2, "jobs must have 2 rows before repair");
    assert_eq!(
        incident_count_before, 1,
        "incident must have 1 row before repair"
    );
    drop(conn);

    // Run repair with confirmation
    let options = RepairOptions {
        dry_run: false,
        confirmed: true,
        reason: "reconcile drifted schemas and verify data preservation".to_owned(),
    };
    let res = repair(&h, &options).expect("repair should succeed");
    assert!(res.repaired, "repair must report repaired: true");

    // Verify pre-repair backup file integrity on diverged schema store
    let backup_path = res
        .backup_path
        .as_ref()
        .expect("repair must create a pre-repair backup");
    assert!(
        backup_path.is_file(),
        "backup file must exist on filesystem"
    );
    assert!(
        backup_path.to_string_lossy().contains("run/backups/"),
        "backup must be stored under run/backups/<ts>/: {}",
        backup_path.display()
    );

    // Verify backup database content is a valid SQLite DB preserving the drifted state
    let backup_conn = Connection::open(backup_path).unwrap();
    let backup_jobs_sql: String = backup_conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'jobs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        backup_jobs_sql.contains("waiting_on IN ('lead', 'user')"),
        "pre-repair backup must preserve original diverged jobs schema: {backup_jobs_sql}"
    );
    let backup_jobs_count: i64 = backup_conn
        .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        backup_jobs_count, 2,
        "pre-repair backup must preserve all jobs rows"
    );
    let backup_inc_count: i64 = backup_conn
        .query_row("SELECT count(*) FROM incident", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        backup_inc_count, 1,
        "pre-repair backup must preserve incident rows"
    );
    drop(backup_conn);

    // Staged via stub check so that red test fails cleanly on stub until impl Job
    assert!(
        is_table_reconciliation_supported(),
        "store repair does not yet support table schema reconciliation (red stub)"
    );

    // Verify all existing rows and data values are preserved
    let mut store = Store::open(&h, 5000).expect("Store::open must succeed post-repair");
    let (job1_key, job1_waiting_on): (String, Option<String>) = store
        .read(|c| {
            Ok(
                c.query_row("SELECT key, waiting_on FROM jobs WHERE seq = 1", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?,
            )
        })
        .unwrap();
    assert_eq!(job1_key, "draft-job");
    assert_eq!(job1_waiting_on, Some("lead".to_owned()));

    let (job2_key, job2_status): (String, String) = store
        .read(|c| {
            Ok(
                c.query_row("SELECT key, status FROM jobs WHERE seq = 2", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?,
            )
        })
        .unwrap();
    assert_eq!(job2_key, "queued-job");
    assert_eq!(job2_status, "queued");

    let (inc_id, inc_reason, inc_until_plan): (i64, String, Option<String>) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id, reason, until_plan FROM incident WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(inc_id, 1);
    assert_eq!(inc_reason, "Drift testing incident");
    assert_eq!(inc_until_plan, Some("plan-1".to_owned()));

    // Verify subsequent cpd install copy migration simulation succeeds:
    // An install copy opens the database (which runs verdict and migration checks)
    let scratch_dir = tempfile::tempdir().unwrap();
    let scratch_db = scratch_dir.path().join("cpd.db");
    fs::copy(h.db(), &scratch_db).unwrap();
    let scratch_home =
        CpdHome::resolve(Some(scratch_dir.path().to_path_buf().into()), None).unwrap();
    let scratch_store = Store::open(&scratch_home, 5000)
        .expect("cpd install copy migration open must succeed on reconciled store");
    assert_eq!(scratch_store.schema_mismatch(), None);
}
