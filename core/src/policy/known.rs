//! Which owners are known: the ONE source, called by core's checks and by the TUI's New Context.

use crate::ids::OwnerLogin;
use std::path::Path;

/// The owners known under `policy_dir`, sorted: those whose `<owner>/owner.toml` exists, as a file or as a symlink
/// that resolves (a dangling link is none). Names starting with `.`, `host.toml`, other root files and directories
/// without `owner.toml` are not owners. A missing `policy_dir` is the empty list.
pub fn known_owners(_policy_dir: &Path) -> Vec<OwnerLogin> {
    todo!("one policy tree: known owners")
}

#[cfg(test)]
#[path = "known_tests.rs"]
mod tests;
