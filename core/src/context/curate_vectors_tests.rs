//! Tests of the vector cache `vectors.db` (contract §4.6 "The vector cache").

use std::collections::HashSet;
use std::fs;

use rusqlite::Connection;
use tempfile::TempDir;

use super::curate_vectors::VectorStore;
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};

const AT: &str = "2026-11-03T12:00:00.000Z";
const MODEL: &str = "m1";

fn home() -> (TempDir, CpdHome) {
    let dir = TempDir::new().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    (dir, home)
}

fn rid(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

fn cid(text: &str) -> ContextId {
    ContextId::parse(text).unwrap()
}

fn set(ids: &[&str]) -> HashSet<RecordId> {
    ids.iter().map(|i| rid(i)).collect()
}

#[test]
fn vectors_round_trip_a_vector() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    let v = vec![0.5_f32, -1.25, 3.0, 0.0];
    store
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &v, AT)
        .unwrap();
    let got = store.get(&cid("cpd"), MODEL, &[rid("hs-d1")]).unwrap();
    assert_eq!(got, vec![(rid("hs-d1"), v)]);
}

#[test]
fn vectors_put_again_replaces_the_row() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    store
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0, 2.0], AT)
        .unwrap();
    store
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[7.0, 8.0, 9.0], AT)
        .unwrap();
    let got = store.get(&cid("cpd"), MODEL, &[rid("hs-d1")]).unwrap();
    assert_eq!(got, vec![(rid("hs-d1"), vec![7.0, 8.0, 9.0])]);
    assert_eq!(store.embedded(&cid("cpd"), MODEL).unwrap(), set(&["hs-d1"]));
}

#[test]
fn vectors_refuse_an_empty_or_non_finite_vector() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    for bad in [
        vec![],
        vec![1.0, f32::NAN],
        vec![f32::INFINITY],
        vec![f32::NEG_INFINITY, 1.0],
    ] {
        let result = store.put(&cid("cpd"), MODEL, &rid("hs-d1"), &bad, AT);
        assert!(result.is_err(), "{bad:?} must be refused");
    }
    assert!(store.embedded(&cid("cpd"), MODEL).unwrap().is_empty());
}

#[test]
fn vectors_read_of_a_missing_file_is_empty_and_creates_nothing() {
    let (dir, home) = home();
    let store = VectorStore::at(&home);
    assert!(
        store
            .get(&cid("cpd"), MODEL, &[rid("hs-d1")])
            .unwrap()
            .is_empty()
    );
    assert!(store.embedded(&cid("cpd"), MODEL).unwrap().is_empty());
    assert!(store.checked(&cid("cpd")).unwrap().is_empty());
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        0,
        "a read created a file"
    );
}

#[test]
fn vectors_get_returns_only_the_asked_ids() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    for (i, n) in ["hs-d1", "hs-d2", "hs-d3"].iter().enumerate() {
        store
            .put(&cid("cpd"), MODEL, &rid(n), &[i as f32 + 1.0], AT)
            .unwrap();
    }
    let mut got = store
        .get(
            &cid("cpd"),
            MODEL,
            &[rid("hs-d1"), rid("hs-d3"), rid("hs-d9")],
        )
        .unwrap();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        vec![(rid("hs-d1"), vec![1.0]), (rid("hs-d3"), vec![3.0])]
    );
    assert!(store.get(&cid("cpd"), MODEL, &[]).unwrap().is_empty());
}

#[test]
fn vectors_are_scoped_by_context_and_model() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    store
        .put(&cid("cpd"), "m1", &rid("hs-d1"), &[1.0], AT)
        .unwrap();
    store
        .put(&cid("cpd"), "m2", &rid("hs-d1"), &[2.0], AT)
        .unwrap();
    store
        .put(&cid("other"), "m1", &rid("hs-d1"), &[3.0], AT)
        .unwrap();
    let one = |c: &str, m: &str| store.get(&cid(c), m, &[rid("hs-d1")]).unwrap();
    assert_eq!(one("cpd", "m1"), vec![(rid("hs-d1"), vec![1.0])]);
    assert_eq!(one("cpd", "m2"), vec![(rid("hs-d1"), vec![2.0])]);
    assert_eq!(one("other", "m1"), vec![(rid("hs-d1"), vec![3.0])]);
    assert!(one("other", "m2").is_empty());
}

#[test]
fn vectors_embedded_lists_the_ids_of_one_context_and_model() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    store
        .put(&cid("cpd"), "m1", &rid("hs-d1"), &[1.0], AT)
        .unwrap();
    store
        .put(&cid("cpd"), "m1", &rid("hs-n2"), &[1.0], AT)
        .unwrap();
    store
        .put(&cid("cpd"), "m2", &rid("hs-d3"), &[1.0], AT)
        .unwrap();
    store
        .put(&cid("other"), "m1", &rid("hs-d4"), &[1.0], AT)
        .unwrap();
    assert_eq!(
        store.embedded(&cid("cpd"), "m1").unwrap(),
        set(&["hs-d1", "hs-n2"])
    );
    assert_eq!(store.embedded(&cid("cpd"), "m2").unwrap(), set(&["hs-d3"]));
    assert!(store.embedded(&cid("cpd"), "m3").unwrap().is_empty());
}

#[test]
fn vectors_checked_marker_is_idempotent_and_per_context() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    store.mark_checked(&cid("cpd"), &rid("hs-d1"), AT).unwrap();
    store.mark_checked(&cid("cpd"), &rid("hs-d1"), AT).unwrap();
    store.mark_checked(&cid("cpd"), &rid("hs-d2"), AT).unwrap();
    store
        .mark_checked(&cid("other"), &rid("hs-d3"), AT)
        .unwrap();
    assert_eq!(
        store.checked(&cid("cpd")).unwrap(),
        set(&["hs-d1", "hs-d2"])
    );
    assert_eq!(store.checked(&cid("other")).unwrap(), set(&["hs-d3"]));
    assert!(store.checked(&cid("third")).unwrap().is_empty());
}

/// Asserts `e` is `store_failed` naming `path` in its message and details.
fn assert_store_failed(e: &crate::error::CpdError, path: &std::path::Path) {
    let shown = path.display().to_string();
    assert_eq!(e.code(), ErrorCode::StoreFailed, "{e:?}");
    assert!(
        e.message().contains(&shown),
        "message {:?} lacks {shown}",
        e.message()
    );
    assert_eq!(
        e.details().get("path").and_then(|v| v.as_str()),
        Some(shown.as_str())
    );
}

#[test]
fn vectors_corrupt_file_is_store_failed_with_the_path() {
    let (_dir, home) = home();
    fs::write(home.vectors_db(), vec![0x42_u8; 8192]).unwrap();
    let store = VectorStore::at(&home);
    let e = store.embedded(&cid("cpd"), MODEL).unwrap_err();
    assert_store_failed(&e, &home.vectors_db());
    let e = store
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0], AT)
        .unwrap_err();
    assert_store_failed(&e, &home.vectors_db());
}

#[test]
fn vectors_foreign_application_id_is_refused() {
    let (_dir, home) = home();
    {
        let conn = Connection::open(home.vectors_db()).unwrap();
        conn.pragma_update(None, "application_id", 1_i32).unwrap();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
    }
    let store = VectorStore::at(&home);
    let e = store.embedded(&cid("cpd"), MODEL).unwrap_err();
    assert_store_failed(&e, &home.vectors_db());
    let e = store
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0], AT)
        .unwrap_err();
    assert_store_failed(&e, &home.vectors_db());
}

#[test]
fn vectors_too_new_user_version_is_refused() {
    let (_dir, home) = home();
    {
        let conn = Connection::open(home.vectors_db()).unwrap();
        conn.pragma_update(None, "application_id", 1_668_703_587_i32)
            .unwrap();
        conn.pragma_update(None, "user_version", 2_i32).unwrap();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
    }
    let store = VectorStore::at(&home);
    let e = store.checked(&cid("cpd")).unwrap_err();
    assert_store_failed(&e, &home.vectors_db());
    let e = store
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0], AT)
        .unwrap_err();
    assert_store_failed(&e, &home.vectors_db());
}

#[test]
fn vectors_second_open_of_a_new_file_keeps_wal_and_loses_no_row() {
    let (_dir, home) = home();
    VectorStore::at(&home)
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0], AT)
        .unwrap();
    let second = VectorStore::at(&home);
    second
        .put(&cid("cpd"), MODEL, &rid("hs-d2"), &[2.0], AT)
        .unwrap();
    assert_eq!(
        second.embedded(&cid("cpd"), MODEL).unwrap(),
        set(&["hs-d1", "hs-d2"])
    );
    let conn = Connection::open(home.vectors_db()).unwrap();
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let app: i32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    let version: i32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!((app, version), (1_668_703_587, 1));
}
