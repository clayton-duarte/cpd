//! Jobs, Specs and Dependencies (contract §2.2, §2.3, §3.2; commands.md §5).
//!
//! - [`spec`]: the Spec file of `job add` and `job edit`;
//! - `add`, `edit`, `cancel`, `retry`, `urgent`: the commands that write;
//! - `show`: `job list` and `job show`.
//!
//! `job dispatch` belongs to the driver (`crate::driver`).
//!
//! Every writing command decides twice from the same pure-ish function: once
//! in a read transaction, so that a rerun which changes nothing writes nothing
//! (no `txns` row), and again inside its write transaction, so that what it
//! writes is what it checked.

#[cfg(test)]
mod incident_tests;

pub(crate) mod add;
mod approve;
mod chain;
pub(crate) mod cancel;
mod edit;
pub(crate) mod fix_retry;
#[cfg(test)]
mod fix_retry_tests;
pub mod guard;
pub mod open;
mod retry;
mod show;
pub(crate) mod spec;
mod urgent;
pub mod worked_on;
#[cfg(test)]
mod worked_on_tests;

pub(crate) use retry::{retried_since_last_start, retries_pending};

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
use crate::plan::jobs::{Job, KINDS, STATUSES};

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
    /// Mark a Job urgent, only when the User asks (§5.8)
    Urgent(UrgentArgs),
    /// The Lead approves an impl Job to change red tests (§5.9)
    ApproveTests(ApproveArgs),
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
    /// The Tier: <O|S|H><0-5> (required, except with --draft)
    #[arg(long, value_name = "T")]
    tier: Option<Tier>,
    /// The Spec file (JSON) (required, except with --draft)
    #[arg(long, value_name = "file")]
    spec: Option<PathBuf>,
    /// Dependencies, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',')]
    needs: Vec<JobKey>,
    /// A review Job (B2): its Worker forks the Lead, it commits nothing and
    /// writes run/<plan>/reviews/<key>.md; it needs every Job of its PR so far
    #[arg(long, conflicts_with = "needs")]
    review: bool,
    /// Add it as a draft in a phase after plan too: the engine never runs it
    /// until a Plan review of the drafts is approved. Needs no --tier and no
    /// --spec: a stub draft, completed later with job edit
    #[arg(long)]
    draft: bool,
    /// Add it urgent (§2.2): E3 starts it first and may run it beyond the
    /// concurrency. Only when the User explicitly asks
    #[arg(long)]
    urgent: bool,
    /// The Job's kind (§2.2 "Job kinds"): docs, test or impl; default work. open and update are cpd's
    #[arg(long, value_name = "kind", value_parser = PossibleValuesParser::new(KINDS))]
    kind: Option<String>,
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
    /// The new Spec file (JSON): it replaces the whole Spec; a queued Job with no Attempt takes only this flag
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
    /// The new kind (§2.2 "Job kinds"): work, docs, test or impl. open and update are cpd's
    #[arg(long, value_name = "kind", value_parser = PossibleValuesParser::new(KINDS))]
    kind: Option<String>,
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
pub struct UrgentArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ApproveArgs {
    /// The impl Job
    #[arg(value_name = "job")]
    key: JobKey,
    /// A guarded path, or a directory ending in `/` (repeatable)
    #[arg(long = "path", value_name = "p", required = true)]
    paths: Vec<String>,
    /// Why
    #[arg(long, value_name = "text")]
    reason: Text,
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
        Command::Urgent(args) => urgent::urgent(args),
        Command::ApproveTests(args) => approve::approve(args),
        Command::Dispatch(args) => driver::dispatch(args),
        Command::List(args) => show::list(args),
        Command::Show(args) => show::show(args),
    }
}

impl Env {
    /// `tier_invalid` (exit 2, `details.efforts`) unless the Tier's effort is
    /// one pi supports for its model (§9); then `tier_not_allowed` (exit 2)
    /// unless a Worker may have it: H0-H3 or S0-S3 (pro-d82, hs-d554).
    ///
    /// The models are those of the Job's PR repo key (contract §8.3 "Models" (5)): `pr` is the PR the Job is
    /// (or is about to be) on, `None` for the PR the existing Job `job` is on.
    fn check_tier(&mut self, plan: &PlanId, job: &JobKey, pr: Option<&PrKey>, tier: Tier) -> Result<(), CpdError> {
        let home = self.home.clone();
        let models = self.store.read(|conn| {
            plan_phase(conn, plan)?;
            match pr {
                Some(pr) => crate::policy::model::models_of_pr(conn, &home, plan, pr),
                None => crate::policy::model::models_of_job(conn, &home, plan, job.as_str()),
            }
        })?;
        config::catalog::load_with(&models, &mut self.store)?.check(tier)?;
        tier.check_worker()
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

/// `open` and `update` are cpd's: the Lead adds neither (`kind_reserved`, §5.1).
fn check_kind_not_reserved(kind: &str) -> Result<(), CpdError> {
    if matches!(kind, "open" | "update") {
        return Err(CpdError::new(
            ErrorCode::KindReserved,
            format!("The kind {kind} is cpd's: it adds the open Job when a PR starts and the update Job for a PR that needs one. Use docs, test, impl, verify or work."),
        )
        .with("kind", kind));
    }
    Ok(())
}

/// The kinds a PR of `shape` may have while it is `waiting` or `dispatch` (§2.2 "Job kinds"): `tdd` the steps of the
/// template, `docs` and `config` their one, `free` (a PR before migration `job-kinds`) every kind the Lead adds.
fn kinds_allowed(shape: &str) -> &'static [&'static str] {
    match shape {
        "tdd" => &["docs", "test", "impl", "verify"],
        "docs" => &["docs"],
        "config" => &["work"],
        _ => &["work", "docs", "test", "impl", "verify"],
    }
}

/// The PR's stored shape allows a Job of `kind` while the PR is `waiting` or `dispatch`, else `shape_violation`
/// (§5.1). A review Job (kind `work`) is allowed in every shape; an `open` PR allows only `work`.
fn check_shape_allows(prs: &[PrRow], plan: &PlanId, pr: &str, kind: &str, review: bool) -> Result<(), CpdError> {
    let Some(row) = prs.iter().find(|p| p.key.as_str() == pr) else { return Ok(()) };
    let allowed = kinds_allowed(&row.shape);
    if review {
        return Ok(());
    }
    if row.status == "open" {
        if kind == "work" {
            return Ok(());
        }
        return Err(CpdError::new(
            ErrorCode::ShapeViolation,
            format!("PR {pr} has opened: a feedback fix is one work Job, not {kind}."),
        )
        .with("plan", plan.as_str())
        .with("pr", pr)
        .with("status", "open")
        .with("shape", row.shape.as_str())
        .with("kind", kind)
        .with("allowed", ["work"]));
    }
    if !matches!(row.status.as_str(), "waiting" | "dispatch") || allowed.contains(&kind) {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::ShapeViolation,
        format!("PR {pr} has the shape {}: it allows the kinds {}, not {kind}. The shape is fixed; cancel the PR and add another to change it.", row.shape, allowed.join(", ")),
    )
    .with("plan", plan.as_str())
    .with("pr", pr)
    .with("shape", row.shape.as_str())
    .with("kind", kind)
    .with("allowed", allowed))
}

/// An `impl` Job that is not a draft needs at least one `test` Job of its PR (`test_job_required`, §5.1).
fn check_test_job_required(jobs: &[Job], plan: &PlanId, job: &Job) -> Result<(), CpdError> {
    let tests: Vec<&str> = jobs.iter().filter(|j| j.pr == job.pr && j.kind == "test" && j.status != "cancelled").map(|j| j.key.as_str()).collect();
    if job.kind != "impl" || job.needs.iter().any(|n| tests.contains(&n.as_str())) {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::TestJobRequired,
        format!("The impl Job {} must need a test Job of its PR {}: its tests are the ones its test Jobs name.", job.key, job.pr),
    )
    .with("plan", plan.as_str())
    .with("job", job.key.as_str())
    .with("jobs", tests))
}

/// A `verify` Job that is not a draft needs at least one `impl` Job of its PR (`impl_job_required`, §5.1).
fn check_impl_job_required(jobs: &[Job], plan: &PlanId, job: &Job) -> Result<(), CpdError> {
    let impls: Vec<&str> = jobs.iter().filter(|j| j.pr == job.pr && j.kind == "impl" && j.status != "cancelled").map(|j| j.key.as_str()).collect();
    if job.kind != "verify" || job.needs.iter().any(|n| impls.contains(&n.as_str())) {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::ImplJobRequired,
        format!("The verify Job {} must need an impl Job of its PR {}: it checks the code those Jobs build.", job.key, job.pr),
    )
    .with("plan", plan.as_str())
    .with("job", job.key.as_str())
    .with("jobs", impls))
}

/// No key of `needs` is a `cancelled` or `failed` Job, nor an `open` or `update` Job, which nothing waits for
/// (`needs_unavailable`) (§5.1).
fn check_needs_available(jobs: &[Job], plan: &PlanId, needs: &[String]) -> Result<(), CpdError> {
    let reserved: Vec<&str> = jobs.iter().filter(|j| needs.contains(&j.key) && matches!(j.kind.as_str(), "open" | "update")).map(|j| j.key.as_str()).collect();
    if !reserved.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, "A Job cannot need an open or update Job: nothing waits for them.")
            .with("plan", plan.as_str())
            .with("jobs", reserved));
    }
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

/// `job retry`: no key of `needs` is a `cancelled` Job (`needs_unavailable`, exit 3, `details.needs`; a `failed`
/// need is allowed, §5.4, hs-d647): a Job held by a cancelled need can never start.
pub(crate) fn check_needs_not_cancelled(jobs: &[Job], plan: &PlanId, needs: &[String]) -> Result<(), CpdError> {
    let cancelled: Vec<serde_json::Value> = jobs
        .iter()
        .filter(|j| needs.contains(&j.key) && j.status == "cancelled")
        .map(|j| serde_json::json!({"key": j.key, "status": j.status}))
        .collect();
    if !cancelled.is_empty() {
        return Err(CpdError::new(
            ErrorCode::NeedsUnavailable,
            "A Job held by a cancelled need can never start: cancel it and add a replacement whose needs do not name the cancelled Job.",
        )
        .with("plan", plan.as_str())
        .with("needs", cancelled));
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
