//! git (contract §7.1–§7.3, §7.5): the git wrapper, worktrees and branches,
//! the hooks (the Workflow's policy `hooks` domain), and the tamper check. No commands of its
//! own: `plan lock`, `attempt submit`, the driver and the merge call it.

pub mod hooks;
pub mod lead;
pub mod run;
pub mod tamper;
pub mod worktree;

#[cfg(test)]
mod runner_tests;

pub use hooks::{
    HookError, HookOutcome, HookResult, Log, Operation, Pipeline, run_hook, run_hook_as,
    run_hook_with,
};
pub use lead::{Synced, sync_lead};
pub use tamper::{check as tamper_check, touched_protected};
pub use worktree::{
    Created, Cut, Kept, LeadRemoval, Prepared, Push, RebaseWorktree, Remote, Setup, StartFailure, Step, attempt_branch, attempt_worktree_path, create_attempt_worktree,
    create_attempt_worktree_with, create_lead_worktree, create_lead_worktree_with, prepare_workflow_lead_worktree, discard_worktree, gate_worktree_path, legacy_lead_branch, lead_worktree_path, merge_conflicts, workflow_lead_worktree_path, prepare_lead_worktree,
    push_commit, push_commit_with, push_lead_branch, remove_merged_attempt, remove_merged_lead,
};
pub use worktree::merge_conflicts_with;

/// The last `lines` lines of `text`, joined with `\n` (contract §7.3: results
/// carry the last 60 lines of a log).
pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// How many lines of output results carry (contract §7.3, commands.md §1.2).
pub const TAIL_LINES: usize = 60;
