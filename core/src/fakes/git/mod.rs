//! The in-process fake git (ADR 0013, docs/implementing.md "The git route"): a [`Universe`] of objects,
//! repos, refs and worktrees, and [`GitRoute`], the `Handler` of the program `git` of a
//! [`FakeRunner`](crate::host::runner::FakeRunner).
//!
//! STUB (Job port-git-tests): every body is `todo!()`; Job port-git implements it.

mod repo;
mod rev;
mod route;
mod universe;

pub use repo::Repo;
pub use rev::{RevError, Selection, parse, range};
pub use route::GitRoute;
pub use universe::{Commit, Head, Oid, RepoId, ReflogEntry, Universe, UnknownObject, Worktree};

use std::sync::Arc;

use crate::host::runner::FakeRunner;

impl FakeRunner {
    /// Routes the program `git` to `route`: scripted `on`/`once` rules still win over it.
    pub fn with_git(self, _route: &Arc<GitRoute>) -> FakeRunner {
        todo!("route the program `git` to the GitRoute")
    }
}

#[cfg(test)]
mod route_read_tests;
#[cfg(test)]
mod universe_tests;
