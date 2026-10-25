//! The fixture handle of a test: a repo of the Universe and its working tree on disk.
//!
//! STUB: every body is `todo!()`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::universe::{Oid, RepoId, Universe};

/// A repo's handle; a clone shares the Universe.
#[derive(Clone)]
pub struct Repo {
    _universe: Arc<Mutex<Universe>>,
    _id: RepoId,
    _dir: PathBuf,
}

impl Repo {
    /// A non-bare repo at `dir` on the unborn branch `main`.
    pub fn init(_universe: &Arc<Mutex<Universe>>, _dir: &Path) -> Repo {
        todo!("Repo::init")
    }
    pub fn dir(&self) -> &Path {
        todo!("Repo::dir")
    }
    pub fn id(&self) -> RepoId {
        todo!("Repo::id")
    }
    /// Writes a real file of the working tree (parents made).
    pub fn write(&self, _path: &str, _text: &str) {
        todo!("Repo::write")
    }
    /// Stages the file as it is on disk (`git add <path>`).
    pub fn add(&self, _path: &str) {
        todo!("Repo::add")
    }
    /// Stages every file of the working tree, commits it on the branch `HEAD` names (or on `HEAD` when
    /// detached) with the reflog message `commit: <message>`, and returns the commit.
    pub fn commit(&self, _message: &str) -> Oid {
        todo!("Repo::commit")
    }
    /// `refs/heads/<name>` at `HEAD`.
    pub fn branch(&self, _name: &str) {
        todo!("Repo::branch")
    }
    /// Appends a value to the repo's own config.
    pub fn config_add(&self, _key: &str, _value: &str) {
        todo!("Repo::config_add")
    }
    /// Makes `dir` a bare repo of the Universe and the remote `origin` of this one (its url is `dir`).
    pub fn bare_origin(&self, _dir: &Path) -> Repo {
        todo!("Repo::bare_origin")
    }
    /// The remote `origin` is `https://github.com/acme/<name>.git`, which resolves to no repo, and
    /// `refs/remotes/origin/<branch>` is the current commit of the branch `HEAD` names.
    pub fn keyed(&self, _name: &str) {
        todo!("Repo::keyed")
    }
    /// Pushes the branch `HEAD` names to `origin`: the objects, `refs/heads/<b>` there and
    /// `refs/remotes/origin/<b>` here.
    pub fn push(&self) {
        todo!("Repo::push")
    }
}
