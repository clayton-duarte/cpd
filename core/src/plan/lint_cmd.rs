//! `plan lint` (commands.md §4.15): every error and warning of the Plan's drafts in one list. Read-only.

use serde_json::{Value, json};

use super::jobs::{self, Job};
use super::lint::{Finding, Input, Severity, run};
use super::prs::{WorkflowRow, load_workflows};
use super::row::{self, Phase, PlanRow};
use super::tier_advice::{self, Advice, FLOOR, recommend};
use crate::args::PlanArg;
use crate::config;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::policy::model::models_of_context;

#[derive(clap::Args, Debug)]
pub struct LintArgs {
    #[command(flatten)]
    plan: PlanArg,
}

/// The one place that lints a Plan that is already loaded.
pub(super) fn find(
    env: &mut Env,
    plan: &PlanRow,
    workflows: &[WorkflowRow],
    jobs: &[Job],
) -> Result<Vec<Finding>, CpdError> {
    let advice = advice(env, plan.context.as_str())?;
    Ok(run(&Input::new(
        &plan.id,
        plan.phase,
        workflows,
        jobs,
        plan.outline.as_ref(),
    )
    .with_advice(&advice)))
}

/// The Tier advice the `tier_advice` rule reads (contract §2.7). The catalog is read only when a cell can advise.
pub(super) fn advice(env: &mut Env, context: &str) -> Result<Vec<Advice>, CpdError> {
    let rows = env.store.read(tier_advice::first_attempts)?;
    if !rows
        .iter()
        .any(|r| r.n >= FLOOR && r.tier.is_worker_allowed())
    {
        return Ok(Vec::new());
    }
    let catalog = match env.catalog.clone() {
        Some(c) => c,
        None => {
            let home = env.home.clone();
            let models = env
                .store
                .read(|conn| models_of_context(conn, &home, context))?;
            config::catalog::load_with(&models, &mut env.store)?
        }
    };
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| catalog.is_valid(r.tier))
        .collect();
    Ok(recommend(&rows, FLOOR, |t| {
        catalog.next(t).expect("the Tier is valid")
    }))
}

/// `plan lint` (§4.15).
pub(super) fn lint(args: LintArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (plan, workflows, jobs) = env.store.read(|conn| {
        let plan = row::load(conn, &id)?;
        if plan.phase == Phase::Grounding {
            return Err(CpdError::new(
                ErrorCode::IllegalPhase,
                format!(
                    "Plan {id} is in phase {}: a Plan is linted from phase plan on.",
                    plan.phase.as_str()
                ),
            )
            .with("plan", id.as_str())
            .with("phase", plan.phase.as_str()));
        }
        let (workflows, jobs) = (load_workflows(conn, &id)?, jobs::load(conn, &id)?);
        Ok((plan, workflows, jobs))
    })?;
    let findings = find(&mut env, &plan, &workflows, &jobs)?;
    let errors: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity() == Severity::Error)
        .collect();
    let warnings = findings.len() - errors.len();
    let json_findings: Vec<Value> = findings.iter().map(Finding::to_json).collect();
    if errors.is_empty() {
        return Ok(Output::success(json!({
            "plan": plan.id.as_str(),
            "errors": 0,
            "warnings": warnings,
            "findings": json_findings,
        })));
    }
    let rules: Vec<&str> = errors.iter().map(|f| f.rule.as_str()).collect();
    Err(CpdError::new(
        ErrorCode::LintFailed,
        format!(
            "Plan {} has {} lint errors: {}. Fix them, then ask again.",
            plan.id,
            errors.len(),
            rules.join(", ")
        ),
    )
    .with("plan", plan.id.as_str())
    .with("errors", errors.len())
    .with("warnings", warnings)
    .with("findings", json_findings))
}
