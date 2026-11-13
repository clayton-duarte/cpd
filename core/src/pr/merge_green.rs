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
    let open_threads = e
        .feedback
        .iter()
        .flatten()
        .filter(|i| i.kind == super::comments::Kind::Thread)
        .count();
    if open_threads > 0 {
        return Err(CpdError::new(
            ErrorCode::ChecksNotGreen,
            format!("{repo}#{number}: {open_threads} review thread(s) are unresolved; nothing was changed."),
        )
        .with("repo", repo.as_str())
        .with("number", number)
        .with("open_threads", open_threads));
    }
    // Prerequisites gate (§9.5b): unmet external prerequisites refuse before any gh call
    let owning_workflow: Option<(String, String)> = store.read(|conn| {
        use rusqlite::OptionalExtension;
        Ok(conn
            .query_row(
                "SELECT plan, key FROM workflows WHERE repo = ?1 AND pr_number = ?2 AND status = 'open'
                 UNION
                 SELECT plan, key FROM workflows WHERE pull_request = ?3 AND status = 'open'
                 LIMIT 1",
                rusqlite::params![repo.as_str(), number, e.row.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    })?;
    if let Some((plan, key)) = owning_workflow {
        store.read(|conn| {
            crate::workflow::prereq::ensure_prereqs_met(
                conn,
                &*ports.runner,
                &plan,
                &key,
                "pr merge-green",
            )
        })?;
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
