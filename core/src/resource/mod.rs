//! Repo-declared exclusive resources: the lock files and the holder record (contract §1.4, §8.3 "The `resources`
//! domain") and the commands `cpd resource list` and `cpd resource run` (commands.md §29).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, PlanId, RepoName};
use crate::lock;

pub mod cmd;
pub use cmd::{Command, run};

#[cfg(test)]
mod tests;

/// The holder record of a held resource (contract §1.4): what the taker wrote into the lock file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "lowercase")]
pub enum Holder {
    /// The starter of an Attempt whose Job lists the resource.
    Attempt { plan: String, job: String, attempt: String, at: String },
    /// `cpd resource run`.
    Command { pid: u32, command: String, session: Option<String>, attempt: Option<String>, at: String },
    /// A held lock whose record is empty or does not parse.
    Unknown,
}

/// The resources taken by one [`take`]: each lock stays held until this is dropped (or the descriptor is handed on).
#[derive(Debug)]
pub struct Held {
    files: Vec<(String, File)>,
}

impl Held {
    /// The names held, in name order.
    pub fn names(&self) -> Vec<&str> {
        self.files.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// The descriptor of the lock of `name`, if it is one of these.
    pub fn fd(&self, name: &str) -> Option<RawFd> {
        self.files.iter().find(|(n, _)| n == name).map(|(_, f)| f.as_raw_fd())
    }
}

/// The outcome of [`take`]: all of the names, or none and the first one found held.
#[derive(Debug)]
pub enum Take {
    Held(Held),
    Busy { resource: String, holder: Holder },
}

/// `locks/resource-<owner>--<repo>--<name>.lock`.
pub fn lock_path(home: &CpdHome, owner: &OwnerLogin, repo: &RepoName, name: &str) -> PathBuf {
    home.locks_dir().join(format!("resource-{owner}--{repo}--{name}.lock"))
}

fn lock_failed(path: &std::path::Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the resource lock {}: {e}.", path.display()))
        .with("lock", path.display().to_string())
}

/// The holder a record in a held lock's file names; [`Holder::Unknown`] when it is empty or does not parse.
fn parse(text: &str) -> Holder {
    serde_json::from_str(text).unwrap_or(Holder::Unknown)
}

/// Takes every resource of `names` (taken in name order, all or none) by a non-blocking exclusive try-lock, then
/// writes `holder` into each lock file.
pub fn take(home: &CpdHome, owner: &OwnerLogin, repo: &RepoName, names: &[&str], holder: &Holder) -> Result<Take, CpdError> {
    let mut sorted: Vec<&str> = names.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    fs::create_dir_all(home.locks_dir()).map_err(|e| lock_failed(&home.locks_dir(), e))?;
    let mut files = Vec::new();
    for name in sorted {
        let path = lock_path(home, owner, repo, name);
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| lock_failed(&path, e))?;
        if lock::try_lock(&file).map_err(|e| lock_failed(&path, e))? {
            files.push((name.to_owned(), file));
        } else {
            let text = fs::read_to_string(&path).unwrap_or_default();
            return Ok(Take::Busy { resource: name.to_owned(), holder: parse(&text) });
        }
    }
    let record = serde_json::to_string(holder).expect("a holder serialises");
    for (name, file) in &mut files {
        let path = lock_path(home, owner, repo, name);
        file.set_len(0).and_then(|()| file.write_all(record.as_bytes())).map_err(|e| lock_failed(&path, e))?;
    }
    Ok(Take::Held(Held { files }))
}

/// The resources a Job's Spec lists and the repo they belong to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    /// The Job's PR.
    pub pr: String,
    pub owner: OwnerLogin,
    pub repo: RepoName,
    /// The names, sorted.
    pub names: Vec<String>,
}

/// The resources Job `key` of `plan` lists in its Spec, with its PR's repo; `None` when it lists none.
pub fn wanted(conn: &rusqlite::Connection, plan: &PlanId, key: &str) -> Result<Option<Wanted>, CpdError> {
    let (pr, listed, owner, repo): (String, Option<String>, Option<String>, Option<String>) = conn.query_row(
        "SELECT j.pr, j.resources, c.owner, r.name FROM jobs j JOIN prs r ON r.plan = j.plan AND r.key = j.pr
           JOIN plans p ON p.id = j.plan JOIN contexts c ON c.id = p.context WHERE j.plan = ?1 AND j.key = ?2",
        [plan.as_str(), key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let Some(listed) = listed else { return Ok(None) };
    let mut names: Vec<String> =
        serde_json::from_str(&listed).map_err(|_| CpdError::bad_row(format!("Job {key} of Plan {plan} has resources {listed:?}")))?;
    names.sort();
    let (Some(owner), Some(repo)) = (owner, repo) else {
        return Err(CpdError::bad_row(format!("Job {key} of Plan {plan} lists resources but its PR {pr} has no repo")));
    };
    Ok(Some(Wanted { pr, owner: OwnerLogin::parse(&owner)?, repo: RepoName::parse(&repo)?, names }))
}

/// The holder of the resource: `None` when its lock is free (whatever the file still holds), else its record.
pub fn read(home: &CpdHome, owner: &OwnerLogin, repo: &RepoName, name: &str) -> Result<Option<Holder>, CpdError> {
    let path = lock_path(home, owner, repo, name);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(lock_failed(&path, e)),
    };
    // A shared try-lock, released at once (dropped): refused only while an exclusive one is held.
    if lock::shared(&file).map_err(|e| lock_failed(&path, e))? {
        return Ok(None);
    }
    Ok(Some(parse(&fs::read_to_string(&path).unwrap_or_default())))
}
