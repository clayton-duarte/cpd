//! Store repair tests (bug b23, Plan `store-repair`).
//!
//! Tests mac-pro's exact diverged migration case, copy dry-run,
//! backup creation, ledger append, verdict acceptance, and clean install compatibility.

use std::fs;

use crate::home::{HasCpdHome, Home};
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::store::repair::{RepairOptions, repair};
use cpd_core::store::{Store, verdict};
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

const DIVERGED_MIGRATION: &str = "20261110T2200-draft-waiting-on";
/// Mac-pro applied draft checksum (from bug b23 / pro-n444 / pro-d562 / pro-d568)
const MAC_PRO_RECORDED_CHECKSUM: &str =
    "d0e0badbbd9c0000000000000000000000000000000000000000000000000000";
/// Expected checksum on origin/main (with column jobs.waiting_on added)
const ORIGIN_MAIN_CHECKSUM: &str =
    "ce607fabfb0036c67828eb91f400d3bdf848b7da5ce221c1704a1367588bd2af";

fn home(env: &impl HasCpdHome) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// Sets up a database that applied all standard migrations plus the draft migration
/// with mac-pro's diverged checksum.
fn setup_mac_pro_diverged_store(env: &impl HasCpdHome) {
    let h = home(env);
    // Initialize standard database
    drop(Store::open(&h, 5000).unwrap());

    // Connect raw and simulate mac-pro's diverged draft migration application
    let conn = Connection::open(h.db()).unwrap();
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
///   Verifies that the pre-repair backup file created at `run/backups/<ts>/cpd.db` is an exact
///   valid SQLite database holding the pre-repair diverged state, and post-repair Store::open
///   and store read/write operations work without any errors.
#[test]
fn pre_repair_backup_integrity_and_post_repair_store_operations() {
    let env = Home::empty();
    let h = home(&env);
    setup_mac_pro_diverged_store(&env);

    let pre_repair_bytes = fs::read(h.db()).unwrap();

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
    // 1. Content matches pre-repair database
    let backup_bytes = fs::read(&backup_path).unwrap();
    assert_eq!(
        pre_repair_bytes, backup_bytes,
        "backup file content must be byte-for-byte identical to the pre-repair database"
    );

    // 2. Backup database can be opened directly as SQLite and still has the diverged checksum
    let backup_conn = Connection::open(&backup_path).unwrap();
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
