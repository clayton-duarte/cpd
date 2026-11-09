//! `pr merge` (commands.md §9.10, swr-github.md §9): queue the merge of one pull request, with a method the repo allows, after
//! checking its cached state. It needs no Plan and no Context.

use std::fmt;
use std::str::FromStr;

use serde_json::json;

use super::gh;
use super::poll;
use super::status::{Required, required_states};
use super::swr_read::checks_of_detail;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;
use crate::swr::endpoint;
use crate::swr::github::effective_pr;
use crate::swr::source::github_write::{
    enqueue, merge_method, queued, target_of, write_repo_settings,
};

/// `<owner>/<repo>#<n>`: `owner` and `repo` non-empty, with no `/` or `#`; `n` a positive integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRef {
    pub(super) repo: String,
    pub(super) number: i64,
}

impl FromStr for PullRef {
    type Err = CpdError;

    fn from_str(text: &str) -> Result<PullRef, CpdError> {
        let usage = || {
            CpdError::new(
                ErrorCode::Usage,
                format!("{text:?} is not <owner>/<repo>#<n>, with n a positive integer."),
            )
        };
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
        Ok(PullRef {
            repo: repo.to_owned(),
            number,
        })
    }
}

impl fmt::Display for PullRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

#[derive(clap::Args, Debug)]
pub struct MergeArgs {
    /// The pull request, as the Dispatch column shows it
    #[arg(value_name = "owner/repo#n")]
    pull: PullRef,
}

impl MergeArgs {
    pub(super) fn pull(self) -> PullRef {
        self.pull
    }
}

/// `pr merge` queues the merge (swr-github.md §9): it checks the effective state of the cached pull request, then enqueues a
/// `merge` mutation whose overlay `merging` closes the `pr-ready` item at once. The engine drains it; nothing here calls GitHub.
/// The Workflow's merge, the unstack of what depends on it and the `pr-merged` Event happen when the poll reads `MERGED`.
pub(super) fn merge(ports: &Ports, args: MergeArgs) -> Result<Output, CpdError> {
    let PullRef { repo, number } = args.pull;
    let home = CpdHome::from_env()?;
    let mut store = Store::open(&home, STORE_BUSY_MS)?;
    let now = ports.clock.utc();
    let Some(e) = effective_pr(&mut store, &repo, number)? else {
        return Err(CpdError::new(
            ErrorCode::ItemNotFound,
            format!("{repo}#{number} is not in the cache: the poll has not read it yet."),
        )
        .with("repo", repo.as_str())
        .with("number", number));
    };
    if !matches!(e.state.as_str(), "open" | "merging") || e.row.merge_state != "clean" {
        return Err(CpdError::new(
            ErrorCode::MergeNotClean,
            format!(
                "{repo}#{number} is {} and {}: only an open pull request that is clean is merged.",
                e.state, e.row.merge_state
            ),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("state", e.state.as_str())
        .with("merge_state", e.row.merge_state.as_str()));
    }
    refuse_unless_required_passed(&home, &repo, number, &e.row.checks, e.detail.as_ref())?;
    let Some(head) = e.head_oid.clone() else {
        return Err(CpdError::new(
            ErrorCode::ItemNotFound,
            format!(
                "{repo}#{number} has no head commit in the cache yet: the poll has not read it."
            ),
        )
        .with("repo", repo.as_str())
        .with("number", number));
    };
    // The up-to-date gate (commands.md §9.10): a head that is behind the default branch is refused and the Workflow's
    // update Job is queued to merge the default branch in, with the merge armed for when the checks pass again.
    if let (Some(base), Some(default)) =
        (e.row.base_ref.as_deref(), e.row.default_branch.as_deref())
        && base == default
    {
        let contained = gh::head_contains_base(&*ports.runner, home.root(), &repo, default, &head)?;
        if !contained {
            poll::arm_for_behind_head(&mut store, &home, &repo, number)?;
            return Err(CpdError::new(
                ErrorCode::HeadBehindMain,
                "behind main, updated, waiting for gate".to_owned(),
            )
            .with("repo", repo.as_str())
            .with("number", number)
            .with("head", head.as_str())
            .with("main", default));
        }
    }
    let mut method = merge_method(&mut store, &repo)?;
    if method.is_none() {
        // The settings cache is cold (air-n70): no refusal. A paused `github` endpoint refuses with the rate limit and
        // reads nothing; otherwise exactly one `gh` read of the repo's settings fills the row, then the method is read
        // again. The read's own `gh` error surfaces; nothing is invalidated and no row is ever dropped.
        if let Some(row) = endpoint::get(&mut store, "github")?
            && endpoint::is_paused(&row, now)
        {
            let reset = row
                .paused_until
                .map(crate::clock::timestamp)
                .unwrap_or_default();
            return Err(CpdError::new(
                ErrorCode::GhRateLimited,
                format!(
                    "The github endpoint is rate limited until {reset}: the settings of {repo} are not cached and cannot be read now. Try again after the reset."
                ),
            )
            .with("repo", repo.as_str())
            .with("rate_limited", true)
            .with("reset_at", reset));
        }
        let settings = gh::repo_settings(&*ports.runner, home.root(), &repo)?;
        store.write("swr repo settings", |tx| {
            write_repo_settings(tx, &repo, &settings, now)
        })?;
        method = merge_method(&mut store, &repo)?;
    }
    let method = match method {
        Some(Ok(method)) => method,
        Some(Err(methods)) => {
            return Err(CpdError::new(
                ErrorCode::NoMergeMethod,
                format!("{repo} allows no merge method: squash, merge commit and rebase are all off in its settings, so cpd cannot merge #{number}. Allow one in the repo's settings."),
            )
            .with("repo", repo.as_str())
            .with("number", number)
            .with("squash", methods.squash)
            .with("merge", methods.merge)
            .with("rebase", methods.rebase));
        }
        None => {
            return Err(CpdError::new(
                ErrorCode::GhFailed,
                format!("The on-demand settings read of {repo} wrote no swr_repos row."),
            )
            .with("repo", repo.as_str()));
        }
    };
    let id = enqueue(
        &mut store,
        now,
        &target_of(&repo, number),
        "merge",
        json!({"expected_head_oid": head, "method": method.as_str()}),
        Some(json!({"state": "merging"})),
    )?;
    queued(
        &mut store,
        &repo,
        number,
        id,
        json!({"repo": repo, "number": number, "method": method.as_str()}),
    )
}

/// `required_check_not_passed` when the persisted required-check verdict of the pull request (`pull_requests.checks`, the
/// same record the Inbox pr-ready readiness reads) is not `passed`. The names of the refusal are the missing, pending and
/// failed ones `checks_of_detail` reads from the cached detail; the detail never overrides a stored `passed` verdict
/// (air-d48), so a cache that lags the poll cannot refuse a ready pull request.
fn refuse_unless_required_passed(
    home: &CpdHome,
    repo: &str,
    number: i64,
    verdict: &str,
    detail: Option<&serde_json::Value>,
) -> Result<(), CpdError> {
    let required = poll::repo_required(home, repo)?;
    if required.is_empty() {
        return Ok(());
    }
    // The poll's live read already evaluated the required checks: a `passed` verdict proceeds, whatever the cache holds.
    if verdict == "passed" {
        return Ok(());
    }
    let checks = detail.map(checks_of_detail).unwrap_or_default();
    let states = required_states(&checks, &required);
    let named = |wanted: Required| {
        states
            .iter()
            .filter(|(_, s)| *s == wanted)
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
    };
    let (missing, pending, failed) = (
        named(Required::Missing),
        named(Required::Pending),
        named(Required::Failed),
    );
    Err(CpdError::new(
        ErrorCode::RequiredCheckNotPassed,
        format!("{repo}#{number} has required checks that have not passed on its head (missing: {missing:?}, pending: {pending:?}, failed: {failed:?})."),
    )
    .with("repo", repo)
    .with("number", number)
    .with("missing", missing)
    .with("pending", pending)
    .with("failed", failed))
}

const STORE_BUSY_MS: u64 = 5000;

#[cfg(test)]
#[path = "merge_uptodate_tests.rs"]
mod merge_uptodate_tests;

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
        for bad in [
            "",
            "foo",
            "o/r",
            "o/r#",
            "o/r#x",
            "o/r#0",
            "o/r#-1",
            "o/r#+1",
            "o/r#1#2",
            "/r#1",
            "o/#1",
            "o/r/s#1",
            "#1",
            "o#1",
            "o/r#99999999999999999999",
        ] {
            assert_eq!(
                bad.parse::<PullRef>().unwrap_err().code(),
                ErrorCode::Usage,
                "{bad:?}"
            );
        }
    }
}
