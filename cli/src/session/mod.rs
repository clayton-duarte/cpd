//! Sessions and their tmux panes (contract §2.2, §3.6, §6; commands.md §10).
//! Filled in by Job `rs-session`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List Sessions (§10.1)
    List(Pending),
    /// Show a Session (§10.2)
    Show(Pending),
    /// Fork a Lead into a parked window (§10.3)
    Fork(Pending),
    /// Bring a Lead into the chat slot (§10.4)
    Open(Pending),
    /// Move the Lead in the chat slot to a parked window (§10.5)
    Park(Pending),
    /// Rename a Session (§10.7)
    Rename(Pending),
    /// Report a Session's agent state (pi adapter) (§10.6)
    State(Pending),
    /// The program in every Lead pane (internal; owns its terminal) (§10.8)
    #[command(hide = true)]
    Run(Pending),
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented(match command {
        Command::List(_) => "session list",
        Command::Show(_) => "session show",
        Command::Fork(_) => "session fork",
        Command::Open(_) => "session open",
        Command::Park(_) => "session park",
        Command::Rename(_) => "session rename",
        Command::State(_) => "session state",
        Command::Run(_) => "session run",
    }))
}
