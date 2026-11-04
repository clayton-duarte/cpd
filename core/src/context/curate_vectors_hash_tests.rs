//! Tests of `vectors.db` version 2: the `content_hash` column, the in-place upgrade and `content_hash()`.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;

use super::curate_vectors::{VectorStore, content_hash};
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};

const AT: &str = "2026-11-04T12:00:00.000Z";
const MODEL: &str = "m1";
const APP_ID: i32 = 1_668_703_587;

/// The version 1 tables, as the first release wrote them.
const V1_DDL: &str = "CREATE TABLE context_embeddings (context TEXT NOT NULL CHECK (length(context) BETWEEN 2 AND 40), model TEXT NOT NULL CHECK (length(model) BETWEEN 1 AND 128), record TEXT NOT NULL CHECK (length(record) BETWEEN 1 AND 64), dims INTEGER NOT NULL CHECK (dims > 0), vector BLOB NOT NULL CHECK (length(vector) = dims * 4), at TEXT NOT NULL, PRIMARY KEY (context, model, record)) STRICT, WITHOUT ROWID;
CREATE TABLE context_checked (context TEXT NOT NULL, record TEXT NOT NULL, at TEXT NOT NULL, PRIMARY KEY (context, record)) STRICT, WITHOUT ROWID;";

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

/// A version 1 file holding one row of `hs-d1`, made with raw rusqlite.
fn version_1_file(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(V1_DDL).unwrap();
    conn.execute(
        "INSERT INTO context_embeddings (context, model, record, dims, vector, at) VALUES ('cpd', 'm1', 'hs-d1', 2, ?1, ?2)",
        rusqlite::params![[1.0_f32, 2.0].iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>(), AT],
    )
    .unwrap();
    conn.pragma_update(None, "application_id", APP_ID).unwrap();
    conn.pragma_update(None, "user_version", 1_i32).unwrap();
}

fn user_version(path: &Path) -> i32 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn columns(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('context_embeddings') ORDER BY cid")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn vectors_db_v2_creates_the_content_hash_column() {
    let (_dir, home) = home();
    VectorStore::at(&home)
        .put(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0], AT)
        .unwrap();
    let path = home.vectors_db();
    assert_eq!(user_version(&path), 2);
    assert!(columns(&path).contains(&"content_hash".to_owned()));
    let conn = Connection::open(&path).unwrap();
    let (kind, not_null, default): (String, i64, String) = conn
        .query_row(
            "SELECT type, \"notnull\", dflt_value FROM pragma_table_info('context_embeddings') WHERE name = 'content_hash'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (kind.as_str(), not_null, default.as_str()),
        ("TEXT", 1, "''")
    );
}

#[test]
fn version_1_file_is_upgraded_in_place_by_the_writer_and_keeps_its_rows() {
    let (_dir, home) = home();
    let path = home.vectors_db();
    version_1_file(&path);
    let store = VectorStore::at(&home);
    store
        .put_hashed(&cid("cpd"), MODEL, &rid("hs-d2"), &[3.0], "abc")
        .unwrap();
    assert_eq!(user_version(&path), 2);
    assert!(columns(&path).contains(&"content_hash".to_owned()));
    let got = store
        .get(&cid("cpd"), MODEL, &[rid("hs-d1"), rid("hs-d2")])
        .unwrap();
    assert_eq!(
        got,
        vec![(rid("hs-d1"), vec![1.0, 2.0]), (rid("hs-d2"), vec![3.0])]
    );
    let hashes = store
        .hashes(&cid("cpd"), MODEL, &[rid("hs-d1"), rid("hs-d2")])
        .unwrap();
    let want: HashMap<RecordId, String> = [
        (rid("hs-d1"), String::new()),
        (rid("hs-d2"), "abc".to_owned()),
    ]
    .into();
    assert_eq!(hashes, want);
}

#[test]
fn the_reader_accepts_a_version_1_file_without_upgrading_it() {
    let (_dir, home) = home();
    let path = home.vectors_db();
    version_1_file(&path);
    let store = VectorStore::at(&home);
    let hashes = store.hashes(&cid("cpd"), MODEL, &[rid("hs-d1")]).unwrap();
    let want: HashMap<RecordId, String> = [(rid("hs-d1"), String::new())].into();
    assert_eq!(hashes, want);
    assert_eq!(
        store.get(&cid("cpd"), MODEL, &[rid("hs-d1")]).unwrap(),
        vec![(rid("hs-d1"), vec![1.0, 2.0])]
    );
    assert_eq!(user_version(&path), 1);
    assert!(!columns(&path).contains(&"content_hash".to_owned()));
}

#[test]
fn a_version_3_file_is_refused() {
    let (_dir, home) = home();
    {
        let conn = Connection::open(home.vectors_db()).unwrap();
        conn.pragma_update(None, "application_id", APP_ID).unwrap();
        conn.pragma_update(None, "user_version", 3_i32).unwrap();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
    }
    let store = VectorStore::at(&home);
    let e = store
        .hashes(&cid("cpd"), MODEL, &[rid("hs-d1")])
        .unwrap_err();
    assert_eq!(e.code(), ErrorCode::StoreFailed);
    let e = store
        .put_hashed(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0], "h")
        .unwrap_err();
    assert_eq!(e.code(), ErrorCode::StoreFailed);
}

#[test]
fn put_hashed_then_hashes_returns_the_hash() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    store
        .put_hashed(&cid("cpd"), MODEL, &rid("hs-d1"), &[1.0, 2.0], "h1")
        .unwrap();
    store
        .put(&cid("cpd"), MODEL, &rid("hs-d2"), &[1.0], AT)
        .unwrap();
    store
        .put_hashed(&cid("cpd"), MODEL, &rid("hs-d1"), &[5.0], "h2")
        .unwrap();
    let got = store
        .hashes(
            &cid("cpd"),
            MODEL,
            &[rid("hs-d1"), rid("hs-d2"), rid("hs-d3")],
        )
        .unwrap();
    let want: HashMap<RecordId, String> = [
        (rid("hs-d1"), "h2".to_owned()),
        (rid("hs-d2"), String::new()),
    ]
    .into();
    assert_eq!(got, want);
    assert_eq!(
        store.get(&cid("cpd"), MODEL, &[rid("hs-d1")]).unwrap(),
        vec![(rid("hs-d1"), vec![5.0])]
    );
    let other_model = store.hashes(&cid("cpd"), "m2", &[rid("hs-d1")]).unwrap();
    assert!(other_model.is_empty());
}

#[test]
fn hashes_of_a_missing_file_is_empty_and_creates_nothing() {
    let (_dir, home) = home();
    let got = VectorStore::at(&home)
        .hashes(&cid("cpd"), MODEL, &[rid("hs-d1")])
        .unwrap();
    assert!(got.is_empty());
    assert!(!home.vectors_db().exists());
}

#[test]
fn content_hash_is_sha256_hex_of_the_text() {
    assert_eq!(
        content_hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        content_hash(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
