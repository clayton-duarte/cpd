//! The merge marker (commands.md §13.2 "A merge in progress", contract §1.4; hs-d495): one file per pull request,
//! `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json`, `{"pid", "at"}`, that `cpd tui action merge` writes before
//! `cpd pr merge` and removes when the request ends. Every view reads it on each snapshot ([`live`]): a marker whose
//! pid is not alive is stale and ignored.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::Pull;
use cpd_core::atomic;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::Pid;
use cpd_core::ports::Ports;
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;

/// The marker file of `pull`: `<owner>/<repo>#<n>` is `<owner>__<repo>__<n>.json`.
pub fn path(home: &CpdHome, pull: &Pull) -> PathBuf {
    let (owner_repo, number) = (pull.repo.replace('/', "__"), pull.number);
    path_dir(home).join(format!("{owner_repo}__{number}.json"))
}

/// Writes the marker of `pull` for this process (`{"pid": <this pid>, "at": <RFC 3339 now>}`), atomically (a temporary
/// file, then a rename), replacing any marker there, a stale one included.
pub fn write(home: &CpdHome, pull: &Pull) -> Result<(), CpdError> {
    write_on(&Ports::current(), home, pull)
}

/// [`write`] stamped by the clock of `ports`.
pub fn write_on(ports: &Ports, home: &CpdHome, pull: &Pull) -> Result<(), CpdError> {
    write_with(ports, home, pull, false)
}

/// Rewrites the marker of `pull` as `{"pid", "at", "merged": true}` (hs-d550): `pr merge` succeeded, and the marker
/// holds until a read stores the merge. Atomic, as [`write`].
pub fn write_merged(home: &CpdHome, pull: &Pull) -> Result<(), CpdError> {
    write_merged_on(&Ports::current(), home, pull)
}

/// [`write_merged`] stamped by the clock of `ports`.
pub fn write_merged_on(ports: &Ports, home: &CpdHome, pull: &Pull) -> Result<(), CpdError> {
    write_with(ports, home, pull, true)
}

fn write_with(ports: &Ports, home: &CpdHome, pull: &Pull, merged: bool) -> Result<(), CpdError> {
    let target = path(home, pull);
    let pid = std::process::id();
    let at = ports
        .clock
        .utc()
        .format(&Rfc3339)
        .expect("a UTC time is RFC 3339");
    let failed = |e: std::io::Error| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot write the merge marker {}: {e}.", target.display()),
        )
        .with("path", target.display().to_string())
    };
    let mut json = serde_json::json!({ "pid": pid, "at": at });
    if merged {
        json["merged"] = true.into();
    }
    let text = json.to_string();
    // The temporary name ends in `.tmp`, never `.json`, so no reader takes it for a marker.
    atomic::write_atomic(&target, text).map_err(failed)
}

/// Removes the marker of `pull`; none there is no error.
pub fn remove(home: &CpdHome, pull: &Pull) {
    let _ = std::fs::remove_file(path(home, pull));
}

/// Every readable marker: its file, its pull request as `<owner>/<repo>#<n>` and its content. A file that is not a
/// marker is none (a view cannot fail on it, and the next merge action replaces it).
fn markers(home: &CpdHome) -> Vec<(PathBuf, String, Marker)> {
    let Ok(entries) = std::fs::read_dir(path_dir(home)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (owner_repo, number) = name.strip_suffix(".json")?.rsplit_once("__")?;
            let (owner, repo) = owner_repo.split_once("__")?;
            let marker: Marker =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            Some((entry.path(), format!("{owner}/{repo}#{number}"), marker))
        })
        .collect()
}

/// The pull requests, as `<owner>/<repo>#<n>`, that are merging (hs-d550): a marker whose `pid` is alive, or one marked
/// `merged` whose pull request is in `open` (the open rows of `pull_requests`: no read has stored the merge yet). A
/// stale marker, and a `merged` one whose pull request is not open, are none.
pub fn live(home: &CpdHome, open: &BTreeSet<String>) -> BTreeSet<String> {
    live_with(&Ports::current(), home, open)
}

/// [`live`] with the pids asked of `ports.spawn`.
pub fn live_with(ports: &Ports, home: &CpdHome, open: &BTreeSet<String>) -> BTreeSet<String> {
    markers(home)
        .into_iter()
        .filter(|(_, pull, marker)| {
            u32::try_from(marker.pid).is_ok_and(|pid| ports.spawn.alive(Pid(pid)))
                || (marker.merged && open.contains(pull))
        })
        .map(|(_, pull, _)| pull)
        .collect()
}

/// Removes every marker marked `merged` whose pull request is not in `open`: the only clean-up of one left behind.
pub fn remove_settled(home: &CpdHome, open: &BTreeSet<String>) {
    for (file, pull, marker) in markers(home) {
        if marker.merged && !open.contains(&pull) {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// A marker file's content: the `pid` and whether `pr merge` succeeded (`at` is for whoever reads the file).
#[derive(Deserialize)]
struct Marker {
    pid: i64,
    #[serde(default)]
    merged: bool,
}

/// The directory of the markers.
fn path_dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("tui").join("merging")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn pull(repo: &str, number: i64) -> Pull {
        Pull::parse(&format!("{repo}#{number}")).unwrap()
    }

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().into()), None).unwrap()
    }

    /// A pid that is not alive: above every platform's `pid_max` (Linux's is at most 2^22), so no process has it; `kill(pid, 0)` says `ESRCH`.
    const DEAD_PID: u32 = 999_999_999;

    /// A marker file as the action leaves it, written by hand: `file` is its name in `run/tui/merging/`.
    fn put(dir: &tempfile::TempDir, file: &str, pid: u32) {
        let merging = dir.path().join("run/tui/merging");
        std::fs::create_dir_all(&merging).unwrap();
        std::fs::write(
            merging.join(file),
            format!(r#"{{"pid": {pid}, "at": "2026-10-20T12:00:00Z"}}"#),
        )
        .unwrap();
    }

    fn files(dir: &tempfile::TempDir) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.path().join("run/tui/merging"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn the_marker_is_one_file_per_pull_request_with_this_pid_and_an_rfc_3339_time_and_no_temporary_file_is_left()
     {
        let dir = tempfile::tempdir().unwrap();
        write(&home(&dir), &pull("acme/api", 3)).unwrap();
        assert_eq!(
            files(&dir),
            ["acme__api__3.json"],
            "one file, named <owner>__<repo>__<n>.json, no temporary file beside it"
        );
        let text =
            std::fs::read_to_string(dir.path().join("run/tui/merging/acme__api__3.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["pid"], std::process::id(), "the action's pid: {text}");
        let at = json["at"]
            .as_str()
            .unwrap_or_else(|| panic!("an `at` string: {text}"));
        assert!(
            OffsetDateTime::parse(at, &Rfc3339).is_ok(),
            "{at:?} is RFC 3339"
        );
        assert_eq!(
            json.as_object().unwrap().len(),
            2,
            "only pid and at: {text}"
        );
    }

    #[test]
    fn writing_replaces_a_stale_marker_and_leaves_the_markers_of_other_pull_requests() {
        let dir = tempfile::tempdir().unwrap();
        put(&dir, "acme__api__3.json", DEAD_PID);
        put(&dir, "acme__web__4.json", std::process::id());
        write(&home(&dir), &pull("acme/api", 3)).unwrap();
        let text =
            std::fs::read_to_string(dir.path().join("run/tui/merging/acme__api__3.json")).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap()["pid"],
            std::process::id(),
            "replaced: {text}"
        );
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
        assert_eq!(
            live(&home(&dir), &BTreeSet::new()),
            BTreeSet::new(),
            "no directory: none"
        );
        put(&dir, "acme__api__3.json", std::process::id());
        put(&dir, "acme__web__4.json", DEAD_PID);
        assert_eq!(
            live(&home(&dir), &BTreeSet::new()),
            BTreeSet::from(["acme/api#3".to_owned()])
        );
        // a restarted view reads the same files
        assert_eq!(
            live(&home(&dir), &BTreeSet::new()),
            live(&home(&dir), &BTreeSet::new())
        );
    }
}

#[cfg(test)]
#[path = "marker_d550.rs"]
mod d550;
