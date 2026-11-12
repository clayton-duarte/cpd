//! Store repair implementation (Plan `store-repair`).

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OptionalExtension};
use time::OffsetDateTime;
use time::macros::format_description;

use super::schema::{self, LEGACY, SCHEMA_MIGRATION_REPAIRS_DDL};
use super::verdict::verdict;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// Options for store repair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairOptions {
    /// When true, perform dry-run on a scratch copy without modifying the live database.
    pub dry_run: bool,
    /// Must be confirmed by user interaction / confirmation flag; aborts if false.
    pub confirmed: bool,
    /// Reason recorded in the repair ledger table.
    pub reason: String,
}

/// Result of a store repair attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairResult {
    /// Whether the store was repaired.
    pub repaired: bool,
    /// Path to the backup created before writing to the live store, if any.
    pub backup_path: Option<PathBuf>,
}

/// A repair entry from `schema_migration_repairs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRepairRow {
    pub id: i64,
    pub name: String,
    pub recorded_checksum: String,
    pub accepted_checksum: String,
    pub txn: i64,
    pub reason: String,
}

/// A detected checksum divergence between what the store recorded and what this binary embeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivergedMigration {
    pub name: String,
    pub recorded_checksum: String,
    pub accepted_checksum: String,
}

/// Detects diverged migrations on the store at `conn`.
pub fn detect_divergences(conn: &Connection) -> Result<Vec<DivergedMigration>, CpdError> {
    let has_schema_migrations: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_schema_migrations {
        return Ok(Vec::new());
    }

    let catalog = super::Catalog::BUILT_IN;
    let known = |name: &str| -> Option<String> {
        LEGACY
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.sql)
            .or_else(|| catalog.named.iter().find(|n| n.name == name).map(|n| n.sql))
            .map(schema::sha256_hex)
    };

    let mut stmt = conn
        .prepare("SELECT name, checksum FROM schema_migrations ORDER BY seq")
        .map_err(CpdError::from)?;
    let applied: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(CpdError::from)?
        .collect::<Result<_, _>>()
        .map_err(CpdError::from)?;

    let has_repairs: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migration_repairs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);

    let repairs: std::collections::HashSet<(String, String, String)> = if has_repairs {
        let mut stmt = conn
            .prepare(
                "SELECT name, recorded_checksum, accepted_checksum FROM schema_migration_repairs",
            )
            .map_err(CpdError::from)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(CpdError::from)?
            .filter_map(rusqlite::Result::ok)
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let mut diverged = Vec::new();
    for (name, checksum) in applied {
        if let Some(embedded) = known(&name)
            && checksum != embedded
            && !repairs.contains(&(name.clone(), checksum.clone(), embedded.clone()))
        {
            diverged.push(DivergedMigration {
                name,
                recorded_checksum: checksum,
                accepted_checksum: embedded,
            });
        }
    }

    Ok(diverged)
}

/// UTC timestamp formatted as `YYYYMMDDTHHMMSSZ`.
fn stamp(at: OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .expect("a UTC time always formats")
}

/// Copies the live store files (db, -wal, -shm) to `dest_dir`.
fn copy_store_files(db: &Path, dest_dir: &Path) -> Result<(), CpdError> {
    fs::create_dir_all(dest_dir).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot create directory {}: {e}", dest_dir.display()),
        )
    })?;

    for suffix in ["", "-wal", "-shm"] {
        let mut from = db.to_path_buf().into_os_string();
        from.push(suffix);
        let from = PathBuf::from(from);
        if from.is_file() {
            let to = dest_dir.join(from.file_name().unwrap_or_default());
            fs::copy(&from, &to).map_err(|e| {
                CpdError::new(
                    ErrorCode::Internal,
                    format!("Cannot copy {} to {}: {e}", from.display(), to.display()),
                )
            })?;
        }
    }
    Ok(())
}

/// Backs up the store to `$CPD_HOME/run/backups/<ts>/cpd.db`.
pub fn backup_store(home: &CpdHome, ts: &str) -> Result<PathBuf, CpdError> {
    let backup_dir = home.root().join("run/backups").join(ts);
    copy_store_files(&home.db(), &backup_dir)?;
    Ok(backup_dir.join("cpd.db"))
}

/// Applies repair entries in a transaction on `conn`.
pub fn apply_repair_in_txn(
    conn: &mut Connection,
    divergences: &[DivergedMigration],
    reason: &str,
) -> Result<(), CpdError> {
    let reason = if reason.trim().is_empty() {
        "store repair"
    } else {
        reason.trim()
    };

    let txn: i64 = conn
        .query_row("SELECT coalesce(max(id), 0) + 1 FROM txns", [], |r| {
            r.get(0)
        })
        .map_err(CpdError::from)?;

    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn))
        .map_err(CpdError::from)?;

    let tx = conn.transaction().map_err(CpdError::from)?;
    tx.execute_batch(SCHEMA_MIGRATION_REPAIRS_DDL)
        .map_err(CpdError::from)?;

    tx.execute(
        "INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (?1, ?2, 'cpd host repair', ?3, ?4, ?5)",
        rusqlite::params![
            txn,
            crate::clock::now(),
            std::process::id() as i64,
            std::env::var("CPD_SESSION").ok(),
            std::env::current_dir().ok().map(|p| p.display().to_string())
        ],
    ).map_err(CpdError::from)?;

    for div in divergences {
        tx.execute(
            "INSERT INTO schema_migration_repairs (name, recorded_checksum, accepted_checksum, txn, reason)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                div.name,
                div.recorded_checksum,
                div.accepted_checksum,
                txn,
                reason
            ],
        ).map_err(CpdError::from)?;
    }

    tx.commit().map_err(CpdError::from)?;
    Ok(())
}

/// Runs a dry-run repair on a scratch copy of the store at `home`.
pub fn dry_run_on_copy(
    home: &CpdHome,
    divergences: &[DivergedMigration],
    reason: &str,
) -> Result<(), CpdError> {
    let scratch_dir = home
        .root()
        .join(format!("run/scratch-repair-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch_dir);
    copy_store_files(&home.db(), &scratch_dir)?;

    let scratch_db = scratch_dir.join(home.db().file_name().unwrap_or_default());
    let res = (|| -> Result<(), CpdError> {
        let mut conn = Connection::open(&scratch_db).map_err(CpdError::from)?;
        apply_repair_in_txn(&mut conn, divergences, reason)?;
        let v = verdict(&conn)?;
        if !v.opens() {
            return Err(CpdError::new(
                ErrorCode::StoreFailed,
                format!("Dry run verdict failed: {:?}", v.outcome),
            ));
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&scratch_dir);
    res
}

/// Repairs a diverged store at `home`.
pub fn repair(home: &CpdHome, options: &RepairOptions) -> Result<RepairResult, CpdError> {
    if !home.db().exists() {
        return Ok(RepairResult {
            repaired: false,
            backup_path: None,
        });
    }

    let conn = Connection::open(home.db()).map_err(CpdError::from)?;
    let divergences = detect_divergences(&conn)?;
    drop(conn);

    if divergences.is_empty() {
        return Ok(RepairResult {
            repaired: false,
            backup_path: None,
        });
    }

    // Step 1: Copy dry-run
    dry_run_on_copy(home, &divergences, &options.reason)?;

    if options.dry_run {
        return Ok(RepairResult {
            repaired: true,
            backup_path: None,
        });
    }

    // Interactive confirmation check (A3)
    if !options.confirmed {
        return Err(CpdError::new(
            ErrorCode::HostApprovalRequired,
            "Store repair requires user confirmation [y/N].",
        ));
    }

    // Step 2: Backup store under run/backups/<ts>/
    let ts = stamp(OffsetDateTime::now_utc());
    let backup_path = backup_store(home, &ts)?;

    // Step 3: Apply repair on live database inside transaction
    let mut conn = Connection::open(home.db()).map_err(CpdError::from)?;
    apply_repair_in_txn(&mut conn, &divergences, &options.reason)?;
    drop(conn);

    Ok(RepairResult {
        repaired: true,
        backup_path: Some(backup_path),
    })
}

/// Detects table schema drift on the store at `conn`.
pub fn detect_table_drift(conn: &Connection) -> Result<Vec<String>, CpdError> {
    let mut drifted = Vec::new();
    let jobs_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'jobs'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(CpdError::from)?;
    if let Some(sql) = jobs_sql {
        if sql.contains("waiting_on IN ('lead', 'user')")
            || sql.contains("CHECK (waiting_on IS NULL OR waiting_on IN")
        {
            drifted.push("jobs".to_string());
        }
    }

    let incident_cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('incident') ORDER BY cid")
        .map_err(CpdError::from)?
        .query_map([], |r| r.get(0))
        .map_err(CpdError::from)?
        .collect::<Result<_, _>>()
        .map_err(CpdError::from)?;

    if incident_cols.len() == 8 && incident_cols.get(5).map(|s| s.as_str()) == Some("until_plan") {
        drifted.push("incident".to_string());
    }

    Ok(drifted)
}

/// Stub indicator for table schema reconciliation support in store repair.
/// Returns false until implemented.
pub fn is_table_reconciliation_supported() -> bool {
    false
}
