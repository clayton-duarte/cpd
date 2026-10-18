//! The Attempt driver (contract §3.4): `attempt run` (A1–A9), the driver lock
//! and its hand-over, and `job dispatch` (commands.md §5.5, §6.4).
//!
//! - [`start`]: the starter that `job dispatch` and the engine's E3 share;
//! - `lock`: the driver lock, taken by the starter and inherited as fd 3;
//! - `run`: the driver itself, A1–A9;
//! - `ledger`: the Attempt as the driver reads it, and the ends before a merge;
//! - `worker`: the Worker Spec, the Worker's pane, its session file;
//! - `stall`: A4's stall rule, the kill of a Worker's idle tool call.

pub(crate) mod ledger;
pub(crate) mod lock;
mod run;
mod stall;
mod start;
pub(crate) mod worker;

use serde_json::json;

use crate::args::PlanArg;
use crate::config::{Config, load_catalog};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey};
use crate::output::Output;
use crate::store::Store;

pub use start::{Origin, StartCtx, Started, start};

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
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let catalog = load_catalog(&config)?;
    let ctx = StartCtx { home: &home, config: &config, catalog: &catalog, command: "job dispatch" };
    let started = start(&mut store, &ctx, &plan, &args.key)?;
    Ok(Output::success(json!({
        "attempt": started.attempt,
        "tier": started.tier.to_string(),
        "origin": started.origin.as_str(),
        "pid": started.pid,
    })))
}
