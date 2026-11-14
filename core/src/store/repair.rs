//! Store repair implementation (Plan `store-repair`).

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
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

/// Takes a consistent SQLite snapshot, including committed WAL data. The
/// destination is reserved exclusively so an earlier rollback artifact survives.
fn snapshot_store(db: &Path, dest_dir: &Path) -> Result<(), CpdError> {
    let source = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(CpdError::from)?;
    fs::create_dir_all(dest_dir).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot create directory {}: {e}", dest_dir.display()),
        )
    })?;

    let to = dest_dir.join(db.file_name().unwrap_or_default());
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&to).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot reserve snapshot {}: {e}", to.display()),
        )
    })?);
    if let Err(error) = source.backup(rusqlite::MAIN_DB, &to, None) {
        let _ = fs::remove_file(&to);
        return Err(CpdError::from(error));
    }
    Ok(())
}

/// Backs up the store to `$CPD_HOME/run/backups/<ts>/cpd.db`.
pub fn backup_store(home: &CpdHome, ts: &str) -> Result<PathBuf, CpdError> {
    let backup_dir = home.root().join("run/backups").join(ts);
    snapshot_store(&home.db(), &backup_dir)?;
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

    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(CpdError::from)?;
    conn.pragma_update(None, "legacy_alter_table", "ON")
        .map_err(CpdError::from)?;

    let result = (|| -> Result<(), CpdError> {
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(CpdError::from)?;
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

        // Reconcile drifted table schemas
        let drifted_tables = detect_table_drift(&tx)?;
        if drifted_tables.iter().any(|t| t == "jobs") {
            reconcile_jobs_table(&tx)?;
        }
        if drifted_tables.iter().any(|t| t == "incident") {
            reconcile_incident_table(&tx)?;
        }

        schema::reconcile(&tx).map_err(CpdError::from)?;

        // PRAGMA foreign_key_check
        {
            let mut stmt = tx
                .prepare("PRAGMA foreign_key_check")
                .map_err(CpdError::from)?;
            let violations: Vec<(String, Option<i64>, String)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(CpdError::from)?
                .collect::<Result<_, _>>()
                .map_err(CpdError::from)?;
            if !violations.is_empty() {
                return Err(CpdError::new(
                    ErrorCode::StoreFailed,
                    format!("foreign key check failed: {violations:?}"),
                ));
            }
        }

        // Verify generated triggers match
        if let Some((name, expected, found)) =
            schema::generated_mismatch(&tx).map_err(CpdError::from)?
        {
            return Err(CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "generated trigger mismatch for {name}: expected {expected}, found {found:?}"
                ),
            ));
        }

        // Verify canonical schema hash matches expected SCHEMA_HASH when no unknown/pending migrations
        let v = verdict(&tx)?;
        if v.unknown.is_empty() && v.pending_names().is_empty() {
            let hash = schema::canonical_hash(&tx).map_err(CpdError::from)?;
            if hash != schema::SCHEMA_HASH {
                return Err(CpdError::new(
                    ErrorCode::StoreFailed,
                    format!(
                        "reconciled schema hash {hash} does not match expected {}",
                        schema::SCHEMA_HASH
                    ),
                ));
            }
        }

        tx.commit().map_err(CpdError::from)?;
        Ok(())
    })();

    let _ = conn.pragma_update(None, "legacy_alter_table", "OFF");
    let _ = conn.pragma_update(None, "foreign_keys", "ON");

    result
}

fn reconcile_jobs_table(conn: &Connection) -> Result<(), CpdError> {
    let has_waivers: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('jobs') WHERE name = 'touches_waived')",
            [],
            |row| row.get(0),
        )
        .map_err(CpdError::from)?;
    let mut sql = "DROP VIEW IF EXISTS job_tenants;
CREATE TABLE jobs__reconcile_new (
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
  waiting_on          TEXT,
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
INSERT INTO jobs__reconcile_new (
  seq, plan, workflow, review, key, title, tier, goal, context, touches,
  acceptance, out_of_scope, status, appended, fingerprint, created_txn,
  retry_tier, cancel_reason, cancel_requested_at, failed_cause, merged_sha,
  created_at, urgent, kind, tests, test_files, resources, test_approved,
  touches_approved, skipped_n, summary, waiting_on, touches_waived
)
SELECT
  seq, plan, workflow, review, key, title, tier, goal, context, touches,
  acceptance, out_of_scope, status, appended, fingerprint, created_txn,
  retry_tier, cancel_reason, cancel_requested_at, failed_cause, merged_sha,
  created_at, urgent, kind, tests, test_files, resources, test_approved,
  touches_approved, skipped_n, summary, waiting_on, touches_waived
FROM jobs;
DROP TABLE jobs;
ALTER TABLE jobs__reconcile_new RENAME TO jobs;
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
   WHERE EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key)
     AND (j.status NOT IN ('merged', 'cancelled')
          OR EXISTS (SELECT 1 FROM attempts a WHERE a.plan = j.plan AND a.job = j.key AND a.status <> 'ended'));
CREATE TRIGGER jobs_workflow_dependencies BEFORE UPDATE OF workflow ON jobs
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
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;".to_owned();
    if !has_waivers {
        // Pending additive migrations must remain responsible for adding their columns.
        sql = sql.replace("  touches_waived      TEXT CHECK (touches_waived IS NULL OR (json_valid(touches_waived) AND json_type(touches_waived) = 'array')),\n", "")
            .replace(", waiting_on, touches_waived", ", waiting_on");
    }
    conn.execute_batch(&sql).map_err(CpdError::from)
}

fn reconcile_incident_table(conn: &Connection) -> Result<(), CpdError> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS incident_hist_ins;
         DROP TRIGGER IF EXISTS incident_hist_upd;
         CREATE TABLE incident__reconcile_new (
           id             INTEGER PRIMARY KEY CHECK (id = 1),
           started_at     TEXT NOT NULL,
           started_by     TEXT NOT NULL,
           reason         TEXT NOT NULL CHECK (length(reason) >= 1),
           started_txn    INTEGER NOT NULL REFERENCES txns(id),
           lead           TEXT REFERENCES sessions(id),
           until_plan     TEXT,
           until_workflow TEXT
         ) STRICT;
         INSERT INTO incident__reconcile_new (id, started_at, started_by, reason, started_txn, lead, until_plan, until_workflow)
           SELECT id, started_at, started_by, reason, started_txn, lead, until_plan, until_workflow FROM incident;
         DROP TABLE incident;
         ALTER TABLE incident__reconcile_new RENAME TO incident;",
    )
    .map_err(CpdError::from)
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
    snapshot_store(&home.db(), &scratch_dir)?;

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
    let drifted = detect_table_drift(&conn)?;
    drop(conn);

    if divergences.is_empty() && drifted.is_empty() {
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
    if let Some(sql) = jobs_sql
        && (sql.contains("waiting_on IN ('lead', 'user')")
            || sql.contains("CHECK (waiting_on IS NULL OR waiting_on IN"))
    {
        drifted.push("jobs".to_string());
    }

    let incident_cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('incident') ORDER BY cid")
        .map_err(CpdError::from)?
        .query_map([], |r| r.get(0))
        .map_err(CpdError::from)?
        .collect::<Result<_, _>>()
        .map_err(CpdError::from)?;

    if !incident_cols.is_empty() {
        let lead_pos = incident_cols.iter().position(|c| c == "lead");
        let until_plan_pos = incident_cols.iter().position(|c| c == "until_plan");
        if let (Some(l), Some(u)) = (lead_pos, until_plan_pos)
            && u < l
        {
            drifted.push("incident".to_string());
        }
    }

    Ok(drifted)
}

/// Stub indicator for table schema reconciliation support in store repair.
/// Returns true when implemented.
pub fn is_table_reconciliation_supported() -> bool {
    true
}
