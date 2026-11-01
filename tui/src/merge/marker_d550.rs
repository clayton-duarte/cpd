//! Red tests of the `merged` marker flag (hs-d550, B5).

use std::collections::BTreeSet;

use cpd_core::home::CpdHome;

use super::live;

fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

fn merged_marker(dir: &tempfile::TempDir) -> CpdHome {
    let merging = dir.path().join("run/tui/merging");
    std::fs::create_dir_all(&merging).unwrap();
    let text = format!(
        r#"{{"pid": {}, "at": "2026-10-22T12:00:00Z", "merged": true}}"#,
        dead_pid()
    );
    std::fs::write(merging.join("acme__api__3.json"), text).unwrap();
    CpdHome::resolve(Some(dir.path().into()), None).unwrap()
}

#[test]
fn a_merged_marker_with_a_dead_pid_is_live_while_its_pull_request_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let home = merged_marker(&dir);
    let open = BTreeSet::from(["acme/api#3".to_owned()]);
    assert_eq!(
        live(&home, &open),
        BTreeSet::from(["acme/api#3".to_owned()])
    );
}

#[test]
fn a_merged_marker_is_not_live_once_its_pull_request_is_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let home = merged_marker(&dir);
    let open = BTreeSet::from(["acme/api#3".to_owned()]);
    assert_eq!(
        live(&home, &open),
        BTreeSet::from(["acme/api#3".to_owned()]),
        "live while open"
    );
    assert_eq!(
        live(&home, &BTreeSet::new()),
        BTreeSet::new(),
        "ignored once not open"
    );
}
