//! The `pr` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod git;
mod pr_live_spec;
mod git_guard;
mod git_lead_branch;
mod merge;
mod merge_durable;
mod pr_adopt_add;
mod pr_adopt_life;
mod pr_adopt_pub;
mod pr_adopt_rebase;
mod pr_lead_branch;
mod pr_checks;
mod pr_newest_run;
mod pr_comments;
mod pr_view_feedback;
mod pr_crud;
mod pr_describe;
mod pr_human_approval;
mod pr_human_gated;
mod pr_excused;
mod pr_jira;
mod pr_jira_queue;
mod pr_local_needs;
mod pr_merge;
mod pr_merge_drafts;
mod pr_needs_required;
mod pr_open;
mod pr_open_jira;
mod pr_post;
mod pr_post_auto_merge;
mod pr_post_flow;
mod pr_push;
mod pr_reaction_queue;
mod pr_rebase_running;
mod pr_reply;
mod pr_rerun;
mod pr_review;
mod pr;
mod pr_start;
mod pr_template_jobs;
mod pr_stacked_ready;
mod pr_view;
mod repo_hooks;
