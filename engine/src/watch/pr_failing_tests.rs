//! The pr-poll-failing record is replaced by rename: the engine reads it every tick.

use std::os::unix::fs::MetadataExt;

use crate::watch::testkit::Fx;

#[test]
fn the_pr_poll_failing_record_is_replaced_by_rename() {
    let mut fx = Fx::new("");
    super::failing::write(&mut fx.engine, "gh pr list", "2026-10-30T00:00:00Z");
    let path = fx.engine.home.run_dir().join("pr-poll-failing.json");
    let before = std::fs::metadata(&path).unwrap().ino();

    super::failing::write(&mut fx.engine, "gh pr view", "2026-10-30T01:00:00Z");

    assert_ne!(
        before,
        std::fs::metadata(&path).unwrap().ino(),
        "the record must be a new inode (rename into place)"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"call": "gh pr view", "since": "2026-10-30T01:00:00Z"})
    );
    let names: Vec<_> = std::fs::read_dir(fx.engine.home.run_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| !n.ends_with(".tmp")),
        "no leftover temporary file: {names:?}"
    );
}
