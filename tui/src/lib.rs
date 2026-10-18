//! The cockpit (contract §6.1, §6.2; commands.md §13, §17, with §19
//! amendments 1, 2, 9 and 10): `cpd tui`, which builds and attaches it, and
//! the ratatui views of `cpd tui view`, one module per view.
//!
//! - [`cockpit`]: `cpd tui [--attach] [--respawn]` (§13.1), reset-layout and
//!   `cpd tui action`, what a click on a StatusBar button runs (the StatusBar
//!   is tmux's status line, contract §6.1);
//! - [`chat`]: the chat slot's placeholder (amendment 2);
//! - [`widgets`]: the Dispatch column and the key legend (`tui view legend`), with the shared [`palette`];
//! - [`logs`]: the Event log, a popup of the StatusBar (R36);
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
    /// A click on a StatusBar button (internal; prints nothing) (§13.2)
    #[command(hide = true)]
    Action(ActionArgs),
}

/// `cpd tui action <reset|legend|logs> --client <name>`.
#[derive(clap::Args, Debug)]
pub struct ActionArgs {
    /// `reset`, `legend` or `logs`: the button's range. Optional and free text, so that any value, an empty
    /// `#{mouse_status_range}` included, reaches the command and fails with `display-message`, never a usage error
    /// on stdout
    pub action: Option<String>,
    /// The tmux client that clicked (`#{client_name}`)
    #[arg(long)]
    pub client: String,
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
    /// An Attempt's Worker, read-only (beside the chat)
    Worker(worker::Args),
    /// An archived Session's history, read-only (the chat slot)
    History(history::Args),
    /// The key legend (opened by `?` in the Dispatch column, in a tmux popup)
    Legend,
    /// The Event log (opened by the StatusBar's logs button, in a tmux popup)
    Logs,
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
            View::Worker(args) => worker::run(args),
            View::History(args) => history::run(args),
            View::Legend => widgets::legend::run(),
            View::Logs => logs::run(),
            View::Chat => chat::run(),
        },
        Some(Command::Action(a)) => Ok(cockpit::action(a.action.as_deref(), &a.client)),
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
