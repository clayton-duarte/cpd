//! The driver lock `locks/attempt-<plan>--<attempt>.lock` (contract §3.4):
//! taken by the starter before the Attempt row exists, handed to the driver
//! as fd 3 at the spawn, and checked by the driver at A1. Held without a gap
//! from before the insert until the driver exits, so a free lock always means
//! a dead driver.
//!
//! The lock is the `flock(2)` lock of one open file description. Closing a
//! descriptor never releases it while another descriptor of the same
//! description is open; an explicit unlock on any copy would release it for
//! all (docs: flock(2)). So nothing here ever unlocks: `std::fs::File`'s
//! `try_lock` takes it and dropping a `File` only closes.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, fcntl};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};

/// The descriptor the driver inherits the lock on (§3.4 step 2).
pub const DRIVER_FD: RawFd = 3;

/// The starter's descriptor of a driver lock it holds (§3.4 steps 1–3).
/// Dropping it closes the descriptor without unlocking: once a driver
/// inherited it, the driver holds the lock alone.
#[derive(Debug)]
pub struct Starter {
    file: File,
}

fn lock_failed(path: &Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the driver lock {}: {e}.", path.display()))
        .with("lock", path.display().to_string())
}

impl Starter {
    /// Step 1: opens the lock file and takes `flock(LOCK_EX | LOCK_NB)`.
    /// Held: another starter is creating the same Attempt, `cas_lost`.
    pub fn take(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Starter, CpdError> {
        let path = home.attempt_lock(plan, attempt);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| lock_failed(&path, e))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| lock_failed(&path, e))?;
        match file.try_lock() {
            Ok(()) => Ok(Starter { file }),
            Err(TryLockError::WouldBlock) => Err(CpdError::new(
                ErrorCode::CasLost,
                format!("The driver lock of Attempt {attempt} is held: another starter is creating it."),
            )
            .with("plan", plan.as_str())
            .with("attempt", attempt.to_string())
            .with("lock", path.display().to_string())),
            Err(TryLockError::Error(e)) => Err(lock_failed(&path, e)),
        }
    }

    /// Step 2: makes `command` start in a new session (`setsid`) with this
    /// descriptor as its fd 3, not close-on-exec.
    pub fn hand_over(&self, command: &mut Command) {
        let fd = self.file.as_raw_fd();
        // SAFETY: the closure runs in the forked child before exec and only
        // makes async-signal-safe system calls; it allocates nothing.
        unsafe {
            command.pre_exec(move || {
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                if fd != DRIVER_FD {
                    Errno::result(nix::libc::dup2(fd, DRIVER_FD)).map_err(std::io::Error::from)?;
                }
                // dup2 clears close-on-exec; when fd already was 3 it did not run.
                fcntl(BorrowedFd::borrow_raw(DRIVER_FD), FcntlArg::F_SETFD(FdFlag::empty()))
                    .map_err(std::io::Error::from)?;
                Ok(())
            });
        }
    }
}

/// A driver lock found free by [`probe`]: its driver is dead. Holds the lock
/// (when the file exists) until it is dropped.
#[derive(Debug)]
pub struct Free {
    _file: Option<File>,
}

/// E2's probe (§3.4): whether the driver of an Attempt is dead. Opens the lock
/// file afresh **without creating it**: a missing file means no driver (the
/// starter creates it before the Attempt row exists and [`remove`] deletes it
/// only after the end is committed). `Some` when the lock was free or missing,
/// `None` when a live driver holds it.
pub fn probe(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Option<Free>, CpdError> {
    try_take(&home.attempt_lock(plan, attempt), false)
}

/// [`probe`] for a caller that means to hold the lock: it **creates** the file
/// when it is missing, as a starter does. Not for E2, R or `pr open`: a file
/// created for an Attempt that has none is never removed.
pub fn take_free_driver_lock(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Option<Free>, CpdError> {
    let path = home.attempt_lock(plan, attempt);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| lock_failed(&path, e))?;
    }
    try_take(&path, true)
}

fn try_take(path: &Path, create: bool) -> Result<Option<Free>, CpdError> {
    let file = match OpenOptions::new().read(true).write(true).create(create).truncate(false).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some(Free { _file: None })),
        Err(e) => return Err(lock_failed(path, e)),
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(Free { _file: Some(file) })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(lock_failed(path, e)),
    }
}

/// A9: removes the lock file of an Attempt that has ended; a missing file is
/// fine. Call it after the end is committed. Removing the path while the
/// driver still holds fd 3 is safe: the `flock` is on the open file
/// description, not on the path. And no one can take a lock on a fresh file
/// for an Attempt that is still running: ids are never reused, so after the
/// end nobody opens this path again (E2's [`probe`] never creates it).
pub fn remove(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<(), CpdError> {
    let path = home.attempt_lock(plan, attempt);
    match fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(lock_failed(&path, e)),
        _ => Ok(()),
    }
}

/// The driver's descriptor of its lock, checked at A1. Keep it alive for the
/// driver's whole life: dropping it closes fd 3 and releases the lock.
#[derive(Debug)]
pub struct Inherited {
    _file: File,
}

impl Inherited {
    /// Clears close-on-exec on fd 3, just before the driver re-executes itself
    /// after an install (§3.4 "Reload on install"): the lock stays held, with
    /// no gap, by the new process.
    pub fn keep_across_exec(&self) -> Result<(), CpdError> {
        self.set_flags(FdFlag::empty())
    }

    /// Sets close-on-exec on fd 3 again: the re-exec failed and the driver goes on.
    pub fn close_on_exec(&self) -> Result<(), CpdError> {
        self.set_flags(FdFlag::FD_CLOEXEC)
    }

    fn set_flags(&self, flags: FdFlag) -> Result<(), CpdError> {
        fcntl(&self._file, FcntlArg::F_SETFD(flags)).map(drop).map_err(|e| {
            CpdError::new(ErrorCode::DriverLockMissing, format!("Cannot change close-on-exec on the driver lock (fd 3): {e}."))
        })
    }
}

/// A1 (§3.4 step 4): fd 3 must be the lock file `locks/attempt-<plan>--<attempt>.lock`
/// (same device and inode as the path), else `driver_lock_missing` and the
/// driver exits at once without writing. Then fd 3 is made close-on-exec, so
/// nothing the driver starts (git, hooks, tmux) can keep the lock alive.
///
/// Call it before the process opens any file, so fd 3 cannot be one of its own.
pub fn inherited(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Inherited, CpdError> {
    let path = home.attempt_lock(plan, attempt);
    let missing = |why: String| {
        CpdError::new(ErrorCode::DriverLockMissing, format!("fd 3 is not the driver lock {}: {why}.", path.display()))
            .with("plan", plan.as_str())
            .with("attempt", attempt.to_string())
            .with("lock", path.display().to_string())
    };
    // SAFETY: only probes the descriptor number; nothing is owned yet.
    let open = fcntl(unsafe { BorrowedFd::borrow_raw(DRIVER_FD) }, FcntlArg::F_GETFD);
    if let Err(errno) = open {
        return Err(missing(format!("it is not open ({errno})")));
    }
    // SAFETY: fd 3 is open and nothing else in this process owns it: the
    // driver calls this before it opens any file.
    let file = unsafe { File::from_raw_fd(DRIVER_FD) };
    let same = match (file.metadata(), fs::metadata(&path)) {
        (Ok(fd), Ok(on_disk)) => {
            if fd.dev() == on_disk.dev() && fd.ino() == on_disk.ino() {
                Ok(())
            } else {
                Err("it is another file".to_owned())
            }
        }
        (Err(e), _) => Err(format!("it cannot be read ({e})")),
        (_, Err(e)) => Err(format!("the lock file cannot be read ({e})")),
    };
    if let Err(why) = same {
        // Not ours to close: leave whatever fd 3 is alone.
        std::mem::forget(file);
        return Err(missing(why));
    }
    fcntl(&file, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)).map_err(|e| missing(format!("cannot set close-on-exec ({e})")))?;
    Ok(Inherited { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, CpdHome, PlanId, AttemptId) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        (dir, home, PlanId::parse("pl").unwrap(), AttemptId::parse("api-a1").unwrap())
    }

    /// engine-G1: E2, R and `pr open` probe the driver lock of an Attempt that
    /// has none (its file is gone): that must not leave a lock file behind.
    #[test]
    fn probing_a_missing_driver_lock_creates_no_file() {
        let (_dir, home, plan, attempt) = home();

        assert!(probe(&home, &plan, &attempt).unwrap().is_some());

        assert!(!home.attempt_lock(&plan, &attempt).exists());
    }

    #[test]
    fn taking_a_missing_driver_lock_holds_it_until_dropped() {
        let (_dir, home, plan, attempt) = home();

        let held = take_free_driver_lock(&home, &plan, &attempt).unwrap().expect("free");

        assert!(probe(&home, &plan, &attempt).unwrap().is_none());
        drop(held);
        assert!(probe(&home, &plan, &attempt).unwrap().is_some());
    }
}
