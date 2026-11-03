//! The `session` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod agent_key_guards;
mod board;
mod host;
mod host_pill;
mod mcp_attach;
mod session;
mod session_asleep;
mod session_first_message;
mod session_list_archived;
mod session_reload;
mod session_report;
mod session_resume;
mod session_retier;
mod session_retier_tier;
mod session_send;
mod session_switch;
mod session_titles;
mod session_titles_glance;
mod session_watchdog;
