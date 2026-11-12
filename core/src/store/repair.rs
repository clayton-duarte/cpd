//! Store repair stubs (Plan `store-repair`).

use std::path::Path;
use crate::error::CpdError;

#[derive(Debug, Clone, Default)]
pub struct RepairOptions {
    pub dry_run: bool,
    pub confirmed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct RepairResult {
    pub repaired: bool,
    pub backup_path: Option<std::path::PathBuf>,
}

pub fn repair_store(_db_path: &Path, _options: &RepairOptions) -> Result<RepairResult, CpdError> {
    panic!("store repair not yet implemented");
}
