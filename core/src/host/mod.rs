//! The tmux host (contract §6.1–§6.5, with commands.md §19 amendments 1, 2, 8
//! and 9): the server, sessions, windows and panes, the typed tmux wrapper,
//! and how cpd starts pi. A library: the commands that use it are other areas'.
//!
//! - [`tmux`]: `tmux -L <socket>` calls (§6.1, §6.3);
//! - [`deadline`]: the one subprocess runner with a deadline (contract §1.2 `tool_timeout`);
//! - [`git_guard`]: the `git` wrapper that refuses `git stash` and the `PATH` that puts it first (§6.5, hs-d310);
//! - [`install_state`]: the install state file `$CPD_HOME/run/cpd-install.json` (§1.4, §3.7 E6, hs-d618);
//! - [`exe`]: the running executable and the reload after an install (§6.7);
//! - [`limit`]: the test overrides of fixed limits and polls (contract §1.2, debug builds only);
//! - [`input`]: the keyboard of a command that owns its terminal, scoped by a test that runs it in-process;
//! - [`layout`]: window names, `@cpd_*` options and the chat slot (§6.2);
//! - [`priority`]: the idle priority prefix of an Attempt's processes (hs-d528);
//! - [`pi`]: pi's argv and environment per role, and the `[-e …]` rule (§6.5).

pub mod deadline;
pub mod exe;
pub mod git_guard;
pub mod host_socket;
pub mod input;
pub mod install_state;
pub mod layout;
pub mod limit;
pub mod pi;
pub mod priority;
pub mod runner;
pub mod spawn;
pub mod tmux;

#[cfg(test)]
mod exe_runner_tests;
#[cfg(test)]
mod runner_stdin_tests;
#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod spawn_tests;
#[cfg(test)]
mod tmux_adapter_tests;
#[cfg(test)]
mod tmux_runner_tests;

pub use layout::{View, chat_slot, session_pane, view_pane};
pub use tmux::{Pane, PaneId, Spawn, Split, Tmux, WindowId};
