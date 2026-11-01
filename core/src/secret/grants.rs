//! The grant files under `$CPD_HOME/run/secret/` (contract §8.4, §1.4): directories 0700, files 0600, written
//! atomically. They hold names, never values.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, SessionId};

/// How long an approval lasts (contract §8.4 Constants).
pub const APPROVAL_LIFETIME: Duration = Duration::minutes(10);

/// One entry of `allow/<attempt>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allowed {
    pub vault: String,
    pub item: String,
    pub field: String,
}

/// `approve/<uuid>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub session: String,
    pub action: String,
    pub vault: String,
    pub item: Option<String>,
    pub field: Option<String>,
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

/// `dir` and its missing parents; `dir` and everything below `$CPD_HOME/run/secret` made 0700.
fn private_dir(home: &CpdHome, dir: &Path) -> Result<(), CpdError> {
    fs::create_dir_all(home.run_dir()).map_err(|e| io(&home.run_dir(), "create", e))?;
    for d in [home.secret_dir(), dir.to_path_buf()] {
        match DirBuilder::new().mode(0o700).create(&d) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io(&d, "create", e)),
        }
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700))
            .map_err(|e| io(&d, "protect", e))?;
    }
    Ok(())
}

/// Writes `bytes` to `dir/name` atomically: a temp file of mode 0600 from creation, then a rename.
fn write_private(home: &CpdHome, dir: &Path, name: &str, bytes: &[u8]) -> Result<(), CpdError> {
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
    })
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

/// The Attempt's entries; none when it has no file.
fn allowed(home: &CpdHome, attempt: &AttemptId) -> Result<Vec<Allowed>, CpdError> {
    let path = home.secret_allow_dir().join(format!("{attempt}.json"));
    Ok(read_json(&path)?.unwrap_or_default())
}

/// Adds `entry` to the Attempt's allowance; `true` when it was already listed (nothing is written then).
pub fn allow(home: &CpdHome, attempt: &AttemptId, entry: Allowed) -> Result<bool, CpdError> {
    let mut held = allowed(home, attempt)?;
    if held.contains(&entry) {
        return Ok(true);
    }
    held.push(entry);
    let bytes = serde_json::to_vec(&held).expect("entries serialize");
    write_private(
        home,
        &home.secret_allow_dir(),
        &format!("{attempt}.json"),
        &bytes,
    )?;
    Ok(false)
}

/// Whether the Attempt is running or was never ended: it exists and has not ended (the store says).
pub fn attempt_live(conn: &Connection, attempt: &AttemptId) -> Result<bool, CpdError> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM attempts WHERE id = ?1",
            [attempt.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(status.is_some_and(|s| s != "ended"))
}

/// Whether the Attempt holds an allowance for this vault, item and field, and is still live.
pub fn allowance_held(
    home: &CpdHome,
    conn: &Connection,
    attempt: &AttemptId,
    entry: &Allowed,
) -> Result<bool, CpdError> {
    Ok(attempt_live(conn, attempt)? && allowed(home, attempt)?.contains(entry))
}

/// Writes a new approval and returns when it expires.
pub fn approve(home: &CpdHome, approval: &Approval) -> Result<OffsetDateTime, CpdError> {
    let at = OffsetDateTime::parse(&approval.at, &Rfc3339).expect("`at` is written by cpd");
    let bytes = serde_json::to_vec(approval).expect("an approval serializes");
    write_private(
        home,
        &home.secret_approve_dir(),
        &format!("{}.json", SessionId::new_v4()),
        &bytes,
    )?;
    Ok(at + APPROVAL_LIFETIME)
}

/// [`approve`] with `at` the time of `ports.clock.utc()`: the approval `want` (its `at` ignored), and when it expires.
pub(crate) fn approve_on(
    ports: &crate::ports::Ports,
    home: &CpdHome,
    want: &Approval,
) -> Result<OffsetDateTime, CpdError> {
    approve(
        home,
        &Approval {
            at: crate::clock::timestamp(ports.clock.utc()),
            ..want.clone()
        },
    )
}

/// [`take_approval`] at `ports.clock.utc()`.
pub(crate) fn take_approval_on(
    ports: &crate::ports::Ports,
    home: &CpdHome,
    want: &Approval,
) -> Result<bool, CpdError> {
    take_approval(home, want, ports.clock.utc())
}

/// Consumes the first approval equal to `want` (its `at` aside) that has not expired: removes it and says so.
/// Expired files are removed and ignored.
pub fn take_approval(
    home: &CpdHome,
    want: &Approval,
    now: OffsetDateTime,
) -> Result<bool, CpdError> {
    let dir = home.secret_approve_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
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
        let expired = now >= at + APPROVAL_LIFETIME;
        let same = Approval {
            at: String::new(),
            ..held.clone()
        } == Approval {
            at: String::new(),
            ..want.clone()
        };
        if expired || same {
            fs::remove_file(&path).map_err(|e| io(&path, "remove", e))?;
        }
        if same && !expired {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
    }

    fn approval(at: OffsetDateTime) -> Approval {
        Approval {
            session: "s".into(),
            action: "get".into(),
            vault: "other".into(),
            item: Some("i".into()),
            field: Some("f".into()),
            at: crate::clock::timestamp(at),
        }
    }

    #[test]
    fn an_approval_is_consumed_once_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        let now = OffsetDateTime::now_utc();
        approve(&home, &approval(now)).unwrap();
        approve(&home, &approval(now - Duration::minutes(11))).unwrap();
        assert!(take_approval(&home, &approval(now), now).unwrap());
        assert!(!take_approval(&home, &approval(now), now).unwrap());
        assert_eq!(
            fs::read_dir(home.secret_approve_dir()).unwrap().count(),
            0,
            "the expired one is removed too"
        );
    }
}
