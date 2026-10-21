//! The `pr` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod git;
mod git_guard;
mod merge;
mod pr_cleanup;
mod pr_cleanup_cancel;
mod pr_crud;
mod pr_local_needs;
mod pr_merge;
mod pr_push;
mod pr;
mod pr_start;
mod pr_view;
