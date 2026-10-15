//! Attempts (contract §2.2, §3.4, §7.3; commands.md §6).
//!
//! - [`submit`]: `attempt submit`, the Worker's one write (§6.1);
//! - [`read`]: `attempt read`, a transcript of the Worker's session (§6.2);
//! - [`show`]: `attempt show` and `attempt list` (§6.3);
//! - [`row`]: the `attempts` and `escalations` rows as read.
//!
//! `attempt run` belongs to the driver (`crate::driver`).

mod read;
mod row;
mod show;
mod submit;

use crate::args::PlanArg;
use crate::config::Config;
use crate::driver;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey};
use crate::output::Output;
use crate::store::Store;

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
}

/// A row the database holds in a shape the binary does not accept: it was
/// changed outside cpd, or by a bug.
fn bad_row(what: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: {what}."))
}
