//! The in-process fake git (ADR 0013, docs/implementing.md "The git route"): a [`Universe`] of objects,
//! repos, refs and worktrees, and [`GitRoute`], the `Handler` of the program `git` of a
//! [`FakeRunner`](crate::host::runner::FakeRunner).

mod merge3;
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod rebase_tests;
mod repo;
mod rev;
mod route;
#[cfg(test)]
mod scenario;
mod sha1;
mod universe;

pub use repo::Repo;
pub use rev::{RevError, Selection, parse, range};
pub use route::GitRoute;
pub use universe::{Commit, Head, Oid, ReflogEntry, RepoId, Universe, UnknownObject, Worktree};

use std::sync::Arc;

use crate::host::runner::FakeRunner;

impl FakeRunner {
    /// Routes the program `git` to `route`: scripted `on`/`once` rules still win over it.
    pub fn with_git(self, route: &Arc<GitRoute>) -> FakeRunner {
        self.route("git", route.clone())
    }
}

#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod remote_tests;
#[cfg(test)]
mod route_read_tests;
#[cfg(test)]
mod twin_tests;
#[cfg(test)]
mod universe_tests;
#[cfg(test)]
mod worktree_tests;
