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

use std::fs::{self, File, OpenOptions};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use nix::fcntl::{FcntlArg, FdFlag, fcntl};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::lock;

/// The descriptor the driver inherits the lock on (§3.4 step 2).
pub const DRIVER_FD: RawFd = 3;

/// The environment variable that lists, comma separated, the descriptors of the resource locks the starter handed to the
/// driver besides fd 3 (§3.4 "Resources of a Job"): they stay open at the numbers they had in the starter.
pub const RESOURCE_FDS_ENV: &str = "CPD_RESOURCE_FDS";

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
        if lock::try_lock(&file).map_err(|e| lock_failed(&path, e))? {
            Ok(Starter { file })
        } else {
            Err(CpdError::new(
                ErrorCode::CasLost,
                format!("The driver lock of Attempt {attempt} is held: another starter is creating it."),
            )
            .with("plan", plan.as_str())
            .with("attempt", attempt.to_string())
            .with("lock", path.display().to_string()))
        }
    }

    /// Step 2: the descriptor a `Launch` hands over (`Launch::hand_over`): the child gets it as its fd 3, not close-on-exec,
    /// in a new session.
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
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
    let taken = lock::try_lock(&file).map_err(|e| lock_failed(path, e))?;
    Ok(taken.then_some(Free { _file: Some(file) }))
}

/// A9: removes the lock file of an Attempt that has ended; a missing file is
/// fine. Call it after the end is committed. Removing the path while the
/// driver still holds fd 3 is safe: the `flock` is on the open file
/// description, not on the path. And no one can take a lock on a fresh file
/// for an Attempt that is still running: ids are never reused, so after the
/// end nobody opens this path again (E2's [`probe`] never creates it).
pub fn remove(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<(), CpdError> {
    // The Attempt's check lock (§7.7) goes with its driver lock: nothing submits for an ended Attempt.
    for path in [home.attempt_lock(plan, attempt), home.check_lock(plan, attempt)] {
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(lock_failed(&path, e)),
            _ => {}
        }
    }
    Ok(())
}

/// Whether a check span of the Attempt is open (contract §7.7): a live `attempt submit` holds `locks/check-<plan>--<attempt>.lock`.
/// Opens the lock file without creating it; a missing file is no span. A4's stall check does not run while this is true.
pub fn check_span_open(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<bool, CpdError> {
    // `try_take` locks the file when it is free, and the guard it returns lets it go at once.
    Ok(try_take(&home.check_lock(plan, attempt), false)?.is_none())
}

/// The driver's descriptor of its lock, checked at A1. Keep it alive for the
/// driver's whole life: dropping it closes fd 3 and releases the lock.
#[derive(Debug)]
pub struct Inherited {
    _file: File,
    /// The resource locks the Attempt holds (§3.4 "Resources of a Job"): kept open for the driver's whole life.
    resources: Vec<File>,
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
        for file in std::iter::once(&self._file).chain(&self.resources) {
            fcntl(file, FcntlArg::F_SETFD(flags)).map(drop).map_err(|e| {
                CpdError::new(
                    ErrorCode::DriverLockMissing,
                    format!("Cannot change close-on-exec on a lock the driver holds (fd {}): {e}.", file.as_raw_fd()),
                )
            })?;
        }
        Ok(())
    }

    /// Takes over the resource locks `list` (the value of [`RESOURCE_FDS_ENV`]) names, and makes each close-on-exec, so
    /// nothing the driver starts keeps a resource held. An empty list is none.
    fn take_resources(&mut self, list: &str, home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<(), CpdError> {
        let missing = |why: String| {
            CpdError::new(ErrorCode::DriverLockMissing, format!("{RESOURCE_FDS_ENV} {list:?} is not the resources of Attempt {attempt}: {why}."))
                .with("plan", plan.as_str())
                .with("attempt", attempt.to_string())
                .with("lock", home.attempt_lock(plan, attempt).display().to_string())
        };
        let mut fds = Vec::new();
        for item in list.split(',').filter(|i| !i.is_empty()) {
            let fd: RawFd = item.parse().map_err(|_| missing(format!("{item:?} is not a descriptor")))?;
            // SAFETY: only probes the descriptor number; nothing is owned yet.
            fcntl(unsafe { BorrowedFd::borrow_raw(fd) }, FcntlArg::F_GETFD).map_err(|e| missing(format!("fd {fd} is not open ({e})")))?;
            fds.push(fd);
        }
        for fd in fds {
            // SAFETY: the fd is open and nothing else in this process owns it: the driver calls this before it opens any file.
            let file = unsafe { File::from_raw_fd(fd) };
            fcntl(&file, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)).map_err(|e| missing(format!("cannot set close-on-exec on fd {fd} ({e})")))?;
            self.resources.push(file);
        }
        Ok(())
    }
}

/// A1 (§3.4 step 4): fd 3 must be the lock file `locks/attempt-<plan>--<attempt>.lock`
/// (same device and inode as the path), else `driver_lock_missing` and the
/// driver exits at once without writing. Then fd 3 is made close-on-exec, so
/// nothing the driver starts (git, hooks, tmux) can keep the lock alive.
///
/// Call it before the process opens any file, so fd 3 cannot be one of its own.
pub fn inherited(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Inherited, CpdError> {
    let mut held = inherited_on(DRIVER_FD, home, plan, attempt)?;
    if let Ok(list) = std::env::var(RESOURCE_FDS_ENV) {
        held.take_resources(&list, home, plan, attempt)?;
    }
    Ok(held)
}

/// [`inherited`] for the descriptor `fd` (the driver's is [`DRIVER_FD`]): a test hands it one that is its own.
pub(crate) fn inherited_on(fd: RawFd, home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Inherited, CpdError> {
    let path = home.attempt_lock(plan, attempt);
    let missing = |why: String| {
        CpdError::new(ErrorCode::DriverLockMissing, format!("fd {fd} is not the driver lock {}: {why}.", path.display()))
            .with("plan", plan.as_str())
            .with("attempt", attempt.to_string())
            .with("lock", path.display().to_string())
    };
    // SAFETY: only probes the descriptor number; nothing is owned yet.
    let open = fcntl(unsafe { BorrowedFd::borrow_raw(fd) }, FcntlArg::F_GETFD);
    if let Err(errno) = open {
        return Err(missing(format!("it is not open ({errno})")));
    }
    // SAFETY: fd 3 is open and nothing else in this process owns it: the
    // driver calls this before it opens any file.
    let file = unsafe { File::from_raw_fd(fd) };
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
    Ok(Inherited { _file: file, resources: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::IntoRawFd;

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

    /// The flags of the descriptor of `held`.
    fn flags(held: &Inherited) -> FdFlag {
        FdFlag::from_bits_truncate(fcntl(&held._file, FcntlArg::F_GETFD).unwrap())
    }

    /// The lock file of the Attempt, created, opened, with close-on-exec cleared (as a spawn leaves it on fd 3).
    fn lock_file(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> File {
        let path = home.attempt_lock(plan, attempt);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(&path).unwrap();
        fcntl(&file, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
        file
    }

    #[test]
    fn keeping_the_lock_across_exec_clears_close_on_exec_and_closing_it_on_exec_sets_it_again() {
        let (_dir, home, plan, attempt) = home();
        let held = Inherited { _file: lock_file(&home, &plan, &attempt), resources: Vec::new() };
        assert!(!flags(&held).contains(FdFlag::FD_CLOEXEC));

        held.close_on_exec().unwrap();
        assert!(flags(&held).contains(FdFlag::FD_CLOEXEC));

        held.keep_across_exec().unwrap();
        assert!(!flags(&held).contains(FdFlag::FD_CLOEXEC));
    }

    #[test]
    fn the_inherited_lock_is_made_close_on_exec_so_the_drivers_children_never_hold_it() {
        let (_dir, home, plan, attempt) = home();
        let fd = lock_file(&home, &plan, &attempt).into_raw_fd();

        let held = inherited_on(fd, &home, &plan, &attempt).unwrap();

        assert_eq!(held._file.as_raw_fd(), fd);
        assert!(flags(&held).contains(FdFlag::FD_CLOEXEC));
    }

    #[test]
    fn a_descriptor_that_is_another_file_than_the_lock_is_refused_and_left_open() {
        let (dir, home, plan, attempt) = home();
        lock_file(&home, &plan, &attempt);
        let other = File::create(dir.path().join("other.lock")).unwrap();
        let fd = other.into_raw_fd();

        let refused = inherited_on(fd, &home, &plan, &attempt).unwrap_err();

        assert_eq!(refused.code(), ErrorCode::DriverLockMissing);
        assert!(refused.message().contains("another file"), "{}", refused.message());
        // Not ours to close: the refusal leaves it open (the test closes it here).
        assert!(fcntl(unsafe { BorrowedFd::borrow_raw(fd) }, FcntlArg::F_GETFD).is_ok());
        drop(unsafe { File::from_raw_fd(fd) });
    }

    #[test]
    fn a_descriptor_that_is_not_open_is_refused() {
        let (_dir, home, plan, attempt) = home();
        lock_file(&home, &plan, &attempt);

        let refused = inherited_on(9999, &home, &plan, &attempt).unwrap_err();

        assert_eq!(refused.code(), ErrorCode::DriverLockMissing);
        assert!(refused.message().contains("not open"), "{}", refused.message());
    }
}
