//! Repo-declared exclusive resources: the lock files and the holder record (contract §1.4, §8.3 "The `resources`
//! domain") and the commands `cpd resource list` and `cpd resource run` (commands.md §29).

use std::fs::File;
use std::os::fd::RawFd;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, RepoName};

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
    pub fn fd(&self, _name: &str) -> Option<RawFd> {
        None
    }
}

/// The outcome of [`take`]: all of the names, or none and the first one found held.
#[derive(Debug)]
pub enum Take {
    Held(Held),
    Busy { resource: String, holder: Holder },
}

/// `locks/resource-<owner>--<repo>--<name>.lock`.
pub fn lock_path(home: &CpdHome, _owner: &OwnerLogin, _repo: &RepoName, _name: &str) -> PathBuf {
    home.locks_dir().join("resource-unimplemented.lock")
}

/// Takes every resource of `names` (taken in name order, all or none) by a non-blocking exclusive try-lock, then
/// writes `holder` into each lock file.
pub fn take(_home: &CpdHome, _owner: &OwnerLogin, _repo: &RepoName, names: &[&str], _holder: &Holder) -> Result<Take, CpdError> {
    Ok(Take::Busy { resource: names.first().copied().unwrap_or_default().to_owned(), holder: Holder::Unknown })
}

/// The holder of the resource: `None` when its lock is free (whatever the file still holds), else its record.
pub fn read(_home: &CpdHome, _owner: &OwnerLogin, _repo: &RepoName, _name: &str) -> Result<Option<Holder>, CpdError> {
    Ok(None)
}
