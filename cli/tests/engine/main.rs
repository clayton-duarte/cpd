//! The `engine` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod doctor;
mod watch_distill_inbox;
mod watch_inbox;
mod watch_sleep;
mod watch;
mod watch_worker_archive;
mod watch_team;
mod watch_titles_width;
