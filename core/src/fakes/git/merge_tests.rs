//! `merge-tree --write-tree --name-only`, `merge-base` and the text three-way merge of `GitRoute`.
use super::scenario::*;

#[test]
fn skeleton_merge_tree_is_not_made_yet() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", "1\n")]);
    let out = w.git(repo.dir(), &["merge-tree", "--write-tree", "--name-only", "main", "main"]);
    assert_eq!(out.code, 0);
}
