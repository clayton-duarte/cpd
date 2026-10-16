//! Pull requests (contract §7.6; commands.md §9), through `gh`:
//!
//! - `add`, `edit`, `cancel`: the Plan's PRs before they start (`pr add`, `pr edit`, `pr cancel`);
//! - `start`: starting a PR (`pr start`), the routine `plan lock` and engine E7 call too;
//! - `open`: the Gate, the push and the PR (`pr open`);
//! - `poll`, with `feedback` and `pulls` (the table `pull_requests`, written only there): one poll of every open pull request of the User (`pr poll`);
//! - `rebase`: the Lead Branch onto the base, the Gate and the push (`pr rebase`);
//! - `gh`: the one gateway to `gh`; `status`: the PR status enums; `row`: the Plan as these commands read it.
//!
//! `pr open` and `pr rebase` run under the per-repo lock, after R (§7.4); each
//! works on one PR of the Plan, `poll` on every open one (§7.6).

mod add;
mod cancel;
mod edit;
// `current_checks` is `pr view`'s (Job `pr-view2`); until then nothing calls it.
#[allow(dead_code)]
mod feedback;
// `pr view` (Job `pr-view2`) reads the reviews, the comments and `inline_comments`; until then nothing does.
#[allow(dead_code)]
mod gh;
mod open;
mod poll;
pub mod pulls;
mod rebase;
mod row;
pub mod start;
// The summary of `gh pr view`'s checks (`ChecksSummary::of`) is `pr view`'s (Job `pr-view2`); until then nothing calls it.
#[allow(dead_code)]
mod status;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::args::{PlanArg, PrArg};
use crate::config::{self, Config};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, HookOutcome, Operation, run_hook};
use crate::home::{CpdHome, Hook};
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::merge::{self, RepoLock};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::PrSpec;
use crate::store::Store;
use row::{PlanRow, PrRow};

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Add a PR to the Plan (§9.4)
    Add(AddArgs),
    /// Change a waiting PR before the lock (§9.5)
    Edit(EditArgs),
    /// Remove a PR from the Plan before the lock (§9.6)
    Cancel(CancelArgs),
    /// Start a waiting PR whose needs are merged, after a failed start (§9.7)
    Start(StartArgs),
    /// Push the Lead Branch and open the PR (§9.1)
    Open(Args),
    /// Poll every open pull request of the User (§9.2)
    Poll(PollArgs),
    /// Rebase the Lead Branch onto the base and push (§9.3)
    Rebase(Args),
}

#[derive(clap::Args, Debug)]
pub struct Args {
    #[command(flatten)]
    pr: PrArg,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct PollArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// The PR key, chosen by the Lead
    #[arg(value_name = "key")]
    key: PrKey,
    /// The PR's repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: PathBuf,
    /// The branch the pull request targets [default: origin's HEAD branch, else main]
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// PRs of the Plan that must be merged before this one starts, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',')]
    needs: Vec<PrKey>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct EditArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    /// The new repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: Option<PathBuf>,
    /// The new base branch
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// The new needs, replacing the list, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',', conflicts_with = "no_needs")]
    needs: Option<Vec<PrKey>>,
    /// Remove every need
    #[arg(long = "no-needs")]
    no_needs: bool,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    #[command(flatten)]
    plan: PlanArg,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Add(args) => add::add(args),
        Command::Edit(args) => edit::edit(args),
        Command::Cancel(args) => cancel::cancel(args),
        Command::Start(args) => start::run(args),
        Command::Open(args) => open::open(args),
        Command::Poll(args) => poll::poll(args),
        Command::Rebase(args) => rebase::rebase(args),
    }
}

/// What every command opens: `$CPD_HOME`, its config and the store.
struct Env {
    home: CpdHome,
    config: Config,
    store: Store,
}

impl Env {
    fn open() -> Result<Env, CpdError> {
        let home = CpdHome::from_env()?;
        let config = Config::load(&home)?;
        let store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
        Ok(Env { home, config, store })
    }

    fn plan(&mut self, id: &PlanId) -> Result<PlanRow, CpdError> {
        self.store.read(|conn| row::load(conn, id))
    }
}

/// `illegal_phase` (exit 3): `command` needs a Plan in phase `needs`.
fn illegal_phase(plan: &PlanRow, command: &str, needs: &str) -> CpdError {
    wrong_phase(&plan.id, plan.phase, command, needs)
}

/// The per-PR Gate log, `run/<plan>/logs/pr-<key>.pre-merge.log` (§7.6). The
/// contract's layout (§1.4) names this file; `CpdHome` has no method for it.
fn gate_log(home: &CpdHome, plan: &PlanId, key: &PrKey) -> PathBuf {
    home.plan_dir(plan).join("logs").join(format!("pr-{key}.pre-merge.log"))
}

fn wrong_phase(plan: &PlanId, phase: Phase, command: &str, needs: &str) -> CpdError {
    CpdError::new(ErrorCode::IllegalPhase, format!("Plan {plan} is in phase {}: {command} needs phase {needs}.", phase.as_str()))
        .with("plan", plan.as_str())
        .with("phase", phase.as_str())
        .with("needs", needs)
}

/// `pr_locked` (exit 3): the Plan is past the phases where its PRs change (§3.8).
fn pr_locked(plan: &PlanId, phase: Phase, command: &str) -> CpdError {
    CpdError::new(
        ErrorCode::PrLocked,
        format!("Plan {plan} is in phase {}: {command} needs phase grounding or plan, and after the lock its PRs are Locked.", phase.as_str()),
    )
    .with("plan", plan.as_str())
    .with("phase", phase.as_str())
}

/// The Plan's phase, else `plan_not_found`.
fn plan_phase(conn: &rusqlite::Connection, plan: &PlanId) -> Result<Phase, CpdError> {
    use rusqlite::OptionalExtension;
    let phase: String = conn
        .query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))
        .optional()?
        .ok_or_else(|| CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str()))?;
    Phase::parse(&phase)
        .ok_or_else(|| CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: Plan {plan} has phase {phase:?}.")))
}

/// The PR `key` among `prs`, else `pr_not_found` (exit 4).
fn find_pr<'a>(prs: &'a [PrSpec], plan: &PlanId, key: &PrKey) -> Result<&'a PrSpec, CpdError> {
    prs.iter().find(|p| p.key == key.as_str()).ok_or_else(|| {
        CpdError::new(ErrorCode::PrNotFound, format!("No PR {key} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
    })
}

/// `needs` as the sorted keys, refusing a repeat with `usage` (§9.4).
fn sorted_needs(needs: &[PrKey]) -> Result<Vec<String>, CpdError> {
    let mut keys: Vec<String> = needs.iter().map(|k| k.as_str().to_owned()).collect();
    keys.sort();
    if let Some(pair) = keys.windows(2).find(|w| w[0] == w[1]) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--needs lists {} twice.", pair[0])).with("pr", pair[0].as_str()));
    }
    Ok(keys)
}

/// Every key of `needs` is a PR of the Plan other than `own` (`needs_unknown`)
/// and none is `cancelled` (`needs_unavailable`) (§9.4, §9.5).
fn check_needs(prs: &[PrSpec], plan: &PlanId, own: Option<&PrKey>, needs: &[String]) -> Result<(), CpdError> {
    let unknown: Vec<String> =
        needs.iter().filter(|n| own.is_some_and(|o| o.as_str() == n.as_str()) || !prs.iter().any(|p| p.key == **n)).cloned().collect();
    if !unknown.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnknown, format!("Plan {plan} has no PR {} to need.", unknown.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unknown));
    }
    let unavailable: Vec<String> = needs.iter().filter(|n| prs.iter().any(|p| p.key == **n && p.status == "cancelled")).cloned().collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, format!("A PR cannot need the cancelled PR {}.", unavailable.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unavailable));
    }
    Ok(())
}

/// Takes the per-repo lock of the PR's repo, waiting at most `lock_wait_seconds`
/// (`lock_timeout`), then runs R for the repo's orphaned merges (§7.4), so that
/// nothing is gated or pushed that holds a merge the Ledger has not recorded.
/// `source` is the command, for what R writes.
fn lock(env: &mut Env, pr: &PrRow, source: &str) -> Result<RepoLock, CpdError> {
    let limit = Duration::from_secs(u64::from(env.config.timeouts.lock_wait_seconds));
    let lock = RepoLock::wait_for(&env.home, &pr.repo, limit)?;
    let repo = pr.repo.to_string_lossy().into_owned();
    let merging = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.plan, a.id FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
               JOIN prs r ON r.plan = j.plan AND r.key = j.pr
              WHERE a.status = 'merging' AND r.repo = ?1",
        )?;
        let rows = stmt.query_map([repo], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    // An Attempt whose driver is alive is that driver's to finish; only a free driver lock is an orphan.
    let mut orphans = false;
    for (plan, attempt) in merging {
        let (plan, attempt) = (PlanId::parse(&plan)?, AttemptId::parse(&attempt)?);
        orphans |= merge::take_free_driver_lock(&env.home, &plan, &attempt)?.is_some();
    }
    // R needs pi's catalog only to pick the next Tier of an Attempt it ends `crashed`.
    if orphans {
        let catalog = config::load_catalog(&env.config)?;
        let ctx = merge::Ctx { home: &env.home, config: &env.config, catalog: &catalog, source, metrics: &|_| None };
        merge::recover(&mut env.store, &ctx, &lock)?;
    }
    Ok(lock)
}

/// The Lead worktree must be clean (`dirty_target`, exit 3).
fn require_clean(plan: &PlanId, pr: &PrRow, lead: &Path) -> Result<(), CpdError> {
    let status = git::run::ok(lead, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::DirtyTarget,
        format!("The Lead worktree {} has uncommitted changes: commit or discard them first.", lead.display()),
    )
    .with("plan", plan.as_str())
    .with("pr", pr.key.as_str())
    .with("lead_worktree", lead.display().to_string())
    .with("files", status.lines().map(|l| l.trim().to_owned()).collect::<Vec<_>>()))
}

/// The Gate (§7.3, §7.6): `pre-merge` from the PR's Lead worktree's `.config/wt.toml`,
/// run there on the Lead Branch. `again` is the command the Lead runs after fixing it.
/// A failure is `gate_failed` (exit 3), an error `gate_error` (exit 1), both with
/// the last 60 lines in `details.tail` and the full log in `details.log`.
fn gate(env: &Env, plan: &PlanId, pr: &PrRow, lead: &Path, again: &str) -> Result<(), CpdError> {
    let log = gate_log(&env.home, plan, &pr.key);
    let timeout = Duration::from_secs(u64::from(env.config.timeouts.gate_minutes) * 60);
    let operation = Operation { branch: &pr.base };
    let result = run_hook(Hook::PreMerge, lead, Some(&operation), timeout, &log)?;
    let tail = result.tail.unwrap_or_default();
    let log = log.display().to_string();
    match result.outcome {
        HookOutcome::Passed => Ok(()),
        HookOutcome::Failed { exit } => Err(CpdError::new(
            ErrorCode::GateFailed,
            format!(
                "The Gate failed on {} (exit {exit}): nothing was pushed. Append fix Jobs (cpd job add), then run cpd {again} again once they are Merged.",
                pr.lead_branch
            ),
        )
        .with("plan", plan.as_str())
        .with("pr", pr.key.as_str())
        .with("exit", exit)
        .with("tail", tail)
        .with("log", log)),
        HookOutcome::Error(why) => Err(CpdError::new(
            ErrorCode::GateError,
            format!("The Gate could not be judged on {}: {why}. Nothing was pushed.", pr.lead_branch),
        )
        .with("plan", plan.as_str())
        .with("pr", pr.key.as_str())
        .with("reason", why.to_string())
        .with("tail", tail)
        .with("log", log)),
    }
}
