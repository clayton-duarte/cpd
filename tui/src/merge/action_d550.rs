//! Red tests of the merge action's marker handling (hs-d550, B4).

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::PathBuf;

use super::*;
use cpd_core::output::Output;

thread_local! {
    /// The home whose store the fake `pr poll` writes to, and whether it sets the row merged.
    static HOME: RefCell<Option<CpdHome>> = const { RefCell::new(None) };
    static POLL_MERGES: Cell<bool> = const { Cell::new(false) };
}

fn fake(words: Vec<OsString>) -> Result<Output, CpdError> {
    let words: Vec<String> = words
        .iter()
        .map(|w| w.to_string_lossy().into_owned())
        .collect();
    if words.get(1).map(String::as_str) == Some("pr")
        && words.get(2).map(String::as_str) == Some("poll")
        && POLL_MERGES.get()
    {
        let home = HOME.with_borrow(|h| h.clone().unwrap());
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE number = 7",
                )?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }
    Ok(Output::Owned)
}

fn setup(dir: &tempfile::TempDir, rows: &[(i64, &str)]) -> CpdHome {
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            for (number, state) in rows {
                tx.execute(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merge_state, authored, merged_at)
                     VALUES ('acme/api', ?1, 'https://github.com/acme/api/pull/' || ?1, 'T', 0, ?2, 'none', 'none', 't', 't', 'clean', 1, CASE WHEN ?2 = 'merged' THEN 't' END)",
                    (number, state),
                )?;
            }
            Ok::<(), CpdError>(())
        })
        .unwrap();
    HOME.with_borrow_mut(|h| *h = Some(home.clone()));
    home
}

fn marker_file(dir: &tempfile::TempDir, number: i64) -> PathBuf {
    dir.path()
        .join(format!("run/tui/merging/acme__api__{number}.json"))
}

fn run_action(home: &CpdHome, merges: bool) {
    POLL_MERGES.set(merges);
    let pull = Pull::parse("acme/api#7").unwrap();
    action(home, &Tmux::new("cpd-d550-unused"), &pull, "client", fake).unwrap();
}

#[test]
fn a_successful_merge_whose_row_stays_open_keeps_its_marker_marked_merged() {
    let dir = tempfile::tempdir().unwrap();
    let home = setup(&dir, &[(7, "open")]);
    run_action(&home, false);
    let text = std::fs::read_to_string(marker_file(&dir, 7))
        .expect("the marker stays while the row is open");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["merged"], true, "{text}");
}

#[test]
fn a_successful_merge_whose_row_is_merged_after_the_poll_removes_its_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = setup(&dir, &[(7, "open")]);
    run_action(&home, true);
    assert!(
        !marker_file(&dir, 7).exists(),
        "no marker once the row is merged"
    );
}

#[test]
fn the_action_first_removes_merged_markers_whose_row_is_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let home = setup(&dir, &[(7, "open"), (8, "merged"), (9, "open")]);
    let merging = dir.path().join("run/tui/merging");
    std::fs::create_dir_all(&merging).unwrap();
    for n in [8, 9] {
        let text = r#"{"pid": 999999999, "at": "2026-10-22T12:00:00Z", "merged": true}"#;
        std::fs::write(marker_file(&dir, n), text).unwrap();
    }
    run_action(&home, false);
    assert!(
        !marker_file(&dir, 8).exists(),
        "the merged marker of a merged row is removed"
    );
    assert!(
        marker_file(&dir, 9).exists(),
        "the merged marker of an open row stays"
    );
}
