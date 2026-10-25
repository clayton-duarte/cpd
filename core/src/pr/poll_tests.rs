//! Red tests for `pr poll`'s clean-up of a merged PR (contract §7.2, hs-d680/hs-d682/hs-d683): it removes
//! the PR's workflow worktree (and its Lead Branch) once, and each parallel Attempt worktree and branch,
//! under the every-Job-idle check. Red until `wlinear-impl` teaches the clean-up the linear case.

#[test]
fn a_merged_pr_removes_the_workflow_worktree_once() {
    todo!("wlinear-impl")
}
