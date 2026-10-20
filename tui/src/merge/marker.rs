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

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    fn pull(repo: &str, number: i64) -> Pull {
        Pull::parse(&format!("{repo}#{number}")).unwrap()
    }

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().into()), None).unwrap()
    }

    /// A pid that is not alive: a child that has ended and been waited for.
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    /// A marker file as the action leaves it, written by hand: `file` is its name in `run/tui/merging/`.
    fn put(dir: &tempfile::TempDir, file: &str, pid: u32) {
        let merging = dir.path().join("run/tui/merging");
        std::fs::create_dir_all(&merging).unwrap();
        std::fs::write(merging.join(file), format!(r#"{{"pid": {pid}, "at": "2026-10-20T12:00:00Z"}}"#)).unwrap();
    }

    fn files(dir: &tempfile::TempDir) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.path().join("run/tui/merging"))
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn the_marker_is_one_file_per_pull_request_with_this_pid_and_an_rfc_3339_time_and_no_temporary_file_is_left() {
        let dir = tempfile::tempdir().unwrap();
        write(&home(&dir), &pull("acme/api", 3)).unwrap();
        assert_eq!(files(&dir), ["acme__api__3.json"], "one file, named <owner>__<repo>__<n>.json, no temporary file beside it");
        let text = std::fs::read_to_string(dir.path().join("run/tui/merging/acme__api__3.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["pid"], std::process::id(), "the action's pid: {text}");
        let at = json["at"].as_str().unwrap_or_else(|| panic!("an `at` string: {text}"));
        assert!(OffsetDateTime::parse(at, &Rfc3339).is_ok(), "{at:?} is RFC 3339");
        assert_eq!(json.as_object().unwrap().len(), 2, "only pid and at: {text}");
    }

    #[test]
    fn writing_replaces_a_stale_marker_and_leaves_the_markers_of_other_pull_requests() {
        let dir = tempfile::tempdir().unwrap();
        put(&dir, "acme__api__3.json", dead_pid());
        put(&dir, "acme__web__4.json", std::process::id());
        write(&home(&dir), &pull("acme/api", 3)).unwrap();
        let text = std::fs::read_to_string(dir.path().join("run/tui/merging/acme__api__3.json")).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["pid"], std::process::id(), "replaced: {text}");
        assert_eq!(files(&dir), ["acme__api__3.json", "acme__web__4.json"]);
    }

    #[test]
    fn removing_deletes_that_pull_requests_marker_only_and_a_missing_one_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        put(&dir, "acme__api__3.json", std::process::id());
        put(&dir, "acme__web__4.json", std::process::id());
        remove(&home(&dir), &pull("acme/api", 3));
        assert_eq!(files(&dir), ["acme__web__4.json"]);
        remove(&home(&dir), &pull("acme/api", 3));
        assert_eq!(files(&dir), ["acme__web__4.json"], "idempotent");
    }

    #[test]
    fn only_a_marker_whose_pid_is_alive_is_live_and_a_stale_one_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(live(&home(&dir)), BTreeSet::new(), "no directory: none");
        put(&dir, "acme__api__3.json", std::process::id());
        put(&dir, "acme__web__4.json", dead_pid());
        assert_eq!(live(&home(&dir)), BTreeSet::from(["acme/api#3".to_owned()]));
        // a restarted view reads the same files
        assert_eq!(live(&home(&dir)), live(&home(&dir)));
    }
}
