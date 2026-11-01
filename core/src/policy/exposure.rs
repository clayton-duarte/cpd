//! Which scopes may set which domain (contract §8.3, the domain table's `exposure` column;
//! commands.md §22.1; decisions pro-d7, pro-d17, pro-d18).
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
//! - One key-level row below the domain: the table `pr.post` is exposed to owner and repo only,
//!   so a machine file with a `pr.post` table, in any spelling and empty included, is
//!   `policy_invalid` with `details.key` = `pr.post`. The `pr` domain stays exposed to machine.

use std::path::Path;

use super::domain::{exposure, Domain, Scope};
use super::load::policy_invalid;
use crate::error::CpdError;

/// A policy scope (contract §8.3 "Model"), the less specific first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    Machine,
    Owner,
    Repo,
}

impl ScopeKind {
    fn word(self) -> &'static str {
        match self {
            ScopeKind::Machine => "machine",
            ScopeKind::Owner => "owner",
            ScopeKind::Repo => "repo",
        }
    }
}

/// The table `pr.post` and the scopes it is exposed to, in the order of [`ScopeKind`].
const PR_POST_KEY: &str = "pr.post";
const PR_POST_EXPOSURE: &[ScopeKind] = &[ScopeKind::Owner, ScopeKind::Repo];

/// Ok when every table `scope` sets is exposed to `kind`; else `policy_invalid` for `file`.
pub fn check_exposed(file: &Path, kind: ScopeKind, scope: &Scope) -> Result<(), CpdError> {
    for &domain in Domain::ALL {
        if scope.sets(domain) && !exposure(domain).contains(&kind) {
            let exposed: Vec<&str> = exposure(domain).iter().map(|k| k.word()).collect();
            let why = format!(
                "the table `{}` is exposed to {} only, not a {} file",
                domain.name(),
                exposed.join(", "),
                kind.word()
            );
            return Err(policy_invalid(file, Some(domain.name()), why));
        }
    }
    if scope.workflow.as_ref().is_some_and(|workflow| workflow.post.is_some()) && !PR_POST_EXPOSURE.contains(&kind) {
        let exposed: Vec<&str> = PR_POST_EXPOSURE.iter().map(|k| k.word()).collect();
        let why = format!(
            "the table `{}` is exposed to {} only, not a {} file",
            PR_POST_KEY,
            exposed.join(", "),
            kind.word()
        );
        return Err(policy_invalid(file, Some(PR_POST_KEY), why));
    }
    Ok(())
}

#[cfg(test)]
#[path = "exposure_tests.rs"]
mod tests;
