//! `pr push` (commands.md §9.9, contract §7.6): the Gate on the Lead Branch's
//! tip G in the Lead worktree, then a plain push of G by rule P. No rebase,
//! never forced, no lock and no R (hs-d243).

use serde_json::json;

use super::row::PlanWorkflows;
use crate::plan::WorkflowRow;
use super::open::{agent, hold_refused};
use super::{Args, Env, gate, hide_untracked, illegal_phase, pr_policy, require_clean_lead};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, Push};
use crate::output::Output;
use crate::plan::Phase;
use crate::policy::Gate;

const COMMAND: &str = "pr push";

pub(super) fn push(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let pr = require_open(&plan, &args)?;
    let key = pr.key.clone();
    env.store.read(|conn| hold_refused(conn, &id, &key, agent()))?;
    let branch = pr.lead_branch.clone();
    let remote_branch = pr.remote_branch().to_owned();
    let adopted = pr.head_ref.is_some();
    let lead = pr.lead(&id)?.clone();
    let policy = pr_policy(&mut env, &id, &key)?;

    // 1. G, read once; 2. when the PR's gate is local, the Lead worktree brought to G and the Gate on it there.
    let sha = git::run::rev_parse(std::path::Path::new(&pr.repo), &format!("refs/heads/{branch}"))?;
    if policy.gate.pr == Gate::Local {
        require_clean_lead(&lead, &id, pr)?;
        git::sync_lead(&lead, &branch)?;
        let hidden = hide_untracked(&lead)?;
        let gated = gate(&env, &policy, &id, pr, &lead, &sha, &format!("pr push --pr {key}"));
        hidden.restore()?;
        gated?;
    }

    // The PR is read again: merged during the Gate, nothing is pushed.
    let plan = env.plan(&id)?;
    require_open(&plan, &args)?;

    // 3. Rule P with G.
    let (pushed, remote) = match git::push_commit(&lead, &remote_branch, &sha)? {
        Push::Pushed { new } => (new, sha.clone()),
        Push::Contained { remote } => (false, remote),
        Push::Rejected { stderr } => {
            let next = if adopted { "It gained commits cpd lacks: catch up" } else { "Rebase" };
            return Err(CpdError::new(
                ErrorCode::PushRejected,
                format!("The remote {remote_branch} does not contain the gated commit, so it moved: nothing was pushed. {next} with cpd pr rebase --pr {key}."),
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
fn require_open<'a>(plan: &'a PlanWorkflows, args: &Args) -> Result<&'a WorkflowRow, CpdError> {
    if matches!(plan.plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "deploy"));
    }
    let pr = plan.select(&args.pr)?;
    if pr.status != "open" {
        return Err(pr.refuse(&plan.plan.id, COMMAND, "open"));
    }
    Ok(pr)
}
