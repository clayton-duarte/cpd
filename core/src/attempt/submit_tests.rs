//! Red tests for `attempt submit` from a linear Attempt's workflow worktree (contract §7.3, commands.md §6.1,
//! hs-d680/hs-d682/hs-d683): the Attempt's worktree is the PR's workflow worktree and its branch the Lead
//! Branch, so `wrong_worktree` accepts a cwd inside the workflow worktree. Red until `wlinear-impl` gives the
//! starter the workflow worktree and Lead Branch.

#[test]
fn a_linear_attempt_submits_from_the_workflow_worktree() {
    todo!("wlinear-impl")
}
