//! The `store` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod events;
mod home;
mod store_pr_posts;
mod scripted;
mod store;
mod store_local_needs;
mod store_asleep;
mod store_window;
