//! `pr push` (commands.md §9.9, contract §7.6): the Gate on the Lead Branch's
//! tip G in a gate worktree, then a plain push of G by rule P. No rebase,
//! never forced, no lock and no R (hs-d243).

use serde_json::json;

use super::row::{PlanRow, PrRow};
use super::{Args, Env, gate, illegal_phase};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, Push};
use crate::output::Output;
use crate::plan::Phase;

const COMMAND: &str = "pr push";

pub(super) fn push(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let pr = require_open(&plan, &args)?;
    let key = pr.key.clone();
    let branch = pr.lead_branch.clone();
    let lead = pr.lead(&id)?.clone();

    // 1. G, read once; 2. the Gate on it, in a gate worktree of its own.
    let sha = git::run::rev_parse(&pr.repo, &format!("refs/heads/{branch}"))?;
    gate(&env, &id, pr, &sha, &format!("pr push --pr {key}"))?;

    // The PR is read again: merged during the Gate, nothing is pushed.
    let plan = env.plan(&id)?;
    require_open(&plan, &args)?;

    // 3. Rule P with G.
    let (pushed, remote) = match git::push_commit(&lead, &branch, &sha)? {
        Push::Pushed { new } => (new, sha.clone()),
        Push::Contained { remote } => (false, remote),
        Push::Rejected { stderr } => {
            return Err(CpdError::new(
                ErrorCode::PushRejected,
                format!("The remote {branch} does not contain the gated commit, so it moved: nothing was pushed. Run cpd pr rebase --pr {key}."),
            )
            .with("plan", id.as_str())
            .with("pr", key.as_str())
            .with("sha", sha.as_str())
            .with("stderr", stderr));
        }
    };

    Ok(Output::success(json!({"plan": id, "pr": key, "pushed": pushed, "sha": sha, "remote": remote})))
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
