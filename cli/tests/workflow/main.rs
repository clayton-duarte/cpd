//! The `workflow` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod git;
mod workflow_live_spec;
mod git_guard;
mod git_lead_branch;
mod merge;
mod merge_durable;
mod workflow_adopt_add;
mod workflow_adopt_life;
mod workflow_adopt_pub;
mod workflow_adopt_rebase;
mod workflow_lead_branch;
mod pr_checks;
mod pr_newest_run;
mod pr_comments;
mod pr_view_feedback;
mod workflow_crud;
mod workflow_describe;
mod pr_human_approval;
mod pr_human_gated;
mod pr_excused;
mod workflow_jira;
mod workflow_jira_queue;
mod workflow_local_needs;
mod pr_merge;
mod workflow_merge_drafts;
mod workflow_needs_required;
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
mod workflow;
mod workflow_start;
mod workflow_template_jobs;
mod pr_stacked_ready;
mod pr_view;
mod repo_hooks;
