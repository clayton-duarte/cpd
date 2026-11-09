//! `pr merge-green` (commands.md §9.10a): the Manager's merge protocol. The cached checks must be green before any `gh`
//! write; then `gh pr update-branch`, a wait for the checks on the new head, and `gh pr merge --squash`.

use std::time::{Duration, Instant};

use serde_json::json;

use super::gh;
use super::merge::{MergeArgs, PullRef};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;
use crate::swr::github::effective_pr;

const WAIT: Duration = Duration::from_secs(3600);
const EVERY: Duration = Duration::from_secs(30);

/// Whether a pull request's cached `checks` value allows the merge protocol to start: only `passed`.
pub fn require_green(checks: &str) -> Result<(), String> {
    if checks == "passed" {
        Ok(())
    } else {
        Err(format!("checks are {checks}, not passed"))
    }
}

pub(super) fn merge_green(ports: &Ports, args: MergeArgs) -> Result<Output, CpdError> {
    let PullRef { repo, number } = args.pull();
    let home = CpdHome::from_env()?;
    let mut store = Store::open(&home, 5000)?;
    let Some(e) = effective_pr(&mut store, &repo, number)? else {
        return Err(CpdError::new(
            ErrorCode::ItemNotFound,
            format!("{repo}#{number} is not in the cache: the poll has not read it yet."),
        ));
    };
    if let Err(why) = require_green(&e.row.checks) {
        return Err(CpdError::new(
            ErrorCode::ChecksNotGreen,
            format!("{repo}#{number}: {why}; nothing was changed."),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("checks", e.row.checks.as_str()));
    }
    let runner = &*ports.runner;
    gh::update_branch(runner, home.root(), &repo, number)?;
    let start = Instant::now();
    let head = loop {
        let (head, green) = gh::head_and_green(runner, home.root(), &repo, number)?;
        if green {
            break head;
        }
        if start.elapsed() > WAIT {
            return Err(CpdError::new(
                ErrorCode::GhFailed,
                format!("{repo}#{number}: checks on the updated head did not pass within an hour."),
            ));
        }
        std::thread::sleep(EVERY);
    };
    gh::merge_squash(runner, home.root(), &repo, number, &head)?;
    Ok(Output::success(
        json!({"repo": repo, "number": number, "head": head, "merged": true}),
    ))
}
