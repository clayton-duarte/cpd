//! Probing an Attempt's driver lock (§3.4). It is a `flock(2)` lock: the
//! kernel releases it when the holder dies, so a free lock always means a
//! dead holder.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};

/// The driver lock of an Attempt, taken because it was free: its driver is
/// dead (§3.4). Held while R ends that Attempt; released on drop.
pub struct DriverLock {
    _flock: Flock<File>,
}

/// Probes the driver lock as E2 does (§3.4): opens the file afresh and tries
/// `flock(LOCK_EX | LOCK_NB)`. `Some` when it was free (the driver died),
/// `None` when a live driver holds it.
pub fn take_free_driver_lock(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Result<Option<DriverLock>, CpdError> {
    let (file, path) = open(&home.attempt_lock(plan, attempt))?;
    Ok(lock(file, &path, FlockArg::LockExclusiveNonblock)?.map(|flock| DriverLock { _flock: flock }))
}

fn open(path: &Path) -> Result<(File, PathBuf), CpdError> {
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot open the lock file {}: {e}.", path.display()))
            .with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(failed)?;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(failed)?;
    Ok((file, path.to_owned()))
}

/// `None` when a non-blocking lock is held elsewhere.
fn lock(mut file: File, path: &Path, arg: FlockArg) -> Result<Option<Flock<File>>, CpdError> {
    loop {
        match Flock::lock(file, arg) {
            Ok(flock) => return Ok(Some(flock)),
            Err((_, Errno::EWOULDBLOCK)) if arg == FlockArg::LockExclusiveNonblock => return Ok(None),
            Err((back, Errno::EINTR)) => file = back,
            Err((_, errno)) => {
                return Err(CpdError::new(ErrorCode::StoreFailed, format!("flock on {} failed: {errno}.", path.display()))
                    .with("lock", path.display().to_string()));
            }
        }
    }
}
