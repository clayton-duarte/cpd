//! Which owners are known: the ONE source, called by core's checks and by the TUI's New Context.

use super::resolve::OWNER_FILE;
use crate::ids::OwnerLogin;
use std::path::Path;

/// The owners known under `policy_dir`, sorted: those whose `<owner>/owner.toml` exists, as a file or as a symlink
/// that resolves (a dangling link is none). Names starting with `.`, `host.toml`, other root files and directories
/// without `owner.toml` are not owners. A missing `policy_dir` is the empty list.
pub fn known_owners(policy_dir: &Path) -> Vec<OwnerLogin> {
    let Ok(read) = std::fs::read_dir(policy_dir) else {
        return Vec::new();
    };
    let mut owners: Vec<OwnerLogin> = read
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            // `metadata` follows a link: a dangling `owner.toml` is not there.
            let known = std::fs::metadata(entry.path().join(OWNER_FILE)).is_ok_and(|m| m.is_file());
            let owner = OwnerLogin::parse(&name)
                .ok()
                .filter(|o| o.as_str() == name)?;
            known.then_some(owner)
        })
        .collect();
    owners.sort();
    owners
}

#[cfg(test)]
#[path = "known_tests.rs"]
mod tests;
