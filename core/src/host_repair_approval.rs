//! The approval files under `$CPD_HOME/run/host/approve/` (commands.md §9.11, hs-d618): directories
//! 0700, files 0600, written atomically. One approval authorizes one repair of the cpd checkout.
//!
//! Modeled on [`crate::secret::grants`](crate::secret): the same private dir, the same atomic write,
//! the same 10-minute lifetime. The approval holds only the Session and when it was written.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::SessionId;

/// How long an approval lasts (commands.md §9.11): ten minutes, like `cpd_secret_approve`.
pub const APPROVAL_LIFETIME: Duration = Duration::minutes(10);

/// `run/host/approve/<uuid>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub session: String,
    /// RFC 3339
    pub at: String,
}

fn io(path: &Path, what: &str, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::Internal,
        format!("Cannot {what} {}: {e}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// The directory of the approval files.
pub fn dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("host/approve")
}

/// `dir` and its missing parents; `dir` made 0700.
fn private_dir(home: &CpdHome, dir: &Path) -> Result<(), CpdError> {
    fs::create_dir_all(home.run_dir()).map_err(|e| io(&home.run_dir(), "create", e))?;
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent).map_err(|e| io(parent, "create", e))?;
    }
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io(dir, "create", e)),
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| io(dir, "protect", e))?;
    Ok(())
}

/// Writes `bytes` to `dir/name` atomically: a temp file of mode 0600 from creation, then a rename.
/// Returns the path written. `dir` is made private; nothing above it is changed.
pub(crate) fn write_private(
    home: &CpdHome,
    dir: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<PathBuf, CpdError> {
    private_dir(home, dir)?;
    let tmp = dir.join(format!(".{name}.{}.tmp", SessionId::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| io(&tmp, "create", e))?;
    let done = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let target = dir.join(name);
    done.and_then(|()| fs::rename(&tmp, &target)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(&target, "write", e)
    })?;
    Ok(target)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, CpdError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| io(path, "parse", e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, "read", e)),
    }
}

/// Writes a new approval for `approval.session` and returns when it expires.
pub fn approve(home: &CpdHome, approval: &Approval) -> Result<OffsetDateTime, CpdError> {
    let at = OffsetDateTime::parse(&approval.at, &Rfc3339).expect("`at` is written by cpd");
    let bytes = serde_json::to_vec(approval).expect("an approval serializes");
    write_private(
        home,
        &dir(home),
        &format!("{}.json", SessionId::new_v4()),
        &bytes,
    )?;
    Ok(at + APPROVAL_LIFETIME)
}

/// The first live approval of `session` (its file), expired ones removed and ignored. `None` when
/// there is none: the repair then runs no git.
pub fn live(
    home: &CpdHome,
    session: &str,
    now: OffsetDateTime,
) -> Result<Option<PathBuf>, CpdError> {
    let dir = dir(home);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(&dir, "read", e)),
    };
    let mut names: Vec<_> = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io(&dir, "read", e))?;
    names.sort_by_key(|e| e.file_name());
    for entry in names {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(held) = read_json::<Approval>(&path)? else {
            continue;
        };
        let at = OffsetDateTime::parse(&held.at, &Rfc3339).map_err(|e| io(&path, "parse", e))?;
        if now >= at + APPROVAL_LIFETIME {
            fs::remove_file(&path).map_err(|e| io(&path, "remove", e))?;
            continue;
        }
        if held.session == session {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Consumes the approval at `path`: removes it. Called only once the repair's last step succeeded.
pub fn consume(path: &Path) -> Result<(), CpdError> {
    fs::remove_file(path).map_err(|e| io(path, "remove", e))
}
