//! The merge marker (commands.md §13.2 "A merge in progress", contract §1.4; hs-d495): one file per pull request,
//! `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json`, `{"pid", "at"}`, that `cpd tui action merge` writes before
//! `cpd pr merge` and removes when the request ends. Every view reads it on each snapshot ([`live`]): a marker whose
//! pid is not alive is stale and ignored.
//!
//! The functions are stubs until Job `ms-impl`: the red tests pin what they must do.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::Pull;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;

/// The marker file of `pull`: `<owner>/<repo>#<n>` is `<owner>__<repo>__<n>.json`.
pub fn path(home: &CpdHome, pull: &Pull) -> PathBuf {
    let (owner_repo, number) = (pull.repo.replace('/', "__"), pull.number);
    home.run_dir().join("tui").join("merging").join(format!("{owner_repo}__{number}.json"))
}

/// Writes the marker of `pull` for this process (`{"pid": <this pid>, "at": <RFC 3339 now>}`), atomically (a temporary
/// file, then a rename), replacing any marker there, a stale one included.
pub fn write(home: &CpdHome, pull: &Pull) -> Result<(), CpdError> {
    let _ = (home, pull);
    Ok(())
}

/// Removes the marker of `pull`; none there is no error.
pub fn remove(home: &CpdHome, pull: &Pull) {
    let _ = (home, pull);
}

/// The pull requests, as `<owner>/<repo>#<n>`, that have a live marker: one whose `pid` is alive. A stale one is none.
pub fn live(home: &CpdHome) -> BTreeSet<String> {
    let _ = home;
    BTreeSet::new()
}
