//! `pr push` (commands.md §9.9, contract §7.6): the Gate on the Lead Branch's
//! tip G in the Lead worktree, then a plain push of G by rule P. No rebase,
//! never forced, no lock and no R (hs-d243).

use serde_json::json;

use super::open::{agent, hold_refused};
use super::row::PlanWorkflows;
use super::{Args, Env, gate, hide_untracked, illegal_phase, require_clean_lead, workflow_policy};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, Push};
use crate::output::Output;
use crate::plan::WorkflowRow;
use crate::policy::Gate;

const COMMAND: &str = "pr push";

pub(super) fn push(args: Args) -> Result<Output, CpdError> {
    super::rebase::repaired(COMMAND, args, push_held)
}

fn push_held(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let workflow = require_pushable(&plan, &args)?;
    let key = workflow.key.clone();
    env.store
        .read(|conn| hold_refused(conn, &id, &key, agent()))?;
    // Checked before the Gate too: a running Attempt refuses at once, without a long Gate
    // (commands.md §9.9). The transaction below re-checks it, since an Attempt may start during the Gate.
    env.store
        .read(|conn| super::rebase::refuse_running_conn(conn, &id, &key, "do the push"))?;
    let branch = workflow.lead_branch.clone();
    let remote_branch = workflow.remote_branch().to_owned();
    let adopted = workflow.head_ref.is_some();
    let lead = workflow.lead(&id)?.clone();
    let policy = workflow_policy(&mut env, &id, &key)?;

    // 1. G, read once; 2. when the Workflow's gate is local, the Lead worktree brought to G and the Gate on it there.
    let sha = git::run::rev_parse(
        std::path::Path::new(&workflow.repo),
        &format!("refs/heads/{branch}"),
    )?;
    env.store.read(|conn| {
        crate::workflow::guard_refactor_tests(
            conn,
            &id,
            key.as_str(),
            std::path::Path::new(&workflow.repo),
            &workflow.base,
            &sha,
        )
    })?;
    if workflow.shape == "refactor"
        && let Some(command) = &policy.coverage_commands.measure
    {
        let repo = std::path::Path::new(&workflow.repo);
        use rusqlite::OptionalExtension;
        let stored: Option<(String, String)> = env.store.read(|conn| {
            Ok(conn.query_row("SELECT base_sha, report FROM refactor_coverage_baselines WHERE plan = ?1 AND workflow = ?2", [id.as_str(), key.as_str()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
        })?;
        let (mut baseline_sha, report) = stored.ok_or_else(|| {
            CpdError::new(
                ErrorCode::CoverageBelowBaseline,
                "Coverage baseline was not captured at workflow creation.",
            )
        })?;
        let mut baseline: crate::workflow::CoverageReport = serde_json::from_str(&report)
            .map_err(|e| CpdError::bad_row(format!("invalid coverage baseline: {e}")))?;
        // Refresh only after an amended base is incorporated in the refactor branch.
        let base_ref = git::run::run(repo, &["rev-parse", "--verify", &workflow.base])?;
        if base_ref.code == 0 {
            let candidate = base_ref.stdout.trim();
            let descendant = git::run::run(
                repo,
                &["merge-base", "--is-ancestor", &baseline_sha, candidate],
            )?;
            let incorporated =
                git::run::run(repo, &["merge-base", "--is-ancestor", candidate, &sha])?;
            if !matches!(descendant.code, 0 | 1) || !matches!(incorporated.code, 0 | 1) {
                return Err(CpdError::new(
                    ErrorCode::GitFailed,
                    "Could not check coverage baseline ancestry.",
                ));
            }
            if candidate != baseline_sha && descendant.code == 0 && incorporated.code == 0 {
                baseline = crate::workflow::measure_coverage(repo, &id, &key, candidate, command)?;
                let json = serde_json::to_string(&baseline)
                    .map_err(|e| CpdError::bad_row(e.to_string()))?;
                env.store.write(COMMAND, |tx| {
                    let changed = tx.execute("UPDATE refactor_coverage_baselines SET base_sha = ?3, report = ?4 WHERE plan = ?1 AND workflow = ?2 AND base_sha = ?5", rusqlite::params![id.as_str(), key.as_str(), candidate, json, baseline_sha])?;
                    if changed != 1 { return Err(CpdError::new(ErrorCode::CasLost, "Coverage baseline changed during refresh; retry.")); }
                    Ok(())
                })?;
                baseline_sha = candidate.to_owned();
            }
        }
        let _ = (baseline, baseline_sha);
        super::coverage_gate(&env, &policy, &id, workflow, &sha)?;
    }
    if policy.gate.workflow == Gate::Local {
        require_clean_lead(&lead, &id, workflow)?;
        git::sync_lead(&lead, &branch)?;
        let hidden = hide_untracked(&lead)?;
        let gated = gate(
            &env,
            &policy,
            &id,
            workflow,
            &lead,
            &sha,
            &format!("pr push --workflow {key}"),
        );
        hidden.restore()?;
        gated?;
    }

    // The Workflow is read again: merged during the Gate, nothing is pushed.
    let plan = env.plan(&id)?;
    require_pushable(&plan, &args)?;

    // 3. Rule P with G, inside one write transaction: the driver starts an Attempt in a store write
    // transaction of its own (ADR-0014), so re-reading the status and the running Attempts here
    // serialises the push with the Attempt start. The Attempt start either committed before this
    // transaction (the refusal wins) or waits for it (the push wins, and the next Attempt's
    // `base_sha` is the pushed commit). The status was re-read before the transaction too, so a Workflow
    // merged during the Gate is refused without taking the write lock.
    let (pushed, remote) = env.store.write(COMMAND, |tx| {
        let plan = super::row::load(tx, &id)?;
        require_pushable(&plan, &args)?;
        super::rebase::refuse_running_conn(tx, &id, &key, "do the push")?;

        let to_push = match crate::merge::reconcile(&lead, &branch, &remote_branch, &sha)? {
            crate::merge::Reconciled::Push(to_push) => to_push,
            crate::merge::Reconciled::Contained(remote) => return Ok((false, remote)),
            crate::merge::Reconciled::Conflict { remote, files } => {
                return Err(crate::merge::remote_conflict(&remote_branch, &sha, &remote, &files)
                    .with("workflow", key.as_str()));
            }
        };
        match git::push_commit(&lead, &remote_branch, &to_push)? {
            Push::Pushed { new } => {
                if new {
                    super::pulls::reset_after_push(tx, &id, &key)?;
                }
                if to_push != sha {
                    git::sync_lead(&lead, &branch)?;
                }
                Ok((new, to_push.clone()))
            }
            Push::Contained { remote } => Ok((false, remote)),
            Push::Rejected { stderr } => {
                let next = if adopted { "It gained commits cpd lacks: catch up" } else { "Rebase" };
                Err(CpdError::new(
                    ErrorCode::PushRejected,
                    format!("The remote {remote_branch} does not contain the gated commit, so it moved: nothing was pushed. {next} with cpd pr rebase --workflow {key}."),
                )
                .with("plan", id.as_str())
                .with("pr", key.as_str())
                .with("sha", sha.as_str())
                .with("stderr", stderr))
            }
        }
    })?;

    if pushed {
        super::request_poll_now();
    }
    Ok(Output::success(
        json!({"plan": id, "pr": key, "pushed": pushed, "sha": sha, "remote": remote}),
    ))
}

/// The selected Workflow must be `open` or `dispatch` (`illegal_transition`); a Plan that has not
/// been locked is `illegal_phase`. A `dispatch` Workflow is started but not yet open: it has a Lead
/// worktree and Lead Branch, so pre-open pushes are allowed (hs-d801).
fn require_pushable<'a>(plan: &'a PlanWorkflows, args: &Args) -> Result<&'a WorkflowRow, CpdError> {
    if plan.plan.locked_at.is_none() {
        return Err(illegal_phase(plan, COMMAND, "deploy"));
    }
    let workflow = plan.select(&args.workflow)?;
    if workflow.status != "open" && workflow.status != "dispatch" {
        return Err(workflow.refuse(&plan.plan.id, COMMAND, "open or dispatch"));
    }
    Ok(workflow)
}
