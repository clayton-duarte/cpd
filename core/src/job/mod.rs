//! Jobs, Specs and Dependencies (contract §2.2, §2.3, §3.2; commands.md §5).
//!
//! - [`spec`]: the Spec file of `job add` and `job edit`;
//! - `add`, `edit`, `cancel`, `retry`: the commands that write;
//! - `show`: `job list` and `job show`.
//!
//! `job dispatch` belongs to the driver (`crate::driver`).
//!
//! Every writing command decides twice from the same pure-ish function: once
//! in a read transaction, so that a rerun which changes nothing writes nothing
//! (no `txns` row), and again inside its write transaction, so that what it
//! writes is what it checked.

mod add;
mod cancel;
mod edit;
mod retry;
mod show;
mod spec;

pub(crate) use retry::retried_since_last_start;

use std::path::PathBuf;

use clap::builder::PossibleValuesParser;
use rusqlite::{Connection, OptionalExtension};

use crate::args::{PlanArg, PrArg, Text};
use crate::config::{self, Tier};
use crate::driver;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{JobKey, PlanId, PrKey};
use crate::output::Output;
use crate::plan::PrRow;
use crate::plan::jobs::{Job, STATUSES};

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Add a Job (§5.1)
    Add(AddArgs),
    /// Edit a draft Job (§5.2)
    Edit(EditArgs),
    /// Cancel a Job (§5.3)
    Cancel(CancelArgs),
    /// Queue a failed Job again at a Tier (§5.4)
    Retry(RetryArgs),
    /// Start one Ready Job now (§5.5)
    Dispatch(driver::DispatchArgs),
    /// List the Jobs (§5.6)
    List(ListArgs),
    /// Show a Job and its Attempts (§5.7)
    Show(ShowArgs),
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    /// The title
    #[arg(long, value_name = "text")]
    title: Text,
    /// The Tier: <O|S|H><0-5>
    #[arg(long, value_name = "T")]
    tier: Tier,
    /// The Spec file (JSON)
    #[arg(long, value_name = "file")]
    spec: PathBuf,
    /// Dependencies, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',')]
    needs: Vec<JobKey>,
    /// A review Job (B2): its Worker forks the Lead, it commits nothing and
    /// writes run/<plan>/reviews/<key>.md; it needs every Job of its PR so far
    #[arg(long, conflicts_with = "needs")]
    review: bool,
    /// Add it as a draft in a phase after plan too: the engine never runs it
    /// until a Plan review of the drafts is approved
    #[arg(long)]
    draft: bool,
    #[command(flatten)]
    pr: PrArg,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct EditArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    /// The new title
    #[arg(long, value_name = "text")]
    title: Option<Text>,
    /// The new Tier: <O|S|H><0-5>
    #[arg(long, value_name = "T")]
    tier: Option<Tier>,
    /// The new Spec file (JSON): it replaces the whole Spec
    #[arg(long, value_name = "file")]
    spec: Option<PathBuf>,
    /// The new Dependencies, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',', conflicts_with = "no_needs")]
    needs: Option<Vec<JobKey>>,
    /// Remove every Dependency
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// Move the Job to this PR (it keeps no old Dependency)
    #[arg(long = "pr", value_name = "key")]
    pr: Option<PrKey>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    /// Why
    #[arg(long, value_name = "text")]
    reason: Text,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct RetryArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    /// The Tier of the next Attempt: <O|S|H><0-5>
    #[arg(long, value_name = "T")]
    tier: Tier,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only Jobs in this status (repeatable)
    #[arg(long = "status", value_name = "status", value_parser = PossibleValuesParser::new(STATUSES))]
    statuses: Vec<String>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    #[command(flatten)]
    plan: PlanArg,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Add(args) => add::add(args),
        Command::Edit(args) => edit::edit(args),
        Command::Cancel(args) => cancel::cancel(args),
        Command::Retry(args) => retry::retry(args),
        Command::Dispatch(args) => driver::dispatch(args),
        Command::List(args) => show::list(args),
        Command::Show(args) => show::show(args),
    }
}

impl Env {
    /// `tier_invalid` (exit 2, `details.efforts`) unless the Tier's effort is
    /// one pi supports for its model (§9).
    fn check_tier(&self, tier: Tier) -> Result<(), CpdError> {
        config::load_catalog(&self.config)?.check(tier)
    }
}

/// The Plan's phase, else `plan_not_found`.
fn plan_phase(conn: &Connection, plan: &PlanId) -> Result<String, CpdError> {
    conn.query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))
        .optional()?
        .ok_or_else(|| CpdError::plan_not_found(plan))
}

/// Where a review Job's Worker writes its findings: `run/<plan>/reviews/<job>.md` (contract §1.4).
pub(crate) fn findings_path(home: &CpdHome, plan: &PlanId, job: &str) -> PathBuf {
    home.plan_dir(plan).join("reviews").join(format!("{job}.md"))
}

/// The Job `key` among `jobs`, else `job_not_found`.
pub(crate) fn find<'a>(jobs: &'a [Job], plan: &PlanId, key: &JobKey) -> Result<&'a Job, CpdError> {
    jobs.iter().find(|j| j.key == key.as_str()).ok_or_else(|| {
        CpdError::new(ErrorCode::JobNotFound, format!("No Job {key} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("job", key.as_str())
    })
}

/// `needs` as the sorted keys, refusing a repeat with `usage` (§5.1).
fn sorted_needs(needs: &[JobKey]) -> Result<Vec<String>, CpdError> {
    let mut keys: Vec<String> = needs.iter().map(|k| k.as_str().to_owned()).collect();
    keys.sort();
    if let Some(pair) = keys.windows(2).find(|w| w[0] == w[1]) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--needs lists {} twice.", pair[0])).with("job", pair[0].as_str()));
    }
    Ok(keys)
}

/// Every key of `needs` is a Job of the Plan (`needs_unknown`) (§5.1).
fn check_needs_known(jobs: &[Job], plan: &PlanId, needs: &[String]) -> Result<(), CpdError> {
    let unknown: Vec<&String> = needs.iter().filter(|n| !jobs.iter().any(|j| j.key == **n)).collect();
    if !unknown.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnknown, format!("Plan {plan} has no Job {}.", join(&unknown)))
            .with("plan", plan.as_str())
            .with("needs", serde_json::json!(unknown)));
    }
    Ok(())
}

/// No key of `needs` is a `cancelled` or `failed` Job (`needs_unavailable`) (§5.1).
fn check_needs_available(jobs: &[Job], plan: &PlanId, needs: &[String]) -> Result<(), CpdError> {
    let unavailable: Vec<serde_json::Value> = jobs
        .iter()
        .filter(|j| needs.contains(&j.key) && matches!(j.status.as_str(), "cancelled" | "failed"))
        .map(|j| serde_json::json!({"key": j.key, "status": j.status}))
        .collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, "A Job cannot need a cancelled or failed Job.")
            .with("plan", plan.as_str())
            .with("needs", unavailable));
    }
    Ok(())
}

/// Every Job of `needs` is in PR `pr` (`needs_other_pr`; `details.jobs` lists the others) (A7).
fn check_needs_same_pr(jobs: &[Job], plan: &PlanId, pr: &str, needs: &[String]) -> Result<(), CpdError> {
    let others: Vec<&str> = jobs.iter().filter(|j| needs.contains(&j.key) && j.pr != pr).map(|j| j.key.as_str()).collect();
    if !others.is_empty() {
        return Err(CpdError::new(
            ErrorCode::NeedsOtherPr,
            format!("A Dependency can only link Jobs of the same PR: {} {} not in PR {pr}.", others.join(", "), if others.len() == 1 { "is" } else { "are" }),
        )
        .with("plan", plan.as_str())
        .with("pr", pr)
        .with("jobs", others));
    }
    Ok(())
}

/// The PR `pr` of the Plan is not `merged` or `cancelled` (`pr_ended`) (§3.2).
fn check_pr_live(prs: &[PrRow], plan: &PlanId, pr: &str) -> Result<(), CpdError> {
    match prs.iter().find(|p| p.key.as_str() == pr) {
        Some(p) if matches!(p.status.as_str(), "merged" | "cancelled") => Err(CpdError::new(
            ErrorCode::PrEnded,
            format!("PR {pr} of Plan {plan} is {}: Jobs are not added to or moved into it.", p.status),
        )
        .with("plan", plan.as_str())
        .with("pr", pr)
        .with("status", p.status.as_str())),
        _ => Ok(()),
    }
}

fn join(keys: &[&String]) -> String {
    keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
}
