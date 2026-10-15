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

/// The driver's descriptor of its lock, checked at A1. Keep it alive for the
/// driver's whole life: dropping it closes fd 3 and releases the lock.
#[derive(Debug)]
pub struct Inherited {
    _file: File,
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
