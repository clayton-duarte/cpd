//! `pr open` (commands.md §9.1, contract §7.6): the Gate, the push, the PR.

use serde_json::json;

use super::row::{PlanRow, Pr};
use super::{Args, Env, gate, gh, illegal_phase, lock, require_clean};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git;
use crate::output::Output;
use crate::plan::{Phase, jobs};

const COMMAND: &str = "pr open";

pub(super) fn open(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    if let Some(done) = require_dispatch(&plan)? {
        return Ok(done);
    }
    require_jobs_done(&mut env, &plan)?;

    // Under the per-repo lock, after R: R may have recorded a merge, and
    // another `pr open` may have finished while this one waited.
    let lock = lock(&mut env, &plan, COMMAND)?;
    let plan = env.plan(&id)?;
    if let Some(done) = require_dispatch(&plan)? {
        return Ok(done);
    }
    let list = require_jobs_done(&mut env, &plan)?;
    let lead = plan.lead()?.clone();
    require_clean(&plan, &lead)?;
    gate(&env, &plan, &lead, COMMAND)?;
    git::run::ok(&lead, &["push", "-u", "origin", &plan.lead_branch])?;
    drop(lock);

    // An earlier run that died before its transaction already created the PR: adopt it.
    let found = match gh::open_pr(&lead, &plan.lead_branch)? {
        Some(found) => found,
        None => gh::create_pr(&lead, &plan.base, &plan.lead_branch, &plan.title, &summary(&plan, &list))?,
    };
    env.store.write(COMMAND, |tx| {
        let moved = tx.execute(
            "UPDATE plans SET pr_number = ?1, pr_url = ?2, pr_opened_at = (SELECT at FROM txns WHERE id = cpd_txn()), phase = 'pr'
              WHERE id = ?3 AND phase = 'dispatch'",
            rusqlite::params![found.number, found.url, id.as_str()],
        )?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {id} left phase dispatch meanwhile: rerun the command."))
                .with("plan", id.as_str()));
        }
        let text = format!(
            "Plan {id} opened PR #{}: {}. The engine polls it; to follow it by hand: cpd2 pr poll --plan {id}",
            found.number, found.url
        );
        let data = json!({"from": "dispatch", "to": "pr", "pr": {"number": found.number, "url": found.url}});
        append(tx, NewEvent::new(Kind::PlanPhase, text, data).plan(id.clone()))?;
        Ok(())
    })?;

    let pr = pr_of(&mut env, &id)?;
    Ok(Output::success(json!({"plan": id, "phase": "pr", "pr": pr.to_json()})))
}

fn pr_of(env: &mut Env, id: &crate::ids::PlanId) -> Result<Pr, CpdError> {
    let plan = env.plan(id)?;
    plan.pr.ok_or_else(|| CpdError::new(ErrorCode::StoreFailed, format!("Plan {id} is in phase pr but has no PR.")))
}

/// The phase must be `dispatch`. `pr` and `done` are `already` (the second value:
/// the success to print); any other phase is `illegal_phase`.
fn require_dispatch(plan: &PlanRow) -> Result<Option<Output>, CpdError> {
    match plan.phase {
        Phase::Dispatch => Ok(None),
        Phase::Pr | Phase::Done => {
            let pr = plan.pr.as_ref().expect("a Plan in phase pr or done has a PR (plans CHECK)");
            Ok(Some(Output::success(json!({"plan": plan.id, "phase": plan.phase.as_str(), "pr": pr.to_json(), "already": true}))))
        }
        _ => Err(illegal_phase(plan, COMMAND, "dispatch")),
    }
}

/// Every Job `merged` or `cancelled`, and at least one `merged`; else `jobs_not_done`.
fn require_jobs_done(env: &mut Env, plan: &PlanRow) -> Result<Vec<jobs::Job>, CpdError> {
    let all = env.store.read(|conn| jobs::load(conn, &plan.id))?;
    let open: Vec<_> = all
        .iter()
        .filter(|j| j.status != "merged" && j.status != "cancelled")
        .map(|j| json!({"key": j.key, "status": j.status}))
        .collect();
    if !open.is_empty() {
        return Err(CpdError::new(
            ErrorCode::JobsNotDone,
            format!("{} Jobs of Plan {} are not Merged or cancelled yet: see cpd2 job list.", open.len(), plan.id),
        )
        .with("plan", plan.id.as_str())
        .with("jobs", open));
    }
    if !all.iter().any(|j| j.status == "merged") {
        return Err(CpdError::new(ErrorCode::JobsNotDone, format!("No Job of Plan {} is Merged: there is nothing to open a PR for.", plan.id))
            .with("plan", plan.id.as_str())
            .with("jobs", json!([])));
    }
    Ok(all)
}

/// A table cell: one line, no `|`.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The PR body: the Plan's title, the Jobs table with each Job's Merged commit, and the generator line.
fn summary(plan: &PlanRow, list: &[jobs::Job]) -> String {
    let mut lines = vec![format!("# {}", plan.title), String::new(), "| Job | Title | Status | Merged commit |".to_owned(), "| --- | --- | --- | --- |".to_owned()];
    for j in list {
        let sha = j.merged_sha.as_deref().map_or("-".to_owned(), |s| format!("`{s}`"));
        lines.push(format!("| {} | {} | {} | {sha} |", cell(&j.key), cell(&j.title), j.status));
    }
    lines.push(String::new());
    lines.push("Generated by cpd".to_owned());
    lines.push(String::new());
    lines.join("\n")
}
