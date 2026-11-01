//! The store's setup lock, `locks/store.lock` (contract §1.4, §4.2) and the
//! marker its holder writes into it while it migrates (hs-n278).
//!
//! A waiter keeps waiting while the holder's marker says a live process
//! migrates, up to [`MIGRATION_WAIT_CEILING`]; for any other holder it waits
//! at most `store_busy_ms`, so a holder that hangs does not hang every cpd.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::spawn::Pid;
use crate::lock as flock;
use crate::ports::Ports;

/// The longest a waiter waits for a holder that says it migrates, counted from
/// the marker's `started_unix_ms` (contract §4.2). Not a config key.
pub const MIGRATION_WAIT_CEILING: Duration = Duration::from_secs(10 * 60);

/// The least a waiter that waited out a migration waits for SQLite in its own setup, whatever its `store_busy_ms`:
/// the migrating holder is still finishing, e.g. closing its connection and so checkpointing the migration's WAL
/// (contract §4.2). Not a config key.
pub(super) const MIGRATION_SETTLE_MS: u64 = 5000;

/// How often a waiter retries a held lock.
const RETRY: Duration = Duration::from_millis(20);

/// What the holder of `locks/store.lock` writes into it while it migrates
/// (contract §1.4, "The migration marker").
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Marker {
    migrating: bool,
    pid: u32,
    started_unix_ms: u64,
    /// The count of applied migrations before and after.
    from: usize,
    to: usize,
    command: String,
}

impl Marker {
    /// Whether the process that wrote it is still there. A holder that died
    /// released the `flock` by itself; a marker of a dead pid under a lock
    /// someone else holds is not a migration.
    fn live(&self) -> bool {
        self.migrating && Ports::current().spawn.alive(Pid(self.pid))
    }

    fn migrated_for(&self) -> Duration {
        Duration::from_millis(unix_ms().saturating_sub(self.started_unix_ms))
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The held setup lock. Dropping it releases the `flock` and, first, empties
/// the file: the marker stays until the holder's whole setup is over and goes
/// as the last step before the release, however the setup ended, so a waiter
/// that saw it never finds the lock held with no marker (§1.4).
pub(super) struct SetupLock {
    file: File,
    path: PathBuf,
    /// Whether this wait read a live migration marker.
    waited: bool,
}

impl SetupLock {
    /// Whether the wait for this lock read a live migration marker.
    pub(super) fn waited_for_migration(&self) -> bool {
        self.waited
    }

    /// Writes the marker: this process migrates the store from `from` to `to`
    /// applied migrations. One `write`, under the `flock`.
    pub(super) fn migrating(&self, from: usize, to: usize) -> Result<(), CpdError> {
        let marker = Marker {
            migrating: true,
            pid: std::process::id(),
            started_unix_ms: unix_ms(),
            from,
            to,
            command: std::env::args_os()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" "),
        };
        let mut text = serde_json::to_string(&marker).expect("a marker serialises");
        text.push('\n');
        self.file
            .set_len(0)
            .and_then(|()| (&self.file).write_all(text.as_bytes()))
            .map_err(|e| failed("write", &self.path, e))
    }
}

impl Drop for SetupLock {
    fn drop(&mut self) {
        // The last step; `file` closes, and the `flock` goes, right after.
        let _ = self.file.set_len(0);
    }
}

fn failed(what: &str, path: &Path, e: std::io::Error) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("Cannot {what} {}: {e}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// Takes `locks/store.lock` (§1.4), creating `$CPD_HOME` and `locks/` on first
/// use. Waits for a holder that is not migrating at most `busy_ms` (the
/// config's `store_busy_ms`) and for one that migrates at most
/// [`MIGRATION_WAIT_CEILING`], also after the marker is gone while the `flock`
/// stays held (the holder is in the last steps of its setup): then `lock_timeout` (exit 5) naming the lock
/// file and the time waited (§4.2). On its first wait for a migration it says
/// so, once, on stderr.
pub(super) fn setup_lock(
    clock: &dyn Clock,
    home: &CpdHome,
    busy_ms: u64,
) -> Result<SetupLock, CpdError> {
    acquire(clock, home, busy_ms, MIGRATION_WAIT_CEILING)
}

fn acquire(
    clock: &dyn Clock,
    home: &CpdHome,
    busy_ms: u64,
    ceiling: Duration,
) -> Result<SetupLock, CpdError> {
    let dir = home.locks_dir();
    fs::create_dir_all(&dir).map_err(|e| failed("create", &dir, e))?;
    // `home.rs` has no method for this lock; it is the store's alone.
    let path = dir.join("store.lock");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| failed("open", &path, e))?;
    let busy = Duration::from_millis(busy_ms);
    let mut deadline = clock.now() + busy;
    let mut said = false;
    // The last live marker this wait read: the migrating holder, while it still holds the `flock`.
    let mut seen: Option<Marker> = None;
    loop {
        if flock::try_lock(&file).map_err(|e| failed("lock", &path, e))? {
            let lock = SetupLock {
                file,
                path,
                waited: said,
            };
            // A marker left by a holder that died is not ours to keep.
            lock.file
                .set_len(0)
                .map_err(|e| failed("clear", &lock.path, e))?;
            return Ok(lock);
        }
        // A marker that is not a live migration's (a dead pid, `migrating: false`) is not the holder we saw; an
        // empty or unreadable file is the same holder in the instants around its marker, or just before it
        // releases: it truncates the file as the last step before it releases the `flock` (§1.4).
        let holder = match read_marker(&path) {
            Some(marker) if marker.live() => Some(marker),
            Some(_) => None,
            None => seen.clone().filter(Marker::live),
        };
        seen = holder.clone();
        match holder {
            Some(holder) => {
                let migrated = holder.migrated_for();
                if migrated >= ceiling {
                    return Err(CpdError::new(
                        ErrorCode::LockTimeout,
                        format!(
                            "Another cpd process, pid {} ({}), has migrated the store from {} to {} applied migrations for {} s and still holds its setup lock {}; waited for it up to {} s.",
                            holder.pid,
                            holder.command,
                            holder.from,
                            holder.to,
                            migrated.as_secs(),
                            path.display(),
                            ceiling.as_secs()
                        ),
                    )
                    .with("lock", path.display().to_string())
                    .with("waited_ms", migrated.as_millis() as u64)
                    .with("pid", holder.pid)
                    .with("command", holder.command)
                    .with("from", holder.from)
                    .with("to", holder.to)
                    .with("migrating_s", migrated.as_secs()));
                }
                if !said {
                    eprintln!(
                        "waiting for cpd pid {} to migrate the store {} -> {} ({})",
                        holder.pid, holder.from, holder.to, holder.command
                    );
                    said = true;
                }
                // The holder that migrates is not the hang `busy_ms` bounds; a later one gets the whole of it.
                deadline = clock.now() + busy;
            }
            None if clock.now() >= deadline => {
                return Err(CpdError::new(
                    ErrorCode::LockTimeout,
                    format!(
                        "Another cpd process held the store's setup lock {} for longer than store_busy_ms ({busy_ms} ms).",
                        path.display()
                    ),
                )
                .with("lock", path.display().to_string())
                .with("waited_ms", busy_ms));
            }
            None => {}
        }
        clock.sleep(RETRY);
    }
}

/// The marker in the lock file; `None` for a file that is empty, unreadable or
/// not a marker (a holder that is not migrating, or one writing right now).
fn read_marker(path: &Path) -> Option<Marker> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::clock::SystemClock;

    fn home() -> (tempfile::TempDir, CpdHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        (dir, home)
    }

    fn marker(pid: u32, started_unix_ms: u64) -> String {
        serde_json::to_string(&Marker {
            migrating: true,
            pid,
            started_unix_ms,
            from: 3,
            to: 9,
            command: "cpd test".into(),
        })
        .unwrap()
    }

    /// A holder (this process, on another descriptor) with `text` in the lock file.
    fn held(home: &CpdHome, text: &str) -> File {
        fs::create_dir_all(home.locks_dir()).unwrap();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.locks_dir().join("store.lock"))
            .unwrap();
        assert!(flock::try_lock(&file).unwrap());
        (&file).write_all(text.as_bytes()).unwrap();
        file
    }

    #[test]
    fn a_migrating_holder_past_the_ceiling_is_lock_timeout_naming_it() {
        let (_dir, home) = home();
        let _holder = held(&home, &marker(std::process::id(), unix_ms() - 5_000));
        let err = acquire(&SystemClock, &home, 60_000, Duration::from_secs(1))
            .err()
            .expect("past the ceiling");
        assert_eq!(err.code(), ErrorCode::LockTimeout, "{err}");
        for part in [
            &std::process::id().to_string(),
            "cpd test",
            "3 to 9",
            "store.lock",
        ] {
            assert!(err.message().contains(part), "{part}: {err}");
        }
        assert_eq!(err.details()["pid"], std::process::id());
    }

    #[test]
    fn a_marker_of_a_dead_process_is_not_a_migration() {
        let (_dir, home) = home();
        let mut child = Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let _holder = held(&home, &marker(dead, unix_ms()));
        let started = std::time::Instant::now();
        let err = acquire(&SystemClock, &home, 200, MIGRATION_WAIT_CEILING)
            .err()
            .expect("the lock is held");
        assert_eq!(err.code(), ErrorCode::LockTimeout, "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_marker_under_a_free_lock_is_cleared_by_the_next_holder() {
        let (_dir, home) = home();
        drop(held(&home, &marker(std::process::id(), unix_ms())));
        let lock = acquire(&SystemClock, &home, 200, MIGRATION_WAIT_CEILING).unwrap();
        assert_eq!(fs::read_to_string(&lock.path).unwrap(), "");
    }

    #[test]
    fn dropping_the_lock_empties_its_marker() {
        let (_dir, home) = home();
        let lock = acquire(&SystemClock, &home, 200, MIGRATION_WAIT_CEILING).unwrap();
        lock.migrating(1, 2).unwrap();
        let written = read_marker(&lock.path).expect("a marker");
        assert!(
            written.migrating && written.live() && (written.from, written.to) == (1, 2),
            "{written:?}"
        );
        let path = lock.path.clone();
        drop(lock);
        assert_eq!(fs::read_to_string(path).unwrap(), "");
    }
}
