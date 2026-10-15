//! Jobs, Specs and Dependencies (contract §2.2, §3.2; commands.md §5).
//! Filled in by Job `rs-job`; `job dispatch` belongs to the driver (`crate::driver`).

use crate::cli::Pending;
use crate::driver;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Add a Job (§5.1)
    Add(Pending),
    /// Edit a draft Job (§5.2)
    Edit(Pending),
    /// Cancel a Job (§5.3)
    Cancel(Pending),
    /// Queue a failed Job again at a Tier (§5.4)
    Retry(Pending),
    /// Start one Ready Job now (§5.5)
    Dispatch(driver::DispatchArgs),
    /// List the Jobs (§5.6)
    List(Pending),
    /// Show a Job and its Attempts (§5.7)
    Show(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    let name = match command {
        Command::Dispatch(args) => return driver::dispatch(args),
        Command::Add(_) => "job add",
        Command::Edit(_) => "job edit",
        Command::Cancel(_) => "job cancel",
        Command::Retry(_) => "job retry",
        Command::List(_) => "job list",
        Command::Show(_) => "job show",
    };
    Err(CpdError::not_implemented(name))
}
