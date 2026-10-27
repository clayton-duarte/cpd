//! The `pr` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod git;
mod git_guard;
mod merge;
mod pr_adopt_add;
mod pr_adopt_life;
mod pr_adopt_pub;
mod pr_adopt_rebase;
mod pr_checks;
mod pr_comments;
mod pr_view_feedback;
mod pr_crud;
mod pr_describe;
mod pr_local_needs;
mod pr_merge;
mod pr_needs_required;
mod pr_open;
mod pr_post;
mod pr_post_flow;
mod pr_push;
mod pr_reply;
mod pr;
mod pr_start;
mod pr_view;
mod repo_hooks;
