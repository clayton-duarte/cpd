//! The cockpit (contract §6.1, §6.2; commands.md §13, §17, with §19
//! amendments 1, 2, 9 and 10): `cpd tui`, which builds and attaches it, and
//! the ratatui views of `cpd tui view`, one module per view.
//!
//! - [`cockpit`]: `cpd tui [--attach] [--respawn]` (§13.1);
//! - [`chat`]: the chat slot's placeholder (amendment 2);
//! - [`widgets`]: the Dispatch column and the key legend (`tui view legend`), with the shared [`palette`];
//! - [`logs`]: the Event log, a popup of the StatusBar (R36);
//! - [`statusbar`]: the StatusBar, the bottom row, with reset-layout;
//! - [`history`]: an archived Session's pi file, read-only, in the chat slot;
//! - [`tree`], [`plan`], [`worker`]: the other views of §13.2.

mod chat;
mod cockpit;
mod history;
pub mod link;
pub mod logs;
pub mod palette;
mod plan;
pub mod read;
mod statusbar;
mod tree;
pub mod widgets;
mod worker;

use std::ffi::OsString;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;

/// Runs `cpd <argv>` in this process, through the root command tree. `cli` owns that tree, so it passes the runner in.
pub type RunCommand = fn(Vec<OsString>) -> Result<Output, CpdError>;

/// `cpd tui [--attach] [--respawn]`, or `cpd tui view …`.
#[derive(clap::Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Then attach this terminal to the cockpit (`tmux -L <socket> attach -t <cockpit>`)
    #[arg(long)]
    pub attach: bool,
    /// Restart every live view of the cockpit, so each runs the installed binary again
    #[arg(long)]
    pub respawn: bool,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// A view in a cockpit pane (internal; owns its terminal) (§13.2)
    #[command(hide = true, subcommand)]
    View(View),
}

/// The views of commands.md §13.2, named as their `@cpd_view` (contract §6.2).
#[derive(clap::Subcommand, Debug)]
pub enum View {
    /// The session tree (left)
    Tree,
    /// The Plan of the Lead in the chat slot (centre top)
    Plan,
    /// The Dispatch column (right)
    Widgets,
    /// The StatusBar (bottom row)
    Statusbar,
    /// An Attempt's Worker, read-only (beside the chat)
    Worker(worker::Args),
    /// An archived Session's history, read-only (the chat slot)
    History(history::Args),
    /// The key legend (opened by `?` in the Dispatch column, in a tmux popup)
    Legend,
    /// The Event log (opened by the StatusBar's logs button and by an Inbox item, in a tmux popup)
    Logs(logs::Args),
    /// The chat slot's placeholder
    Chat,
}

pub fn run(args: Args, run_command: RunCommand) -> Result<Output, CpdError> {
    match args.command {
        None => cockpit::run(args.attach, args.respawn, run_command),
        Some(Command::View(view)) => match view {
            View::Tree => tree::run(),
            View::Plan => plan::run(run_command),
            View::Widgets => widgets::run(run_command),
            View::Statusbar => statusbar::run(),
            View::Worker(args) => worker::run(args),
            View::History(args) => history::run(args),
            View::Legend => widgets::legend::run(),
            View::Logs(args) => logs::run(args),
            View::Chat => chat::run(),
        },
    }
}

/// `not_a_terminal`: a command that owns its terminal was not given one.
fn not_a_terminal(command: &str) -> CpdError {
    CpdError::new(ErrorCode::NotATerminal, format!("cpd {command} needs a terminal on stdin and stdout."))
        .with("command", command)
}

/// Whether stdin and stdout are both a terminal.
fn on_a_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}
