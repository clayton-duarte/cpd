//! Revisions: `HEAD`, a full sha, refs in git's DWIM order, `^{commit}`, `^{tree}`, `<rev>:<path>`,
//! `a..b` and `a...b`.
//!
//! STUB: every body is `todo!()`.

use std::path::Path;

use super::universe::{Oid, Universe};

/// Why a revision did not resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevError {
    /// Names nothing: no such ref, an unborn `HEAD`, an object the repo does not hold, a path not in the tree.
    Unknown(String),
    /// Names an object of the wrong type for `^{commit}` / `^{tree}`.
    WrongType(String),
}

/// What `rev-list` selects: reachable from `include`, not from `exclude`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub include: Vec<Oid>,
    pub exclude: Vec<Oid>,
}

/// Resolves one revision in the repo of the worktree `wt` (the `HEAD` it reads is that worktree's).
/// DWIM order for a name `<x>`: `<x>`, `refs/<x>`, `refs/tags/<x>`, `refs/heads/<x>`, `refs/remotes/<x>`,
/// `refs/remotes/<x>/HEAD`; the first that exists wins.
pub fn parse(_u: &Universe, _wt: &Path, _spec: &str) -> Result<Oid, RevError> {
    todo!("rev::parse")
}

/// Resolves `a..b` (include `b`, exclude `a`) or `a...b` (include both, exclude their merge base); a side
/// left empty is `HEAD`.
pub fn range(_u: &Universe, _wt: &Path, _spec: &str) -> Result<Selection, RevError> {
    todo!("rev::range")
}
