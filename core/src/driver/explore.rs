//! A2 of an explore Attempt (contract §3.4 "An explore Job's Attempt", R7): no worktree, no `pre-start`, no `base_sha`; the
//! explore directory is created if missing and is the Worker's cwd, and the Job's artefact is kept (pro-d433).

use std::path::{Path, PathBuf};

use crate::attempt::ledger::Attempt as Target;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ports::Ports;

/// The artefact of Job `job` in the explore directory `dir`: `<dir>/<job>.md`.
pub(crate) fn artefact_in(dir: &Path, job: &str) -> PathBuf {
    dir.join(format!("{job}.md"))
}

/// Prepares the start of an explore `target`: creates the explore directory, keeps any artefact of the Job there
/// (a valid file of an earlier Attempt may be submitted by the retry), and returns the Worker's cwd. Runs no git and no hook.
pub(super) fn prepare(
    _ports: &Ports,
    _home: &CpdHome,
    target: &Target,
) -> Result<PathBuf, CpdError> {
    let dir = target.worktree.clone();
    let failed = |what: &str, path: &Path, e: std::io::Error| {
        CpdError::new(
            ErrorCode::SetupFailed,
            format!("Cannot {what} {}: {e}.", path.display()),
        )
        .with("path", path.display().to_string())
    };
    std::fs::create_dir_all(&dir).map_err(|e| failed("create", &dir, e))?;
    Ok(dir)
}
