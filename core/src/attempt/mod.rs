//! Attempts (contract §2.2, §3.4, §7.3; commands.md §6).
//!
//! - [`submit`]: `attempt submit`, the Worker's one write (§6.1);
//! - [`read`]: `attempt read`, a transcript of the Worker's session (§6.2);
//! - [`show`]: `attempt show` and `attempt list` (§6.3);
//! - [`row`]: the `attempts` and `escalations` rows as read;
//! - `ledger`: the Attempt as the driver and the merge read it, and the transactions that end one.
//!
//! `attempt run` belongs to the driver (`crate::driver`).

mod kind;
pub mod ledger;
mod read;
mod row;
mod show;
mod span;
mod submit;

#[cfg(test)]
mod cancel_tests;

#[cfg(test)]
mod incident_tests;

#[cfg(test)]
mod ladder_tests;

#[cfg(test)]
pub(crate) mod explore_tests;

#[cfg(test)]
mod submit_tests;

use crate::args::PlanArg;
use crate::driver;
use crate::env::Env;
use crate::error::CpdError;
use crate::ids::{AttemptId, JobKey};
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Submit the Attempt for review (Worker) (§6.1)
    Submit(SubmitArgs),
    /// A readable transcript of the Worker (§6.2)
    Read(ReadArgs),
    /// Show an Attempt (§6.3)
    Show(ShowArgs),
    /// List Attempts (§6.3)
    List(ListArgs),
    /// The Attempt driver (internal) (§6.4)
    #[command(hide = true)]
    Run(driver::RunArgs),
}

#[derive(clap::Args, Debug)]
pub struct SubmitArgs {
    /// The Attempt id
    #[arg(value_name = "attempt")]
    attempt: AttemptId,
    /// An explore Attempt's summary (at most 1500 characters)
    #[arg(long, value_name = "path")]
    summary_file: Option<std::path::PathBuf>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ReadArgs {
    /// The Attempt id
    #[arg(value_name = "attempt")]
    attempt: AttemptId,
    /// Keep the last n lines
    #[arg(long, value_name = "n", default_value_t = 40, value_parser = clap::value_parser!(u32).range(1..))]
    lines: u32,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The Attempt id
    #[arg(value_name = "attempt")]
    attempt: AttemptId,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only the Attempts of this Job
    #[arg(long, value_name = "key")]
    job: Option<JobKey>,
    /// Only the Attempts that have not ended
    #[arg(long)]
    active: bool,
    #[command(flatten)]
    plan: PlanArg,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Submit(args) => submit::submit(args),
        Command::Read(args) => read::read(args),
        Command::Show(args) => show::show(args),
        Command::List(args) => show::list(args),
        Command::Run(args) => driver::run(args),
    }
}

