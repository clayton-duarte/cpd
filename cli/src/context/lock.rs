//! The Context lock: `locks/context-<id>.lock`, held while writing or syncing
//! a Context store (contract §1.4, §4.6). A `flock(2)` lock: the kernel
//! releases it when the holder dies.

use std::fs::{self, File, OpenOptions};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use super::io_error;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::ContextId;

/// The held lock; dropping it releases it.
pub struct ContextLock {
    _lock: Flock<File>,
}

/// Waits for the lock without a limit: a holder is another cpd2 process
/// doing a bounded amount of work (contract §4.6 gives no timeout).
pub fn acquire(home: &CpdHome, context: &ContextId) -> Result<ContextLock, CpdError> {
    let path = home.context_lock(context);
    fs::create_dir_all(home.locks_dir()).map_err(|e| io_error("create", &home.locks_dir(), &e))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| io_error("open", &path, &e))?;
    loop {
        match Flock::lock(file, FlockArg::LockExclusive) {
            Ok(lock) => return Ok(ContextLock { _lock: lock }),
            Err((f, Errno::EINTR)) => file = f,
            Err((_, errno)) => return Err(io_error("lock", &path, &std::io::Error::from(errno))),
        }
    }
}
