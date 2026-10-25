//! Red tests for a linear Attempt's merge (contract §7.4, hs-d680/hs-d682/hs-d683): its branch is the
//! PR's Lead Branch, so the merge diffs `base_sha...submitted_sha` for the tamper check, skips the rebase
//! and the fast-forward, keeps rule P's push and runs the Gate in the workflow worktree, and records
//! `merged_sha = submitted_sha`. R recovers a linear Attempt in `merging` whose `submitted_sha` is an
//! ancestor of the Lead Branch tip with `merged_sha = submitted_sha`. Red until `wlinear-impl` teaches the
//! merge and R the linear case.

#[test]
fn a_linear_merge_uses_the_base_sha_for_the_tamper_check() {
    todo!("wlinear-impl")
}

#[test]
fn a_linear_merge_records_the_submitted_commit() {
    todo!("wlinear-impl")
}

#[test]
fn r_recovers_a_linear_merging_attempt() {
    todo!("wlinear-impl")
}
