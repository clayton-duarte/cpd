//! The `tui` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod bare;
mod inbox;
mod inbox_rules;
mod tui_chat_pad;
mod tui_checklist;
mod tui_dialog;
mod tui_history;
mod tui_idle;
mod tui_logs;
mod tui_loop_guard;
mod tui_merge;
mod tui_plan;
mod tui_plan_cancelled;
mod tui_plan_waiting;
mod tui_pulse;
mod tui_read;
mod tui;
mod tui_runner;
mod tui_tree;
mod tui_tree_delete;
mod tui_tree_archive_manager;
mod tui_tree_asleep;
mod tui_widgets;
mod tui_titles;
mod tui_worker;
