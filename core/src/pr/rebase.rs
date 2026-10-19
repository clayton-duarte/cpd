//! `pr rebase` (commands.md §9.3, contract §7.6): the Lead Branch onto the
//! base, the Gate, and the push.

use std::path::Path;

use serde_json::json;

use super::row::PlanRow;
use crate::plan::PrRow;
use super::{Args, Env, gate, illegal_phase, lock, require_clean};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, TAIL_LINES, tail};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;

const COMMAND: &str = "pr rebase";

pub(super) fn rebase(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let key = require_open(&plan, &args)?.key.clone();

    // Under the per-repo lock, after R, so that no merge interleaves and none is unrecorded.
    let lock = lock(&mut env, plan_pr(&plan, &key), COMMAND)?;
    let plan = env.plan(&id)?;
    let pr = require_open(&plan, &args)?;
    let lead = pr.lead(&id)?.clone();
    require_clean(&id, pr, &lead)?;
    git::run::ok(&lead, &["fetch", "origin", &pr.base])?;
    let onto = format!("origin/{}", pr.base);
    let branch = pr.lead_branch.as_str();

    // What the remote had when we last saw it, for the lease.
    let remote = git::run::rev_parse(&lead, &format!("refs/remotes/origin/{branch}"))?;

    let merge = git::run::run(&lead, &["merge-tree", "--write-tree", "--name-only", &onto, branch])?;
    match merge.code {
        0 => {}
        1 => {
            // The tree's id, the conflicted files, a blank line, then git's messages.
            let files: Vec<String> = merge.stdout.lines().skip(1).take_while(|l| !l.is_empty()).map(str::to_owned).collect();
            return Err(conflict(&id, &key, branch, &onto, files));
        }
        code => {
            return Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-tree exited {code}."))
                .with("exit", code)
                .with("stderr", tail(&merge.stderr, TAIL_LINES)));
        }
    }

    let old = git::run::rev_parse(&lead, &format!("refs/heads/{branch}"))?;
    if let Some(files) = rebase_onto(&lead, &onto)? {
        return Err(conflict(&id, &key, branch, &onto, files));
    }
    let new = git::run::rev_parse(&lead, &format!("refs/heads/{branch}"))?;

    gate(&env, &id, pr, &lead, &format!("pr rebase --pr {key}"))?;
    let lease = format!("--force-with-lease=refs/heads/{branch}:{remote}");
    git::run::ok(&lead, &["push", &lease, "origin", branch])?;
    drop(lock);

    Ok(Output::success(json!({"plan": id, "pr": key, "rebased": old != new, "old": old, "new": new, "pushed": true})))
}

fn plan_pr<'a>(plan: &'a PlanRow, key: &PrKey) -> &'a PrRow {
    plan.prs.iter().find(|p| p.key == *key).expect("the selected PR is still in the Plan")
}

/// The selected PR must be `open` (`illegal_transition`); a Plan that has not
/// been locked is `illegal_phase`.
fn require_open<'a>(plan: &'a PlanRow, args: &Args) -> Result<&'a PrRow, CpdError> {
    if matches!(plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "deploy"));
    }
    let pr = plan.select(&args.pr)?;
    if pr.status != "open" {
        return Err(pr.refuse(&plan.id, COMMAND, "open"));
    }
    Ok(pr)
}

fn conflict(plan: &PlanId, key: &PrKey, branch: &str, onto: &str, files: Vec<String>) -> CpdError {
    CpdError::new(
        ErrorCode::RebaseConflict,
        format!("{branch} conflicts with {onto} in {} files: nothing was changed. Decide, for example by appending a Job that resolves it.", files.len()),
    )
    .with("plan", plan.as_str())
    .with("pr", key.as_str())
    .with("files", files)
}

/// `git rebase <onto>` in `worktree`. `Some(files)` on a conflict, after
/// `git rebase --abort`; `None` when it rebased.
fn rebase_onto(worktree: &Path, onto: &str) -> Result<Option<Vec<String>>, CpdError> {
    let out = git::run::run(worktree, &["rebase", onto])?;
    if out.code == 0 {
        return Ok(None);
    }
    let conflicts = git::run::ok(worktree, &["diff", "--name-only", "--diff-filter=U"])?;
    let files: Vec<String> = conflicts.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
    let abort = git::run::run(worktree, &["rebase", "--abort"])?;
    if files.is_empty() || abort.code != 0 {
        let (what, failed) = if files.is_empty() {
            ("git rebase failed without a conflict", &out)
        } else {
            ("git rebase --abort failed after a conflict", &abort)
        };
        return Err(CpdError::new(ErrorCode::GitFailed, format!("{what}."))
            .with("exit", failed.code)
            .with("stderr", tail(&failed.stderr, TAIL_LINES)));
    }
    Ok(Some(files))
}
