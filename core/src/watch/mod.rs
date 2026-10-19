//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): one per
//! machine, running E1–E5 and E7–E9 (E6 was removed) every `tick_seconds` under `locks/watch.lock`.
//!
//! - [`engine`]: the loop, its signals, `watch-started` and `watch-stopped`, and E7
//!   (the PR start, which calls `pr::start`);
//! - [`distill`] E9: starts `cpd context distill` for a Context whose digest is behind;
//! - [`escalations`] E1, [`liveness`] E2, [`schedule`] E3, [`completion`] E4,
//!   [`pr`] E5, [`inbox`] E8: one module per step (E6 was removed, its number stays);
//! - [`workers`]: the `@cpd_workers` step, which sets the StatusBar's `workers N/M` text;
//! - [`report`]: an unexpected failure as an `error` Event, once until it clears;
//! - [`ensure`]: `watch --ensure`, which starts the engine in `cpd-work:watch`;
//! - [`restart`]: the tmux server's restart: E3's hold while it is pending, the drain, the detached run;
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
pub mod restart;
mod schedule;
mod workers;

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
    /// Restart the tmux server onto the installed tmux (internal: the engine starts it, contract §6.1)
    #[arg(long, hide = true, conflicts_with = "ensure")]
    restart_tmux: bool,
}

/// `cpd watch [--ensure]`.
pub fn run(args: Args) -> Result<Output, CpdError> {
    if args.ensure {
        ensure::ensure()
    } else if args.restart_tmux {
        restart::detached()
    } else {
        engine::run()
    }
}
