//! The tmux host (contract §6.1–§6.5, with commands.md §19 amendments 1, 2, 8
//! and 9): the server, sessions, windows and panes, the typed tmux wrapper,
//! and how cpd starts pi. A library: the commands that use it are other areas'.
//!
//! - [`tmux`]: `tmux -L <socket>` calls (§6.1, §6.3);
//! - [`addr`]: the review host (§6.5);
//! - [`deadline`]: the one subprocess runner with a deadline (contract §1.2 `tool_timeout`);
//! - [`git_guard`]: the `git` wrapper that refuses `git stash` and the `PATH` that puts it first (§6.5, hs-d310);
//! - [`exe`]: the running executable and the reload after an install (§6.7);
//! - [`layout`]: window names, `@cpd_*` options and the chat slot (§6.2);
//! - [`pi`]: pi's argv and environment per role, and the `[-e …]` rule (§6.5).

pub mod addr;
pub mod deadline;
pub mod exe;
pub mod git_guard;
pub mod layout;
pub mod pi;
pub mod tmux;

pub use layout::{View, chat_slot, session_pane, view_pane};
pub use tmux::{Pane, PaneId, Spawn, Split, Tmux, WindowId};
