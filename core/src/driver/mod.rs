//! The Attempt driver (contract §3.4): `attempt run` (A1–A9), the driver lock
//! and its hand-over, and `job dispatch` (commands.md §5.5, §6.4).
//!
//! - [`start`]: the starter that `job dispatch` and the engine's E3 share;
//! - `lock`: the driver lock, taken by the starter and inherited as fd 3;
//! - `run`: the driver itself, A1–A9;
//! - `worker`: the Worker Spec, the Worker's pane, its session file;
//! - [`schedule`]: E3's queue walk, a pure function;
//! - `stall`: A4's stall rule, the kill of a Worker's idle tool call.

pub mod lock;
#[cfg(test)]
pub(crate) mod ports_tests;
mod run;
pub mod schedule;
#[cfg(test)]
mod schedule_tests;
pub(crate) mod stall;
mod start;
pub mod worker;

use serde_json::json;

use crate::args::PlanArg;
use crate::config::load_catalog;
use crate::env::Env;
use crate::error::CpdError;
use crate::ids::{AttemptId, JobKey};
use crate::output::Output;
use crate::ports::Ports;

pub use start::{Allocation, Origin, StartCtx, Started, pr_has_live_linear, start, start_with, start_with_allocation};

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
    let Env { home, config, mut store, .. } = Env::open()?;
    let models = store.read(|conn| crate::policy::model::models_of_job(conn, &home, &plan, args.key.as_str()))?;
    let catalog = load_catalog(&config, &models)?;
    let ctx = StartCtx { home: &home, config: &config, catalog: &catalog, command: "job dispatch" };
    // A manual dispatch asks for the linear Attempt; the starter refuses it (CasLost) when a sibling
    // of the PR is live, so two Attempts never share the workflow worktree (hs-d680/hs-d682/hs-d683).
    let started = start_with_allocation(&Ports::current(), &mut store, &ctx, &plan, &args.key, Some(Allocation::Linear))?;
    Ok(Output::success(json!({
        "attempt": started.attempt,
        "tier": started.tier.to_string(),
        "origin": started.origin.as_str(),
        "pid": started.pid,
    })))
}
