//! The merge marker (commands.md §13.2 "A merge in progress", contract §1.4; hs-d495): one file per pull request,
//! `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json`, `{"pid", "at"}`, that `cpd tui action merge` writes before
//! `cpd pr merge` and removes when the request ends. Every view reads it on each snapshot ([`live`]): a marker whose
//! pid is not alive is stale and ignored.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::Pull;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The marker file of `pull`: `<owner>/<repo>#<n>` is `<owner>__<repo>__<n>.json`.
pub fn path(home: &CpdHome, pull: &Pull) -> PathBuf {
    let (owner_repo, number) = (pull.repo.replace('/', "__"), pull.number);
    path_dir(home).join(format!("{owner_repo}__{number}.json"))
}

/// Writes the marker of `pull` for this process (`{"pid": <this pid>, "at": <RFC 3339 now>}`), atomically (a temporary
/// file, then a rename), replacing any marker there, a stale one included.
pub fn write(home: &CpdHome, pull: &Pull) -> Result<(), CpdError> {
    let target = path(home, pull);
    let pid = std::process::id();
    let at = OffsetDateTime::now_utc().format(&Rfc3339).expect("a UTC time is RFC 3339");
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::Internal, format!("Cannot write the merge marker {}: {e}.", target.display()))
            .with("path", target.display().to_string())
    };
    let dir = target.parent().expect("a marker is in a directory");
    std::fs::create_dir_all(dir).map_err(failed)?;
    // The temporary name does not end in `.json`, so no reader takes it for a marker; the pid keeps two writers apart.
    let temporary = dir.join(format!(".{pid}.tmp"));
    let text = serde_json::json!({ "pid": pid, "at": at }).to_string();
    std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, &target)).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        failed(e)
    })
}

/// Removes the marker of `pull`; none there is no error.
pub fn remove(home: &CpdHome, pull: &Pull) {
    let _ = std::fs::remove_file(path(home, pull));
}

/// The pull requests, as `<owner>/<repo>#<n>`, that have a live marker: one whose `pid` is alive. A stale one is none,
/// and so is a file that is not a marker (a view cannot fail on it, and the next merge action replaces it).
/// Stub (hs-d550): `open` is ignored until implemented.
pub fn live(home: &CpdHome, _open: &BTreeSet<String>) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(path_dir(home)) else { return BTreeSet::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (owner_repo, number) = name.strip_suffix(".json")?.rsplit_once("__")?;
            let (owner, repo) = owner_repo.split_once("__")?;
            let marker: Marker = serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            alive(marker.pid).then(|| format!("{owner}/{repo}#{number}"))
        })
        .collect()
}

/// A marker file's content: only the `pid` is read (`at` is for whoever reads the file).
#[derive(Deserialize)]
struct Marker {
    pid: i64,
}

/// The directory of the markers.
fn path_dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("tui").join("merging")
}

/// Whether process `pid` exists (signal 0). A process of another user exists too (`EPERM`).
fn alive(pid: i64) -> bool {
    // 0 and the negatives would signal a process group, not a process.
    let Some(pid) = i32::try_from(pid).ok().filter(|p| *p > 0) else { return false };
    !matches!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(live(&home(&dir), &BTreeSet::new()), BTreeSet::new(), "no directory: none");
        put(&dir, "acme__api__3.json", std::process::id());
        put(&dir, "acme__web__4.json", dead_pid());
        assert_eq!(live(&home(&dir), &BTreeSet::new()), BTreeSet::from(["acme/api#3".to_owned()]));
        // a restarted view reads the same files
        assert_eq!(live(&home(&dir), &BTreeSet::new()), live(&home(&dir), &BTreeSet::new()));
    }
}

#[cfg(test)]
#[path = "marker_d550.rs"]
mod d550;
