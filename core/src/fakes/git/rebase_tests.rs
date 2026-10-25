//! `rebase`, `--continue` and `--abort` of `GitRoute`.
use super::scenario::*;

#[test]
fn skeleton_rebase_is_not_made_yet() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", "1\n")]);
    let out = w.git(repo.dir(), &["rebase", "main"]);
    assert_eq!(out.code, 0);
}
