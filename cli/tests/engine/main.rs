//! The `engine` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod doctor;
mod watch;
mod watch_auto_merge;
mod watch_curate_docs_gone;
mod watch_curate_inbox;
mod watch_distill_inbox;
mod watch_durable;
mod watch_host_alert;
mod watch_inbox_models;
mod watch_inbox_overdue;
mod watch_models;
mod watch_own_post;
mod watch_parked;
mod watch_parked_inbox;
mod watch_permanent;
mod watch_resources;
mod watch_review_archive;
mod watch_sleep;
mod watch_stand_down;
mod watch_team;
mod watch_titles_width;
mod watch_worker_archive;
mod write_executable;
