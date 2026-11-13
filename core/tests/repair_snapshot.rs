use cpd_core::home::CpdHome;
use cpd_core::store::repair::backup_store;
use rusqlite::Connection;

fn live_store(root: &std::path::Path) -> (CpdHome, Connection) {
    let home = CpdHome::resolve(Some(root.into()), None).unwrap();
    let conn = Connection::open(home.db()).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
         CREATE TABLE proof(id INTEGER PRIMARY KEY, value TEXT);
         INSERT INTO proof VALUES(1, 'committed in WAL');",
    )
    .unwrap();
    (home, conn)
}

#[test]
fn backup_is_a_self_contained_snapshot_of_committed_wal_data() {
    let dir = tempfile::tempdir().unwrap();
    let (home, live) = live_store(dir.path());
    let path = backup_store(&home, "snapshot").unwrap();
    let restored = dir.path().join("restored.db");
    std::fs::copy(path, &restored).unwrap();
    // Restore the advertised backup independently of the live WAL/shm files.
    live.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    let restored = Connection::open(restored).unwrap();
    let value: String = restored
        .query_row("SELECT value FROM proof WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "committed in WAL");
    assert_eq!(
        restored
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn an_existing_backup_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (home, live) = live_store(dir.path());
    let path = backup_store(&home, "same-second").unwrap();
    let before = std::fs::read(&path).unwrap();
    live.execute("UPDATE proof SET value='later'", []).unwrap();
    assert!(backup_store(&home, "same-second").is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn a_missing_source_is_not_reported_as_a_successful_backup() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    assert!(backup_store(&home, "missing").is_err());
    assert!(!home.db().exists());
}
