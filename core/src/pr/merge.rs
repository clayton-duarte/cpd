//! `pr merge` (commands.md §9.10): merge, with a method the repo allows, one pull request on GitHub, after
//! re-reading it. It needs no Plan and no Context. It stores the merge (hs-d526) from its
//! own success or from a precheck that reads MERGED, as the poll would: one transaction.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde_json::json;

use super::gh;
use super::poll;
use super::pulls;
use super::status::{Required, required_states};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::Runner;
use crate::ids::{PlanId, WorkflowKey};
use crate::inbox::model::close_pr_ready;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;
use crate::store::WriteTxn;

/// `<owner>/<repo>#<n>`: `owner` and `repo` non-empty, with no `/` or `#`; `n` a positive integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRef {
    repo: String,
    number: i64,
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

pub(super) fn merge(ports: &Ports, args: MergeArgs) -> Result<Output, CpdError> {
    let runner = &*ports.runner;
    let PullRef { repo, number } = args.pull;
    // `--repo` names the repository: any directory will do.
    let cwd = Path::new(".");
    let view = gh::view_for_merge(runner, cwd, &repo, number)?;
    if view.state == "MERGED" {
        store_merge(&repo, number, view.merged_at.as_deref())?;
    }
    if view.state != "OPEN" || view.merge_state != "CLEAN" {
        return Err(CpdError::new(
            ErrorCode::MergeNotClean,
            format!(
                "{repo}#{number} is {} and {}: only an OPEN pull request that is CLEAN is merged.",
                view.state, view.merge_state
            ),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("state", view.state)
        .with("merge_state", view.merge_state));
    }
    refuse_unless_required_passed(runner, cwd, &repo, number)?;
    let methods = gh::merge_methods(runner, cwd, &repo)?;
    let Some(method) = methods.choose() else {
        return Err(CpdError::new(
            ErrorCode::NoMergeMethod,
            format!("{repo} allows no merge method: squash, merge commit and rebase are all off in its settings, so cpd cannot merge #{number}. Allow one in the repo's settings."),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("squash", methods.squash)
        .with("merge", methods.merge)
        .with("rebase", methods.rebase));
    };
    gh::merge_pr(runner, cwd, &repo, number, method, &view.head)?;
    // The merge happened; store it only from what GitHub reads back (hs-d550). A failed read stores nothing.
    let stored = match gh::view_for_merge(runner, cwd, &repo, number) {
        Ok(read) if read.state == "MERGED" => {
            store_merge(&repo, number, read.merged_at.as_deref())?;
            true
        }
        _ => false,
    };
    let warnings: Vec<serde_json::Value> = match gh::delete_branch_on_merge(runner, cwd, &repo) {
        Ok(true) => vec![],
        Ok(false) => vec![json!({"code": "auto_delete_off", "text": format!(
            "{repo} has delete_branch_on_merge off: the merged branch stays on GitHub and a pull request stacked on it is not repointed to the base. Turn the setting on in the repo's settings."
        )})],
        Err(e) => vec![json!({"code": "auto_delete_unread", "text": e.message()})],
    };
    Ok(Output::success(
        json!({"repo": repo, "number": number, "method": method.as_str(), "stored": stored, "warnings": warnings}),
    ))
}

/// `required_check_not_passed` when a name of the repo policy's `pr.required_checks` is missing, pending or failed on the head.
fn refuse_unless_required_passed(
    runner: &dyn Runner,
    cwd: &Path,
    repo: &str,
    number: i64,
) -> Result<(), CpdError> {
    let required = poll::repo_required(&CpdHome::from_env()?, repo)?;
    if required.is_empty() {
        return Ok(());
    }
    let checks = gh::checks_for_merge(runner, cwd, repo, number)?;
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
    if missing.is_empty() && pending.is_empty() && failed.is_empty() {
        return Ok(());
    }
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

/// The merge of `repo#number`, in one transaction: its `pull_requests` row (when stored and not merged yet) `merged` at
/// GitHub's time, else the transaction's; the `prs` row that points to it, merged as the poll does; its open `pr-ready` item.
fn store_merge(repo: &str, number: i64, merged_at: Option<&str>) -> Result<(), CpdError> {
    // The store alone, not `Env`: `pr merge` needs no config (the default of `timeouts.store_busy_ms`).
    let mut store = Store::open(&CpdHome::from_env()?, STORE_BUSY_MS)?;
    store.write("pr merge", |tx| store_merge_in(tx, repo, number, merged_at))
}

fn store_merge_in(
    tx: &WriteTxn<'_>,
    repo: &str,
    number: i64,
    merged_at: Option<&str>,
) -> Result<(), CpdError> {
    let Some(row) = pulls::find(tx, repo, number)? else {
        return Ok(());
    };
    if row.state == "merged" {
        return Ok(());
    }
    tx.execute(
        "UPDATE pull_requests SET state = 'merged', merged_at = coalesce(?1, (SELECT at FROM txns WHERE id = cpd_txn())),
                changed_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?2",
        rusqlite::params![merged_at, row.id],
    )?;
    let after = pulls::load(tx, row.id)?;
    let owners: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT plan, key FROM workflows WHERE pull_request = ?1 AND status = 'open'")?;
        stmt.query_map([row.id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    for (plan, key) in owners {
        let (plan, key): (PlanId, WorkflowKey) = (plan.parse()?, key.parse()?);
        poll::merge(tx, &plan, &key, &after)?;
        poll::unstack_stacked_on(tx, &plan, &key)?;
    }
    crate::incident::end_if_until_workflow_merged(tx)?;
    close_pr_ready(tx, repo, number)
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
