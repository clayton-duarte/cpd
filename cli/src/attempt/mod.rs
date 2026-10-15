//! Attempts (contract §2.2, §3.4; commands.md §6).
//! Filled in by Job `rs-attempt-cmds`; `attempt run` belongs to the driver (`crate::driver`).

use crate::cli::Pending;
use crate::driver;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Submit the Attempt for review (Worker) (§6.1)
    Submit(Pending),
    /// A readable transcript of the Worker or Reviewer (§6.2)
    Read(Pending),
    /// Show an Attempt (§6.3)
    Show(Pending),
    /// List Attempts (§6.3)
    List(Pending),
    /// The Attempt driver (internal) (§6.4)
    #[command(hide = true)]
    Run(driver::RunArgs),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    let name = match command {
        Command::Run(args) => return driver::run(args),
        Command::Submit(_) => "attempt submit",
        Command::Read(_) => "attempt read",
        Command::Show(_) => "attempt show",
        Command::List(_) => "attempt list",
    };
    Err(CpdError::not_implemented(name))
}
