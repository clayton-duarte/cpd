//! Pull requests (contract §7.6; commands.md §9), through `gh`:
//!
//! - `open`: the Gate, the push and the PR (`pr open`);
//! - `poll`, with `feedback`: the PR's checks, reviews, comments and merge (`pr poll`);
//! - `rebase`: the Lead Branch onto the base, the Gate and the push (`pr rebase`);
//! - `gh`: the one gateway to `gh`; `row`: the Plan as these commands read it.
//!
//! `pr open` and `pr rebase` run under the per-repo lock, after R (§7.4).

mod feedback;
mod gh;
mod open;
mod poll;
mod rebase;
mod row;

use std::path::Path;
use std::time::Duration;

use crate::args::PlanArg;
use crate::config::{self, Config};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, HookOutcome, Operation, run_hook};
use crate::home::{CpdHome, Hook};
use crate::ids::{AttemptId, PlanId};
use crate::merge::{self, RepoLock};
use crate::output::Output;
use crate::store::Store;
use row::PlanRow;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Push the Lead Branch and open the PR (§9.1)
    Open(Args),
    /// Poll the PR for feedback and the merge (§9.2)
    Poll(Args),
    /// Rebase the Lead Branch onto the base and push (§9.3)
    Rebase(Args),
}

#[derive(clap::Args, Debug)]
pub struct Args {
    #[command(flatten)]
    plan: PlanArg,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
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
    CpdError::new(
        ErrorCode::IllegalPhase,
        format!("Plan {} is in phase {}: {command} needs phase {needs}.", plan.id, plan.phase.as_str()),
    )
    .with("plan", plan.id.as_str())
    .with("phase", plan.phase.as_str())
    .with("needs", needs)
}

/// Takes the per-repo lock, waiting at most `lock_wait_seconds` (`lock_timeout`),
/// then runs R for the repo's orphaned merges (§7.4), so that nothing is gated
/// or pushed that holds a merge the Ledger has not recorded. `source` is the
/// command, for what R writes.
fn lock(env: &mut Env, plan: &PlanRow, source: &str) -> Result<RepoLock, CpdError> {
    let limit = Duration::from_secs(u64::from(env.config.timeouts.lock_wait_seconds));
    let lock = RepoLock::wait_for(&env.home, &plan.repo, limit)?;
    let repo = plan.repo.to_string_lossy().into_owned();
    let merging = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.plan, a.id FROM attempts a JOIN plans p ON p.id = a.plan WHERE a.status = 'merging' AND p.repo = ?1",
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
fn require_clean(plan: &PlanRow, lead: &Path) -> Result<(), CpdError> {
    let status = git::run::ok(lead, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::DirtyTarget,
        format!("The Lead worktree {} has uncommitted changes: commit or discard them first.", lead.display()),
    )
    .with("plan", plan.id.as_str())
    .with("lead_worktree", lead.display().to_string())
    .with("files", status.lines().map(|l| l.trim().to_owned()).collect::<Vec<_>>()))
}

/// The Gate (§7.3, §7.6): `pre-merge` from the Lead worktree's `.config/wt.toml`,
/// run there on the Lead Branch. `again` is the command the Lead runs after fixing it.
/// A failure is `gate_failed` (exit 3), an error `gate_error` (exit 1), both with
/// the last 60 lines in `details.tail` and the full log in `details.log`.
fn gate(env: &Env, plan: &PlanRow, lead: &Path, again: &str) -> Result<(), CpdError> {
    // The contract's layout (§1.4) names this file; `CpdHome` has no method for it.
    let log = env.home.plan_dir(&plan.id).join("logs").join("pr.pre-merge.log");
    let timeout = Duration::from_secs(u64::from(env.config.timeouts.gate_minutes) * 60);
    let operation = Operation { branch: &plan.base };
    let result = run_hook(Hook::PreMerge, lead, Some(&operation), timeout, &log)?;
    let tail = result.tail.unwrap_or_default();
    let log = log.display().to_string();
    match result.outcome {
        HookOutcome::Passed => Ok(()),
        HookOutcome::Failed { exit } => Err(CpdError::new(
            ErrorCode::GateFailed,
            format!(
                "The Gate failed on {} (exit {exit}): nothing was pushed. Append fix Jobs (cpd2 job add), then run cpd2 {again} again once they are Merged.",
                plan.lead_branch
            ),
        )
        .with("plan", plan.id.as_str())
        .with("exit", exit)
        .with("tail", tail)
        .with("log", log)),
        HookOutcome::Error(why) => Err(CpdError::new(
            ErrorCode::GateError,
            format!("The Gate could not be judged on {}: {why}. Nothing was pushed.", plan.lead_branch),
        )
        .with("plan", plan.id.as_str())
        .with("reason", why.to_string())
        .with("tail", tail)
        .with("log", log)),
    }
}
