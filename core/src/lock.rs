//! The one place cpd takes a `flock(2)` lock (contract §1.4).
//!
//! Every lock file of cpd goes through [`blocking`], [`try_lock`] or
//! [`shared`]: they take the lock on an open [`File`] and retry on `EINTR`.
//! Nothing here unlocks. The lock belongs to the open file description, so
//! dropping the last descriptor of it releases the lock, and the kernel does
//! so when the holder dies. An explicit unlock on any copy of a descriptor
//! would release it for all of them (flock(2)), which the driver lock relies
//! on not happening (`driver::lock`).

use std::fs::{File, TryLockError};
use std::io::{self, ErrorKind};

/// Waits for an exclusive lock on `file`, without a limit.
pub fn blocking(file: &File) -> io::Result<()> {
    loop {
        match file.lock() {
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            other => return other,
        }
    }
}

/// Takes an exclusive lock on `file` if it is free now: `false` when another
/// open file description holds it.
pub fn try_lock(file: &File) -> io::Result<bool> {
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(e)) if e.kind() == ErrorKind::Interrupted => {}
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// Takes a shared lock on `file` if no exclusive one is held: `false` when one
/// is. Shared locks never conflict with each other.
pub fn shared(file: &File) -> io::Result<bool> {
    loop {
        match file.try_lock_shared() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(e)) if e.kind() == ErrorKind::Interrupted => {}
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (tempfile::TempDir, File, File) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.lock");
        let a = File::options().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
        let b = File::options().read(true).write(true).open(&path).unwrap();
        (dir, a, b)
    }

    #[test]
    fn an_exclusive_lock_is_refused_while_another_description_holds_it() {
        let (_dir, a, b) = pair();
        assert!(try_lock(&a).unwrap());
        assert!(!try_lock(&b).unwrap());
        assert!(!shared(&b).unwrap());
        drop(a);
        assert!(try_lock(&b).unwrap());
    }

    #[test]
    fn shared_locks_do_not_conflict_but_stop_an_exclusive_one() {
        let (_dir, a, b) = pair();
        assert!(shared(&a).unwrap());
        assert!(shared(&b).unwrap());
        let c = File::options().read(true).write(true).open(_dir.path().join("x.lock")).unwrap();
        assert!(!try_lock(&c).unwrap());
    }

    #[test]
    fn blocking_waits_until_the_holder_lets_go() {
        let (_dir, a, b) = pair();
        assert!(try_lock(&a).unwrap());
        let waiter = std::thread::spawn(move || blocking(&b).map(|()| b));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!waiter.is_finished());
        drop(a);
        waiter.join().unwrap().unwrap();
    }

    #[test]
    fn dropping_a_copy_of_a_descriptor_keeps_the_lock() {
        let (_dir, a, b) = pair();
        assert!(try_lock(&a).unwrap());
        drop(a.try_clone().unwrap());
        assert!(!try_lock(&b).unwrap());
    }
}
