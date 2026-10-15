//! Events (contract §5; commands.md §11): appending them, `events list` and
//! `events follow`. Filled in by Job `rs-events`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List Events (§11.1)
    List(Pending),
    /// Follow Events, one per line (the one streaming command) (§11.2)
    Follow(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match command {
        Command::List(_) => "events list",
        Command::Follow(_) => "events follow",
    }))
}
