//! `Store::data_version`: moves only for commits of other connections.

use super::*;

fn open_two() -> (tempfile::TempDir, Store, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let a = Store::open(&home, 5000).unwrap();
    let b = Store::open(&home, 5000).unwrap();
    (dir, a, b)
}

#[test]
fn data_version_is_unchanged_when_nothing_committed() {
    let (_dir, mut a, _b) = open_two();
    let first = a.data_version().unwrap();
    assert_eq!(a.data_version().unwrap(), first);
}

#[test]
fn data_version_is_unchanged_by_this_connections_own_commit() {
    let (_dir, mut a, _b) = open_two();
    let before = a.data_version().unwrap();
    a.write("test", |_| Ok(())).unwrap();
    assert_eq!(a.data_version().unwrap(), before);
}

#[test]
fn data_version_changes_when_another_connection_commits() {
    let (_dir, mut a, mut b) = open_two();
    let before = a.data_version().unwrap();
    b.write("test", |_| Ok(())).unwrap();
    assert_ne!(a.data_version().unwrap(), before);
}
