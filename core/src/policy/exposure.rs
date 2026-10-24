//! Which scopes may set which domain (contract §8.3, the domain table's `exposure` column;
//! commands.md §22.1; decisions pro-d7, pro-d17, pro-d18).
//!
//! STUB: the bodies are `todo!()`; the red tests are `exposure_tests.rs`.
//!
//! The rules this API implements:
//!
//! - [`exposure`]: the scopes a domain may be set in, in the order machine, owner, repo. `pr`,
//!   `deploy`, `code`, `tests`, `monitor` and `instructions` are exposed to all three; `model`
//!   to machine and owner (never repo); `mcp` to machine only; the repo-only domains (`hooks`,
//!   `test-commands`, `protected`) to repo only. The table is one row per domain in the domain
//!   registry, so a new domain with no exposure row must not compile.
//! - [`check_exposed`]: a scope file of `kind` that has a table of a domain not exposed to
//!   `kind` is `policy_invalid` with `details.file` = `file` and `details.key` = the domain's
//!   table name; the message names the exposed scopes (contract §8.3 "Exposure rule").

use std::path::Path;

use super::domain::{Domain, Scope};
use crate::error::CpdError;

/// A policy scope (contract §8.3 "Model"), the less specific first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    Machine,
    Owner,
    Repo,
}

/// The scopes `domain` may be set in, in the order machine, owner, repo.
pub fn exposure(_domain: Domain) -> &'static [ScopeKind] {
    todo!("exposure registry")
}

/// Ok when every table `scope` sets is exposed to `kind`; else `policy_invalid` for `file`.
pub fn check_exposed(_file: &Path, _kind: ScopeKind, _scope: &Scope) -> Result<(), CpdError> {
    todo!("exposure check")
}

#[cfg(test)]
#[path = "exposure_tests.rs"]
mod tests;
