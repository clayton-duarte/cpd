//! Stubs and types for cpd upgrade (docs/architecture/upgrade.md, docs/commands.md §14.4c).

use std::path::PathBuf;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ports::Ports;

/// Configuration for `cpd upgrade`.
#[derive(Debug, Clone)]
pub struct UpgradeConfig {
    pub home: CpdHome,
    pub dry_run: bool,
    pub check_only: bool,
    pub target_bin: Option<PathBuf>,
}

/// Upgrade outcome details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeOutcome {
    pub from_sha: String,
    pub to_sha: String,
    pub backup_path: Option<PathBuf>,
    pub already: bool,
}

/// Run `cpd upgrade` against the given ports and configuration.
pub fn run_upgrade(_ports: &Ports, _config: &UpgradeConfig) -> Result<UpgradeOutcome, CpdError> {
    todo!("implement cpd upgrade: fetch CI-green commit, isolated build, migration dry run, backup, swap, health check, rollback")
}
