//! The cockpit (contract §6.1, §6.2; commands.md §13, §17, with §19
//! amendments 1, 2, 9 and 10): `cpd tui`, which builds and attaches it, and
//! the ratatui views of `cpd tui view`, one module per view.
//!
//! - [`cockpit`]: `cpd tui [--attach] [--respawn]` (§13.1), reset-layout and
//!   `cpd tui action`, what a click on a StatusBar button runs (the StatusBar
//!   is tmux's status line, contract §6.1) and the `merge` popup's confirm starts;
//! - [`chat`]: the chat slot's placeholder (amendment 2);
//! - [`widgets`]: the Dispatch column and the key legend (`tui view legend`), with the shared [`palette`];
//! - [`logs`]: the Event log, a popup of the StatusBar (R36);
//! - [`history`]: an archived Session's pi file, read-only, in the chat slot;
//! - [`menu`]: the right-click menu the tree and the Deploy column share;
//! - [`merge`]: the merge confirmation, a popup of the Deploy column's merge icon, and the merge it starts in the background;
//! - [`dialog`]: the dialog kit (confirm, input and scrollable text), the popups of the tree and the StatusBar;
//! - [`fold`]: the one widget every block that folds is drawn by (the tree's Contexts and archived groups, the Inbox's toggle);
//! - [`section`]: the one helper every in-pane section title is drawn by;
//! - [`tree`], [`plan`]: the other views of §13.2.

mod background;
#[cfg(test)]
mod background_tests;
pub mod box_widget;
#[cfg(test)]
mod box_widget_tests;
#[cfg(test)]
mod inbox_link_tests;
#[cfg(test)]
mod link_widget_tests;
#[cfg(test)]
mod osc_8_guard_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod guard_popup_tests;
#[cfg(test)]
mod guard_terminal_tests;
#[cfg(test)]
mod section_title_tests;
mod buffer;
mod cells;
mod chat;
mod cockpit;
mod host_click;
mod incident_click;
pub mod dialog;
pub mod fold;
pub mod history;
mod input_box;
#[cfg(test)]
mod layout_dialog_tests;
#[cfg(test)]
mod layout_section_tests;
#[cfg(test)]
mod layout_plan_tests;
#[cfg(test)]
mod layout_popup_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod layout_tree_tests;
#[cfg(test)]
mod input_box_tests;
#[cfg(test)]
mod input_box_width_tests;
pub mod link;
#[cfg(test)]
mod read_source_tests;
pub mod logs;
pub mod menu;
#[cfg(test)]
mod menu_width_tests;
#[cfg(test)]
mod failed_pill_tests;
#[cfg(test)]
mod landing_tests;
#[cfg(test)]
mod moon_tests;
#[cfg(test)]
mod phase_dot_tests;
#[cfg(test)]
mod zone_tests;
#[cfg(test)]
mod phase_dot_views_tests;
#[cfg(test)]
mod worked_on_tests;
pub mod trace;
#[cfg(test)]
mod trace_tests;
#[cfg(test)]
mod icons_tests;
pub mod merge;
pub mod pace;
pub mod pump;
#[cfg(test)]
mod pump_tests;
#[cfg(test)]
mod pump_scripted_tests;
#[cfg(test)]
mod pace_tests;
pub mod palette;
pub mod phi;
pub mod pill;
#[cfg(test)]
mod pill_tests;
#[cfg(test)]
mod updating_label;
#[cfg(test)]
mod pill_colours;
pub mod plan;
pub use plan::draw_first_frame;
pub use history::{draw_history, show_with as show_history_with};
mod popup;
pub mod read;
mod runner;
#[cfg(test)]
mod runner_tests;
mod scroll;
#[cfg(test)]
mod scroll_width_tests;
pub mod section;
#[cfg(test)]
mod section_width_tests;
mod no_notes;
mod terminal_writes;
#[cfg(test)]
mod test_text;
pub mod tree;
mod viewlog;
pub mod widgets;
pub mod view_ports;
#[cfg(test)]
mod view_ports_tests;
#[cfg(test)]
mod merge_at_once_tests;

pub use runner::{Capture, ViewError, run_view, run_view_on};

use std::ffi::OsString;

use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

/// Runs `cpd <argv>` in this process, through the root command tree. `cli` owns that tree, so it passes the runner in.
pub type RunCommand = fn(Vec<OsString>) -> Result<Output, CpdError>;

/// [`RunCommand`] on the `Ports` the caller runs on: what the merge action runs `pr merge` and `pr poll` through, so a test's
/// fake `gh` answers them.
pub type RunOn = fn(&Ports, Vec<OsString>) -> Result<Output, CpdError>;

/// The engine's restart of the tmux server onto the installed tmux (contract §6.1), which `cpd-engine` owns
/// (`cpd_engine::watch::restart`). `cpd-tui` does not depend on `cpd-engine`, so `cli` passes it in, as it passes
/// [`RunCommand`].
#[derive(Clone, Copy)]
pub struct Restart {
    /// The one line about a pending restart that failed (`restart::read`, then `Pending::describe` with the caller's `zone`); `None` when
    /// none is pending or the pending one has not failed.
    pub failed: fn(&CpdHome, &cpd_core::clock::TimeZone) -> Result<Option<String>, CpdError>,
    /// Removes the pending restart (`restart::clear`).
    pub clear: fn(&CpdHome) -> Result<(), CpdError>,
    /// Marks a restart pending, from the server's tmux to the installed one; whether it newly marked (`restart::mark`).
    pub mark: fn(&CpdHome, &str, &str) -> Result<bool, CpdError>,
    /// Restarts the server now when it is older and it is safe (`restart::restart`, logging nothing).
    pub now: fn(&CpdHome, &Config, &mut Store) -> Result<RestartOutcome, CpdError>,
}

/// What [`Restart::now`] did: `restart::Outcome`.
#[derive(Debug, PartialEq, Eq)]
pub enum RestartOutcome {
    /// There is no server, or it does not run an older tmux than the installed one: nothing to do.
    NotNeeded,
    /// Another run of the restart holds `locks/tmux-restart.lock`.
    Running,
    /// Not safe now: nothing was touched; each line says what is in the way.
    Busy(Vec<String>),
    /// The server was restarted and `reopened` Sessions are back in their places.
    Restarted { reopened: usize },
}

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
    /// A click on a StatusBar button, or the merge popup's confirm (internal; prints nothing) (§13.2)
    #[command(hide = true)]
    Action(ActionArgs),
}

/// `cpd tui action <reset|legend|logs|host|incident> --client <name>`, `cpd tui action merge <owner>/<repo>#<n> --client <name>`.
#[derive(clap::Args, Debug)]
pub struct ActionArgs {
    /// `reset`, `legend`, `logs`, `host` or `incident`: the button's range; or `merge`, the `merge` popup's confirm. Optional and free
    /// text, so that any value, an empty `#{mouse_status_range}` included, reaches the command and fails with
    /// `display-message`, never a usage error on stdout
    pub action: Option<String>,
    /// `merge`'s pull request, `<owner>/<repo>#<n>`; free text, checked by the action, for the same reason
    #[arg(value_name = "owner/repo#n")]
    pub pull: Option<String>,
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
    /// An archived Session's history, read-only (the chat slot)
    History(history::Args),
    /// The key legend (opened by `?` in the Dispatch column, in a tmux popup)
    Legend,
    /// The Event log (opened by the StatusBar's logs button and by an Inbox item, in a tmux popup)
    Logs(logs::Args),
    /// The merge confirmation of a pull request (opened by the Deploy column's merge icon, in a tmux popup)
    Merge(merge::Args),
    /// A dialog of the kit: `confirm`, `input`, `text`, `checklist` or `select` (opened by the cockpit's actions, in a tmux popup)
    Dialog(dialog::Args),
    /// The chat slot's placeholder
    Chat,
}

/// `cpd tui …` on `ports`: the views the ports reach wait through their clock and read their time from it, and the merge
/// action runs its commands through `run_on`.
pub fn run_with(ports: &Ports, args: Args, version: &str, run_command: RunCommand, run_on: RunOn, restart: Restart) -> Result<Output, CpdError> {
    match args.command {
        None => cockpit::run(args.attach, args.respawn, version, run_command, restart),
        Some(Command::View(view)) => backed_off(ports, match view {
            View::Tree => tree::run(ports),
            View::Plan => plan::run(ports, run_command),
            View::Widgets => widgets::run(ports, run_command),
            View::History(args) => history::run(ports, args),
            View::Legend => widgets::legend::run(ports),
            View::Logs(args) => logs::run(ports, args),
            View::Merge(args) => merge::run(ports, args),
            View::Dialog(args) => dialog::run(ports, args),
            View::Chat => chat::run(),
        }),
        Some(Command::Action(a)) => Ok(cockpit::action(ports, a.action.as_deref(), a.pull.as_deref(), &a.client, run_command, run_on)),
    }
}

/// A view that fails exits, and the cockpit's `pane-died` hook starts it again at once (contract §6.1), so a
/// persistent failure would be a tight loop. In a pane the failure is shown, and the view waits [`view_ports::BACKOFF`] before
/// it exits. Without a terminal no hook is involved: the error is returned at once.
fn backed_off(ports: &Ports, result: Result<Output, CpdError>) -> Result<Output, CpdError> {
    view_ports::backed_off_with(ports, on_a_terminal(), &mut std::io::stderr(), result)
}

/// `cpd` with no arguments, in a terminal (commands.md §13.3): the follow-ups of an update, then attach.
pub fn bare(version: &str, run_command: RunCommand, restart: Restart) -> Result<Output, CpdError> {
    cockpit::bare(version, run_command, restart)
}

/// `not_a_terminal`: a command that owns its terminal was not given one.
fn not_a_terminal(command: &str) -> CpdError {
    CpdError::new(ErrorCode::NotATerminal, format!("cpd {command} needs a terminal on stdin and stdout."))
        .with("command", command)
}

/// Whether stdin and stdout are both a terminal.
pub fn on_a_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}
