//! The per-repo lock (contract §1.4, §7.4) and probing an Attempt's driver
//! lock (§3.4). Both are `flock(2)` locks: the kernel releases them when the
//! holder dies, so a free lock always means a dead holder.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use crate::error::{CpdError, ErrorCode};
use crate::git::run::ok;
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};

/// How often [`RepoLock::wait_for`] tries again.
const RETRY: Duration = Duration::from_millis(100);

/// The per-repo lock `locks/repo-<sha1>.lock`, held while this value lives.
/// The merge holds it from step 1 until its step-10 transaction has
/// committed; `pr open` and `pr rebase` until their push has finished.
pub struct RepoLock {
    _flock: Flock<File>,
    repo: PathBuf,
    path: PathBuf,
}

impl std::fmt::Debug for RepoLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoLock").field("repo", &self.repo).field("path", &self.path).finish()
    }
}

impl RepoLock {
    /// The lock file of `repo`: the sha1 of the realpath of
    /// `git rev-parse --git-common-dir`, through [`CpdHome::repo_lock`].
    pub fn path(home: &CpdHome, repo: &Path) -> Result<PathBuf, CpdError> {
        let out = ok(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let dir = PathBuf::from(out.trim_end_matches('\n'));
        let real = dir.canonicalize().map_err(|e| {
            CpdError::new(ErrorCode::GitFailed, format!("Cannot resolve the git common dir {}: {e}.", dir.display()))
                .with("path", dir.display().to_string())
        })?;
        Ok(home.repo_lock(&real))
    }

    /// Waits for the lock without a limit (the driver at A8).
    pub fn wait(home: &CpdHome, repo: &Path) -> Result<RepoLock, CpdError> {
        let (file, path) = open(&RepoLock::path(home, repo)?)?;
        let flock = lock(file, &path, FlockArg::LockExclusive)?.expect("a blocking flock either locks or fails");
        Ok(RepoLock { _flock: flock, repo: repo.to_owned(), path })
    }

    /// Waits at most `limit` (`lock_wait_seconds`; `pr open`, `pr rebase`),
    /// then `lock_timeout` (exit 5).
    pub fn wait_for(home: &CpdHome, repo: &Path, limit: Duration) -> Result<RepoLock, CpdError> {
        let path = RepoLock::path(home, repo)?;
        let deadline = Instant::now() + limit;
        loop {
            if let Some(lock) = RepoLock::try_path(repo, &path)? {
                return Ok(lock);
            }
            if Instant::now() >= deadline {
                return Err(CpdError::new(
                    ErrorCode::LockTimeout,
                    format!("Another cpd process held the per-repo lock for longer than {}s.", limit.as_secs()),
                )
                .with("lock", path.display().to_string())
                .with("repo", repo.display().to_string())
                .with("waited_seconds", limit.as_secs()));
            }
            std::thread::sleep(RETRY);
        }
    }

    /// Takes the lock only if it is free now (engine E2); `None` when held.
    pub fn try_take(home: &CpdHome, repo: &Path) -> Result<Option<RepoLock>, CpdError> {
        RepoLock::try_path(repo, &RepoLock::path(home, repo)?)
    }

    fn try_path(repo: &Path, path: &Path) -> Result<Option<RepoLock>, CpdError> {
        let (file, path) = open(path)?;
        Ok(lock(file, &path, FlockArg::LockExclusiveNonblock)?.map(|flock| RepoLock {
            _flock: flock,
            repo: repo.to_owned(),
            path,
        }))
    }

    /// The repo it was taken for, as the caller named it: a Plan's `repo`.
    pub fn repo(&self) -> &Path {
        &self.repo
    }

    /// The lock file.
    pub fn file(&self) -> &Path {
        &self.path
    }
}

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
