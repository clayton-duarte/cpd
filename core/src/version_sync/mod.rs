//! `cpd version sync [--print]` (docs/commands.md §26.2) and the migration file the sync follows. Files only (std `fs`).
//! STUB: the logic is the next Job's.

use std::path::{Path, PathBuf};

use crate::ports::Ports;

/// `0.<PROTOCOL>.<migrations>` of the repository at `root`; with `print` nothing is written. `Ok` is the stdout line
/// without its newline: the version for `print`, else `sync-version: <version>`. `Err` is the one `sync-version: ...`
/// line of a failure (no newline), and nothing is written then.
pub fn sync(_root: &Path, _print: bool) -> Result<String, String> {
    todo!("cpd version sync")
}

/// Creates `core/src/store/migrations/<YYYYMMDDTHHMM>-<slug>.sql` (the UTC minute of `ports.clock`) holding
/// `-- cpd-migration: additive`, runs [`sync`] and returns the file's path.
pub fn new_migration(_ports: &Ports, _root: &Path, _slug: &str) -> Result<PathBuf, String> {
    todo!("new migration")
}

#[cfg(test)]
mod tests;
