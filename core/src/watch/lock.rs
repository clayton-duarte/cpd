//! `locks/watch.lock` (contract §1.4, §3.7): held by the one `cpd watch`
//! for its whole life. A `flock(2)` lock, so the kernel releases it when the
//! engine dies and a free lock always means no engine.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::lock;

/// The engine's lock, held while this value lives.
pub(super) struct WatchLock {
    _file: File,
}

fn failed(path: &Path, what: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the engine's lock {}: {what}.", path.display()))
        .with("lock", path.display().to_string())
}

fn open(path: &Path) -> Result<File, CpdError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(path, e))?;
    }
    // Close-on-exec (std's default), so no child of the engine keeps it alive.
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| failed(path, e))
}

/// Tries the exclusive lock on a fresh descriptor; `None` when held.
fn try_take(path: &Path) -> Result<Option<File>, CpdError> {
    let file = open(path)?;
    let taken = lock::try_lock(&file).map_err(|e| failed(path, format!("flock failed: {e}")))?;
    Ok(taken.then_some(file))
}

impl WatchLock {
    /// Takes the lock for the engine's life; `watch_running` (exit 3) when
    /// another engine holds it.
    pub(super) fn take(home: &CpdHome) -> Result<WatchLock, CpdError> {
        let path = home.watch_lock();
        match try_take(&path)? {
            Some(file) => Ok(WatchLock { _file: file }),
            None => Err(CpdError::new(ErrorCode::WatchRunning, "The engine is already running on this machine: there is one per machine.")
                .with("lock", path.display().to_string())),
        }
    }
}

/// Whether an engine holds the lock (commands.md §12 `--ensure`): probes it
/// and lets go at once.
pub(super) fn held(home: &CpdHome) -> Result<bool, CpdError> {
    Ok(try_take(&home.watch_lock())?.is_none())
}
