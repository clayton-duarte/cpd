//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): one per
//! machine, running E1–E5 and E7–E10 (E6 was removed) every `tick_seconds` under `locks/watch.lock`.
//!
//! - [`engine`]: the loop, its signals, `watch-started` and `watch-stopped`, and E7
//!   (the PR start, which calls `pr::start`);
//! - [`distill`] E9: starts `cpd context distill` for a Context whose digest is behind;
//! - [`titles`] E10: starts `cpd session retitle` for idle Lead and Manager Sessions whose turn ended after their title;
//! - [`escalations`] E1, [`liveness`] E2, [`schedule`] E3, [`completion`] E4,
//!   [`pr`] E5, [`inbox`] E8: one module per step (E6 was removed, its number stays);
//! - [`report`]: an unexpected failure as an `error` Event, once until it clears;
//! - [`ensure`]: `watch --ensure`, which starts the engine in `cpd-work:watch`;
//! - [`lock`]: `locks/watch.lock`; [`log`]: stderr and `run/watch.log`.

mod completion;
mod distill;
mod engine;
mod ensure;
mod escalations;
mod inbox;
mod liveness;
mod lock;
mod log;
mod pr;
mod report;
mod schedule;
mod titles;

use crate::error::CpdError;
use crate::output::Output;

/// The `txns.command` of the engine's transactions and the `source` of its
/// `error` Events (commands.md §12).
const SOURCE: &str = "watch";

/// The arguments of `cpd watch` (commands.md §12).
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Start the engine in cpd-work:watch when it is not running, and return at once
    #[arg(long)]
    ensure: bool,
}

/// `cpd watch [--ensure]`.
pub fn run(args: Args) -> Result<Output, CpdError> {
    if args.ensure { ensure::ensure() } else { engine::run() }
}
