//! The cockpit (contract §6.1, §6.2; commands.md §13, §17, with §19
//! amendments 1, 2, 9 and 10): `cpd2 tui`, which builds and attaches it, and
//! the ratatui views of `cpd2 tui view`, one module per view.
//!
//! - [`cockpit`]: `cpd2 tui [--attach]` (§13.1);
//! - [`chat`]: the chat slot's placeholder (amendment 2);
//! - [`tree`], [`plan`], [`widgets`], [`worker`]: the other views of §13.2,
//!   `not_implemented` until their Jobs land.

mod chat;
mod cockpit;
mod plan;
mod tree;
mod widgets;
mod worker;

use crate::cli::Pending;
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;

/// `cpd2 tui [--attach]`, or `cpd2 tui view …`.
#[derive(clap::Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Then attach this terminal to the cockpit (`tmux -L <socket> attach -t <cockpit>`)
    #[arg(long)]
    pub attach: bool,
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
    Tree(Pending),
    /// The Plan of the Lead in the chat slot (centre top)
    Plan(Pending),
    /// The Dispatch column (right)
    Widgets(Pending),
    /// An Attempt's Worker, read-only (beside the chat)
    Worker(Pending),
    /// The chat slot's placeholder
    Chat,
}

pub fn run(args: Args) -> Result<Output, CpdError> {
    match args.command {
        None => cockpit::run(args.attach),
        Some(Command::View(view)) => match view {
            View::Tree(_) => tree::run(),
            View::Plan(_) => plan::run(),
            View::Widgets(_) => widgets::run(),
            View::Worker(_) => worker::run(),
            View::Chat => chat::run(),
        },
    }
}

/// `not_a_terminal`: a command that owns its terminal was not given one.
fn not_a_terminal(command: &str) -> CpdError {
    CpdError::new(ErrorCode::NotATerminal, format!("cpd2 {command} needs a terminal on stdin and stdout."))
        .with("command", command)
}

/// Whether stdin and stdout are both a terminal.
fn on_a_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}
