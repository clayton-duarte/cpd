//! git (contract §7.1–§7.3, §7.5): the git wrapper, worktrees and branches,
//! the hooks of `.config/wt.toml`, and the tamper check. No commands of its
//! own: `plan lock`, `attempt submit`, the driver and the merge call it.

pub mod gate;
pub mod hooks;
pub mod lead;
pub mod run;
pub mod snapshot;
pub mod tamper;
pub mod worktree;

pub use hooks::{HookError, HookOutcome, HookResult, Log, Operation, Pipeline, run_hook, run_hook_as};
pub use lead::{Synced, sync_lead};
pub use snapshot::{GateHooks, GateWorktree, Refused, gate, gate_worktree_path, sweep as sweep_gate_worktrees};
pub use tamper::{check as tamper_check, protected_list, repo_tests, touched_protected};
pub use worktree::{
    Created, Kept, LeadRemoval, Prepared, Push, Remote, Setup, StartFailure, Step, attempt_branch, attempt_worktree_path, create_attempt_worktree,
    create_lead_worktree, discard_worktree, lead_branch, lead_worktree_path, merge_conflicts, pr_lead_worktree_path, prepare_adopted_lead_worktree, prepare_lead_worktree,
    push_commit, push_lead_branch, remove_merged_attempt, remove_merged_lead,
};

/// The last `lines` lines of `text`, joined with `\n` (contract §7.3: results
/// carry the last 60 lines of a log).
pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// How many lines of output results carry (contract §7.3, commands.md §1.2).
pub const TAIL_LINES: usize = 60;
