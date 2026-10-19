//! `pr merge` (commands.md §9.10): squash-merge one pull request on GitHub, after
//! re-reading it. It needs no Plan, no Context and no store, and writes nothing:
//! the next `pr poll` records the merge.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde_json::json;

use super::gh;
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;

/// `<owner>/<repo>#<n>`: `owner` and `repo` non-empty, with no `/` or `#`; `n` a positive integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRef {
    repo: String,
    number: i64,
}

impl FromStr for PullRef {
    type Err = CpdError;

    fn from_str(text: &str) -> Result<PullRef, CpdError> {
        let usage = || CpdError::new(ErrorCode::Usage, format!("{text:?} is not <owner>/<repo>#<n>, with n a positive integer."));
        let (repo, number) = text.split_once('#').ok_or_else(usage)?;
        let (owner, name) = repo.split_once('/').ok_or_else(usage)?;
        let part = |p: &str| !p.is_empty() && !p.contains(['/', '#']);
        let number = number
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| number.parse::<i64>().ok())
            .flatten()
            .filter(|n| *n > 0)
            .ok_or_else(usage)?;
        if !part(owner) || !part(name) {
            return Err(usage());
        }
        Ok(PullRef { repo: repo.to_owned(), number })
    }
}

impl fmt::Display for PullRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

#[derive(clap::Args, Debug)]
pub struct MergeArgs {
    /// The pull request, as the Deploy column shows it
    #[arg(value_name = "owner/repo#n")]
    pull: PullRef,
}

pub(super) fn merge(args: MergeArgs) -> Result<Output, CpdError> {
    let PullRef { repo, number } = args.pull;
    // `--repo` names the repository: any directory will do.
    let cwd = Path::new(".");
    let view = gh::view_for_merge(cwd, &repo, number)?;
    if view.state != "OPEN" || view.merge_state != "CLEAN" {
        return Err(CpdError::new(
            ErrorCode::MergeNotClean,
            format!("{repo}#{number} is {} and {}: only an OPEN pull request that is CLEAN is merged.", view.state, view.merge_state),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("state", view.state)
        .with("merge_state", view.merge_state));
    }
    gh::merge_pr(cwd, &repo, number, &view.head)?;
    Ok(Output::success(json!({"repo": repo, "number": number, "method": "squash", "branch_deleted": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_reference_is_owner_repo_hash_number() {
        let pull: PullRef = "o/r#7".parse().unwrap();
        assert_eq!((pull.repo.as_str(), pull.number), ("o/r", 7));
        assert_eq!(pull.to_string(), "o/r#7");
    }

    #[test]
    fn anything_else_is_usage() {
        for bad in ["", "foo", "o/r", "o/r#", "o/r#x", "o/r#0", "o/r#-1", "o/r#+1", "o/r#1#2", "/r#1", "o/#1", "o/r/s#1", "#1", "o#1", "o/r#99999999999999999999"] {
            assert_eq!(bad.parse::<PullRef>().unwrap_err().code(), ErrorCode::Usage, "{bad:?}");
        }
    }
}
