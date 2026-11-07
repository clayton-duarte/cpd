//! The Attempt driver (contract §3.4): `attempt run` (A1–A9), the driver lock
//! and its hand-over, and `job dispatch` (commands.md §5.5, §6.4).
//!
//! - [`start`]: the starter that `job dispatch` and the engine's E3 share;
//! - `lock`: the driver lock, taken by the starter and inherited as fd 3;
//! - `run`: the driver itself, A1–A9;
//! - `worker`: the Worker Spec, the Worker's pane, its session file;
//! - `runner`: the seam between the two Worker runners, `pi` and `durable`;
//! - [`schedule`]: E3's queue walk, a pure function;
//! - `stall`: A4's stall rule, the kill of a Worker's idle tool call.

mod explore;
#[cfg(test)]
mod explore_tests;
pub mod lock;
#[cfg(test)]
pub(crate) mod ports_tests;
mod run;
pub mod runner;
pub mod schedule;
#[cfg(test)]
mod schedule_tests;
pub(crate) mod stall;
mod start;
#[cfg(test)]
mod start_replan_tests;
pub mod worker;

use serde_json::json;

use crate::args::PlanArg;
use crate::config::load_catalog;
use crate::env::Env;
use crate::error::CpdError;
use crate::ids::{AttemptId, JobKey};
use crate::output::Output;
use crate::ports::Ports;

pub use start::{
    Allocation, Origin, StartCtx, Started, explore_dir, start, start_with, start_with_allocation,
    workflow_has_live_linear,
};

/// The arguments of `cpd attempt run` (commands.md §6.4).
#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// The Attempt id
    #[arg(value_name = "attempt")]
    attempt: AttemptId,
    #[command(flatten)]
    plan: PlanArg,
}

/// The arguments of `cpd job dispatch` (commands.md §5.5).
#[derive(clap::Args, Debug)]
pub struct DispatchArgs {
    /// The Job key
    #[arg(value_name = "key")]
    key: JobKey,
    #[command(flatten)]
    plan: PlanArg,
}

/// `cpd attempt run` (internal): the driver, started detached with the
/// driver lock as fd 3.
pub fn run(args: RunArgs) -> Result<Output, CpdError> {
    run::run(args)
}

/// `cpd job dispatch` (commands.md §5.5): starts one Ready Job now, as E3 does.
pub fn dispatch(args: DispatchArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let models = store
        .read(|conn| crate::policy::model::models_of_job(conn, &home, &plan, args.key.as_str()))?;
    let catalog = load_catalog(&config, &models)?;
    let ctx = StartCtx {
        home: &home,
        config: &config,
        catalog: &catalog,
        command: "job dispatch",
    };
    // A manual dispatch asks for the linear Attempt; the starter refuses it (CasLost) when a sibling
    // of the Workflow is live, so two Attempts never share the Lead worktree (hs-d680/hs-d682/hs-d683).
    let started = start_with_allocation(
        &Ports::current(),
        &mut store,
        &ctx,
        &plan,
        &args.key,
        Some(Allocation::Linear),
    )?;
    Ok(Output::success(json!({
        "attempt": started.attempt,
        "tier": started.tier.to_string(),
        "origin": started.origin.as_str(),
        "pid": started.pid,
    })))
}

/// A9 for an Attempt the Lead aborted whose driver is already dead: the `attempt abort` command calls it after its
/// transaction ended the Attempt (`core/src/attempt/abort.rs`). It reuses the existing teardown, never a copy: the
/// Worker is stopped by its stored runner through `runner::kill_orphan` (the `pi` window, or SIGKILL to the `durable`
/// pid), and a parallel `pi` Attempt's worktree and branch go the way a merged end removes them; a linear Attempt
/// keeps its workflow worktree (hs-d680). The driver lock file goes last. A live driver never calls it: it consumes
/// the stop request itself (run.rs A5).
pub(crate) fn teardown_aborted(
    ports: &Ports,
    config: &crate::config::Config,
    home: &crate::home::CpdHome,
    store: &mut crate::store::Store,
    attempt: &crate::attempt::ledger::Attempt,
) -> Result<(), CpdError> {
    let tmux = crate::host::tmux::Tmux::new(config.tmux.socket.as_str());
    let (runner, _pid) = store.read(|conn| runner::stored(conn, &attempt.plan, &attempt.id))?;
    let killed = runner::kill_orphan(
        ports,
        config,
        home,
        &tmux,
        store,
        &attempt.plan,
        &attempt.id,
    );
    let removed = match runner {
        // An explore Attempt has no worktree and no branch to remove (A9, explore variant; contract §3.4 "An explore
        // Job's Attempt"): its Worktree is the explore directory, which stays.
        crate::config::WorkerRunner::Pi
            if !crate::plan::jobs::is_explore(&attempt.kind)
                && !crate::git::worktree::is_linear(&attempt.branch, &attempt.lead_branch) =>
        {
            crate::git::remove_merged_attempt(
                &attempt.lead_worktree,
                &attempt.worktree,
                &attempt.branch,
            )
        }
        _ => Ok(()),
    };
    let unlocked = lock::remove(home, &attempt.plan, &attempt.id);
    killed.and(removed).and(unlocked)
}
