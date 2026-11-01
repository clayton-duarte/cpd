//! `Store::open_with`: the store's waits run on a `Clock`. The held lock is a connection (or a file lock) the test
//! owns; no child process, no real wait.

use std::fs::OpenOptions;
use std::time::Duration;

use rusqlite::Connection;

use super::*;
use crate::clock::ManualClock;
use crate::error::ErrorCode;

fn home() -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    (dir, home)
}

fn persist() -> Option<Persist> {
    Some(Persist::new(|_| {}, || false))
}

/// A new database file (no first open: rollback-journal mode) that a connection of the test holds with `BEGIN EXCLUSIVE`,
/// as when another process is still setting it up: a plain open is busy.
fn held_database(home: &CpdHome) -> Connection {
    std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
    let other = Connection::open(home.db()).unwrap();
    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    other
}

#[test]
fn a_held_database_is_busy_for_a_plain_open_at_once() {
    let (_dir, home) = home();
    let _held = held_database(&home);
    let err = Store::open(&home, 0).err().expect("held");
    assert!(is_transient(&err), "{err:?}");
}

#[test]
fn a_database_that_is_held_is_waited_for_with_a_back_off_of_500_ms_doubling_to_5_s() {
    let (_dir, home) = home();
    let _held = held_database(&home);
    let clock = ManualClock::new();
    let _ = Store::open_with(&clock, &home, 0, persist());
    let naps = clock.naps();
    let expected = [500, 1000, 2000, 4000, 5000, 5000].map(Duration::from_millis);
    assert!(naps.len() >= expected.len(), "naps: {naps:?}");
    assert_eq!(naps[..expected.len()], expected);
    assert!(
        naps[expected.len()..].iter().all(|n| *n == BACKOFF_MAX),
        "naps: {naps:?}"
    );
}

#[test]
fn a_database_that_stays_held_gives_up_once_the_migration_wait_has_passed_in_virtual_time() {
    let (_dir, home) = home();
    let _held = held_database(&home);
    let clock = ManualClock::new();
    let err = Store::open_with(&clock, &home, 0, persist())
        .err()
        .expect("still held");
    assert!(is_transient(&err), "{err:?}");
    let ceiling = lock::MIGRATION_WAIT_CEILING;
    let naps = clock.naps();
    assert!(
        clock.elapsed() >= ceiling,
        "gave up after {:?}",
        clock.elapsed()
    );
    let before_last = clock.elapsed() - *naps.last().expect("it napped");
    assert!(
        before_last < ceiling,
        "kept waiting past {ceiling:?}: {:?}",
        clock.elapsed()
    );
}

#[test]
fn a_database_held_without_persisting_returns_the_busy_error_at_once_without_a_nap() {
    let (_dir, home) = home();
    let _held = held_database(&home);
    let clock = ManualClock::new();
    let err = Store::open_with(&clock, &home, 0, None)
        .err()
        .expect("held");
    assert!(is_transient(&err), "{err:?}");
    assert!(clock.naps().is_empty());
}

#[test]
fn a_setup_lock_that_is_held_is_retried_every_20_ms() {
    let (_dir, home) = home();
    std::fs::create_dir_all(home.locks_dir()).unwrap();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.locks_dir().join("store.lock"))
        .unwrap();
    assert!(crate::lock::try_lock(&file).unwrap());
    let clock = ManualClock::new();
    let _ = Store::open_with(&clock, &home, 100, None);
    assert_eq!(clock.naps(), vec![Duration::from_millis(20); 5]);
}

#[test]
fn a_setup_lock_that_stays_held_is_lock_timeout_after_store_busy_ms_of_virtual_time() {
    let (_dir, home) = home();
    std::fs::create_dir_all(home.locks_dir()).unwrap();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.locks_dir().join("store.lock"))
        .unwrap();
    assert!(crate::lock::try_lock(&file).unwrap());
    let clock = ManualClock::new();
    let err = Store::open_with(&clock, &home, 100, None)
        .err()
        .expect("held");
    assert_eq!(err.code(), ErrorCode::LockTimeout);
    assert!(clock.elapsed() >= Duration::from_millis(100));
}

#[test]
fn a_database_released_before_the_call_opens_at_the_first_attempt_without_a_nap() {
    let (_dir, home) = home();
    let held = held_database(&home);
    held.execute_batch("ROLLBACK").unwrap();
    drop(held);
    let clock = ManualClock::new();
    Store::open_with(&clock, &home, 0, persist()).expect("not held");
    assert!(clock.naps().is_empty());
    assert_eq!(clock.elapsed(), Duration::ZERO);
}
