//! Store repair stubs (Plan `store-repair`).

use crate::error::CpdError;
use crate::home::CpdHome;
use std::path::PathBuf;

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

/// Repairs a diverged store at `home`.
pub fn repair(_home: &CpdHome, _options: &RepairOptions) -> Result<RepairResult, CpdError> {
    // Skeleton implementation
    Err(CpdError::new(
        crate::error::ErrorCode::Internal,
        "store repair skeleton",
    ))
}
