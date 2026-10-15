//! The cockpit (contract §6.2; commands.md §13, §17): `cpd2 tui` and the
//! ratatui views of `cpd2 tui view`. Filled in by the TUI Job, which also adds
//! the TUI's dependencies (ratatui, crossterm).

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

/// `cpd2 tui [--attach]`, or `cpd2 tui view …`.
#[derive(clap::Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub tui: Pending,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// A view in a cockpit pane (internal; owns its terminal) (§13.2)
    #[command(hide = true)]
    View(Pending),
}

pub fn run(args: Args) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match args.command {
        Some(Command::View(_)) => "tui view",
        None => "tui",
    }))
}
