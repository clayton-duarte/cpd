//! `merge-tree --write-tree --name-only`, `merge-base` and the text three-way merge under them, on `GitRoute`
//! through a `FakeRunner` (`FakeRunner::with_git`), and the product's `git/worktree.rs::merge_conflicts_with`
//! against the fake.
//!
//! The expected texts are literals recorded once by hand from real git in a scratch directory. The git at
//! hand was 2.54.0 (Apple Git-157), not 2.43; none of these texts changed between the two as far as I know
//! (unverified on 2.43). What was recorded, and what the tests pin of it:
//!
//! - a clean `merge-tree --write-tree --name-only A B` prints the tree id and exits 0; a conflicted one prints
//!   the tree id, the conflicted paths sorted, a blank line, then the messages in path order (`Auto-merging
//!   <p>` before each path that needed a content merge, `CONFLICT (content): Merge conflict in <p>`,
//!   `CONFLICT (add/add): Merge conflict in <p>`, `CONFLICT (modify/delete): <p> deleted in <x> and modified
//!   in <y>.  Version <y> of <p> left in tree.`), and exits 1. The messages are NOT printed by a clean merge.
//! - the markers are `<<<<<<< <A as given>`, `=======`, `>>>>>>> <B as given>`: the arguments' own text, so a
//!   branch name stays a name and a sha stays the full sha.
//! - two edits one unchanged line apart merge; two edits that touch (lines 2 and 3) conflict. For the touching
//!   ones real git widens the block to both lines; only the conflict is pinned, not the block's text.
//! - `merge-tree` of a name that is no commit (real git: `merge-tree: <x> - not something we can merge`, exit
//!   1 on 2.54, which the product would read as a conflict with no files) is pinned only as "fails, prints no
//!   tree, says so on stderr"; of unrelated histories: `fatal: refusing to merge unrelated histories`, 128.
//! - `merge-base A B` prints the best common ancestor, exit 1 and nothing when there is none, 128 `fatal: Not a
//!   valid object name <x>` for an unknown name.
//!
//! No test runs git (or any process), reads the wall clock or sleeps: every call is a `Command` handed to the
//! `FakeRunner`, which never starts it.

use super::Oid;
use super::scenario::*;
use crate::error::ErrorCode;
use crate::git::merge_conflicts_with;

const MT: [&str; 3] = ["merge-tree", "--write-tree", "--name-only"];

/// A repo whose `base` commit holds `base`, and the branches `ours` and `theirs` over it, merged.
struct Merge {
    w: World,
    repo: super::Repo,
    base: Oid,
    theirs: Oid,
    out: Out,
}

fn merge(base: &[(&str, &str)], ours: &[(&str, &str)], theirs: &[(&str, &str)]) -> Merge {
    let mut w = world();
    let repo = w.repo(base);
    let base_oid = w.rev(repo.dir(), "main");
    let ours_oid = w.snap(&repo, Some(&base_oid), ours, "ours");
    let theirs_oid = w.snap(&repo, Some(&base_oid), theirs, "theirs");
    w.branch(&repo, "ours", &ours_oid);
    w.branch(&repo, "theirs", &theirs_oid);
    let mut m = Merge {
        w,
        repo,
        base: base_oid,
        theirs: theirs_oid,
        out: Out {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
    };
    m.out = m.run("ours", "theirs");
    m
}

impl Merge {
    fn run(&self, a: &str, b: &str) -> Out {
        self.w
            .git(self.repo.dir(), &[MT.as_slice(), &[a, b]].concat())
    }
    /// The first line of stdout: the tree id.
    fn tree(&self) -> String {
        self.out
            .stdout
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    }
    /// The text of `path` in the tree the merge wrote.
    fn file(&self, path: &str) -> Option<String> {
        self.w.file_at(self.repo.dir(), &self.tree(), path)
    }
    /// What follows the tree id: `(conflicted paths, messages)`.
    fn sections(&self) -> (Vec<String>, Vec<String>) {
        let mut lines = self.out.stdout.lines().skip(1);
        let names = lines
            .by_ref()
            .take_while(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        (names, lines.map(str::to_owned).collect())
    }
    fn assert_clean(&self) {
        assert_eq!(
            (self.out.code, self.out.stderr.as_str()),
            (0, ""),
            "stdout: {}",
            self.out.stdout
        );
    }
}

fn is_oid(text: &str) -> bool {
    super::universe::is_oid(text)
}

const EIGHT: &str = "1\n2\n3\n4\n5\n6\n7\n8\n";

// ---- a clean merge ---------------------------------------------------------------------------------

#[test]
fn edits_far_apart_in_one_file_merge_cleanly() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(1, "1X")]))],
        &[("f.txt", &numbered(8, &[(8, "8X")]))],
    );
    m.assert_clean();
    assert_eq!(
        m.file("f.txt").unwrap(),
        numbered(8, &[(1, "1X"), (8, "8X")])
    );
}

#[test]
fn a_clean_merge_prints_the_tree_id_and_nothing_else() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(1, "1X")]))],
        &[("f.txt", &numbered(8, &[(8, "8X")]))],
    );
    assert_eq!(m.out.code, 0);
    assert!(
        is_oid(m.out.stdout.trim_end()),
        "stdout: {:?}",
        m.out.stdout
    );
    assert_eq!(m.out.stdout, format!("{}\n", m.tree()));
    assert_eq!(m.out.stderr, "");
}

#[test]
fn edits_one_unchanged_line_apart_merge_cleanly() {
    let base = numbered(9, &[]);
    let m = merge(
        &[("f.txt", &base)],
        &[("f.txt", &numbered(9, &[(1, "1X")]))],
        &[("f.txt", &numbered(9, &[(3, "3X")]))],
    );
    m.assert_clean();
    assert_eq!(
        m.file("f.txt").unwrap(),
        numbered(9, &[(1, "1X"), (3, "3X")])
    );
}

#[test]
fn an_insertion_by_one_side_and_a_deletion_by_the_other_merge_cleanly() {
    let base = numbered(9, &[]);
    let ours = format!("1\n2\nnew a\nnew b\n{}", "3\n4\n5\n6\n7\n8\n9\n");
    let theirs = "1\n2\n3\n4\n5\n6\n7\n8\n";
    let m = merge(
        &[("f.txt", &base)],
        &[("f.txt", &ours)],
        &[("f.txt", theirs)],
    );
    m.assert_clean();
    assert_eq!(
        m.file("f.txt").unwrap(),
        "1\n2\nnew a\nnew b\n3\n4\n5\n6\n7\n8\n"
    );
}

#[test]
fn a_file_changed_only_by_theirs_takes_theirs() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(4, "4T")]))],
    );
    m.assert_clean();
    assert_eq!(m.file("f.txt").unwrap(), numbered(8, &[(4, "4T")]));
}

#[test]
fn a_file_changed_only_by_ours_keeps_ours() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(4, "4O")]))],
        &[("f.txt", EIGHT)],
    );
    m.assert_clean();
    assert_eq!(m.file("f.txt").unwrap(), numbered(8, &[(4, "4O")]));
}

#[test]
fn edits_in_different_files_merge_cleanly() {
    let m = merge(
        &[("a.txt", "a\n"), ("b.txt", "b\n")],
        &[("a.txt", "a ours\n"), ("b.txt", "b\n")],
        &[("a.txt", "a\n"), ("b.txt", "b theirs\n")],
    );
    m.assert_clean();
    assert_eq!(
        (m.file("a.txt").unwrap(), m.file("b.txt").unwrap()),
        ("a ours\n".to_owned(), "b theirs\n".to_owned())
    );
}

#[test]
fn files_added_by_each_side_under_different_names_are_both_kept() {
    let m = merge(
        &[("keep.txt", "k\n")],
        &[("keep.txt", "k\n"), ("mine.txt", "m\n")],
        &[("keep.txt", "k\n"), ("yours.txt", "y\n")],
    );
    m.assert_clean();
    assert_eq!(
        m.w.paths_at(m.repo.dir(), &m.tree()),
        ["keep.txt", "mine.txt", "yours.txt"]
    );
}

#[test]
fn a_file_deleted_by_one_side_and_untouched_by_the_other_is_deleted() {
    let m = merge(
        &[("gone.txt", "g\n"), ("keep.txt", "k\n")],
        &[("gone.txt", "g\n"), ("keep.txt", "k ours\n")],
        &[("keep.txt", "k\n")],
    );
    m.assert_clean();
    assert_eq!(m.w.paths_at(m.repo.dir(), &m.tree()), ["keep.txt"]);
    assert_eq!(m.file("keep.txt").unwrap(), "k ours\n");
}

#[test]
fn a_file_deleted_on_both_sides_is_deleted_without_a_conflict() {
    let m = merge(
        &[("gone.txt", "g\n"), ("keep.txt", "k\n")],
        &[("keep.txt", "k\n")],
        &[("keep.txt", "k\n")],
    );
    m.assert_clean();
    assert_eq!(m.w.paths_at(m.repo.dir(), &m.tree()), ["keep.txt"]);
}

#[test]
fn the_same_edit_on_both_sides_is_no_conflict() {
    let same = numbered(8, &[(3, "3 both")]);
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &same)],
        &[("f.txt", &same)],
    );
    m.assert_clean();
    assert_eq!(m.file("f.txt").unwrap(), same);
}

#[test]
fn the_same_file_added_with_the_same_text_on_both_sides_is_no_conflict() {
    let m = merge(
        &[("keep.txt", "k\n")],
        &[("keep.txt", "k\n"), ("new.txt", "same\n")],
        &[("keep.txt", "k\n"), ("new.txt", "same\n")],
    );
    m.assert_clean();
    assert_eq!(m.file("new.txt").unwrap(), "same\n");
}

#[test]
fn a_file_without_a_final_newline_keeps_that_after_a_merge() {
    let m = merge(
        &[("f.txt", "e1\ne2\ne3\ne4\ne5")],
        &[("f.txt", "E1\ne2\ne3\ne4\ne5")],
        &[("f.txt", "e1\ne2\ne3\ne4\nE5")],
    );
    m.assert_clean();
    assert_eq!(m.file("f.txt").unwrap(), "E1\ne2\ne3\ne4\nE5");
}

#[test]
fn a_file_in_a_directory_merges_by_its_full_path() {
    let m = merge(
        &[("dir/sub/f.txt", EIGHT)],
        &[("dir/sub/f.txt", &numbered(8, &[(1, "1X")]))],
        &[("dir/sub/f.txt", &numbered(8, &[(8, "8X")]))],
    );
    m.assert_clean();
    assert_eq!(m.w.paths_at(m.repo.dir(), &m.tree()), ["dir/sub/f.txt"]);
    assert_eq!(
        m.file("dir/sub/f.txt").unwrap(),
        numbered(8, &[(1, "1X"), (8, "8X")])
    );
}

#[test]
fn the_tree_of_a_clean_merge_lists_every_path_of_the_result() {
    let m = merge(
        &[("a.txt", "a\n"), ("b.txt", "b\n"), ("c/d.txt", "d\n")],
        &[("a.txt", "a2\n"), ("b.txt", "b\n"), ("c/d.txt", "d\n")],
        &[
            ("a.txt", "a\n"),
            ("b.txt", "b\n"),
            ("c/d.txt", "d\n"),
            ("e.txt", "e\n"),
        ],
    );
    m.assert_clean();
    assert_eq!(
        m.w.paths_at(m.repo.dir(), &m.tree()),
        ["a.txt", "b.txt", "c/d.txt", "e.txt"]
    );
}

#[test]
fn the_same_merge_twice_prints_the_same_tree_id() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(1, "1X")]))],
        &[("f.txt", &numbered(8, &[(8, "8X")]))],
    );
    let again = m.run("ours", "theirs");
    assert_eq!(
        (again.code, again.stdout.as_str()),
        (0, m.out.stdout.as_str())
    );
}

#[test]
fn the_merge_uses_the_merge_base_not_the_root() {
    // base -> x1 (edits line 1) -> ours (edits line 8); theirs is x1 -> theirs (edits line 5).
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let x1 = w.snap(
        &repo,
        Some(&base),
        &[("f.txt", &numbered(8, &[(1, "1X")]))],
        "x1",
    );
    let ours = w.snap(
        &repo,
        Some(&x1),
        &[("f.txt", &numbered(8, &[(1, "1X"), (8, "8O")]))],
        "ours",
    );
    let theirs = w.snap(
        &repo,
        Some(&x1),
        &[("f.txt", &numbered(8, &[(1, "1X"), (5, "5T")]))],
        "theirs",
    );
    w.branch(&repo, "ours", &ours);
    w.branch(&repo, "theirs", &theirs);
    let out = w.git(repo.dir(), &[MT.as_slice(), &["ours", "theirs"]].concat());
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    let tree = out.stdout.trim_end();
    assert_eq!(
        w.file_at(repo.dir(), tree, "f.txt").unwrap(),
        numbered(8, &[(1, "1X"), (5, "5T"), (8, "8O")])
    );
}

#[test]
fn merging_a_commit_the_other_side_already_contains_gives_the_others_tree() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let ahead = w.snap(
        &repo,
        Some(&base),
        &[("f.txt", &numbered(8, &[(2, "2X")]))],
        "ahead",
    );
    w.branch(&repo, "ahead", &ahead);
    let out = w.git(repo.dir(), &[MT.as_slice(), &["ahead", "main"]].concat());
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    assert_eq!(
        out.stdout.trim_end(),
        w.rev(repo.dir(), "ahead^{tree}").as_str()
    );
    let other_way = w.git(repo.dir(), &[MT.as_slice(), &["main", "ahead"]].concat());
    assert_eq!(
        (other_way.code, other_way.stdout.trim_end()),
        (0, w.rev(repo.dir(), "ahead^{tree}").as_str())
    );
}

#[test]
fn merging_a_branch_with_itself_gives_its_tree() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(repo.dir(), &[MT.as_slice(), &["main", "main"]].concat());
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    assert_eq!(
        out.stdout.trim_end(),
        w.rev(repo.dir(), "main^{tree}").as_str()
    );
}

// ---- a conflict ------------------------------------------------------------------------------------

#[test]
fn both_sides_editing_one_line_differently_is_a_conflict_that_exits_1() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(m.out.stderr, "");
    assert_eq!(
        m.out.stdout,
        format!(
            "{}\nf.txt\n\nAuto-merging f.txt\nCONFLICT (content): Merge conflict in f.txt\n",
            m.tree()
        )
    );
    assert!(is_oid(&m.tree()));
}

#[test]
fn the_conflicting_file_holds_the_hunk_between_markers_labelled_with_the_arguments() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    assert_eq!(
        m.file("f.txt").unwrap(),
        "1\n2\n3\n4\n<<<<<<< ours\n5 ours\n=======\n5 theirs\n>>>>>>> theirs\n6\n7\n8\n"
    );
}

#[test]
fn a_sha_given_as_an_argument_labels_the_marker_with_the_full_sha() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    let out = m.run("ours", m.theirs.as_str());
    assert_eq!(out.code, 1);
    let tree = out.stdout.lines().next().unwrap();
    assert_eq!(
        m.w.file_at(m.repo.dir(), tree, "f.txt").unwrap(),
        format!(
            "1\n2\n3\n4\n<<<<<<< ours\n5 ours\n=======\n5 theirs\n>>>>>>> {}\n6\n7\n8\n",
            m.theirs
        )
    );
}

#[test]
fn the_conflict_tree_is_stored_so_its_paths_can_be_listed() {
    let m = merge(
        &[("f.txt", EIGHT), ("ok.txt", "ok\n")],
        &[
            ("f.txt", &numbered(8, &[(5, "5 ours")])),
            ("ok.txt", "ok\n"),
        ],
        &[
            ("f.txt", &numbered(8, &[(5, "5 theirs")])),
            ("ok.txt", "ok\n"),
        ],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(m.w.paths_at(m.repo.dir(), &m.tree()), ["f.txt", "ok.txt"]);
    assert_eq!(m.file("ok.txt").unwrap(), "ok\n");
}

#[test]
fn edits_that_touch_each_other_are_a_conflict() {
    let m = merge(
        &[("f.txt", &numbered(9, &[]))],
        &[("f.txt", &numbered(9, &[(2, "2 ours")]))],
        &[("f.txt", &numbered(9, &[(3, "3 theirs")]))],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(m.sections().0, ["f.txt"]);
    assert!(m.file("f.txt").unwrap().contains("<<<<<<< ours\n"));
}

#[test]
fn two_insertions_at_the_same_place_are_a_conflict() {
    let m = merge(
        &[("f.txt", "x\ny\n")],
        &[("f.txt", "x\nours-ins\ny\n")],
        &[("f.txt", "x\ntheirs-ins\ny\n")],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.file("f.txt").unwrap(),
        "x\n<<<<<<< ours\nours-ins\n=======\ntheirs-ins\n>>>>>>> theirs\ny\n"
    );
}

#[test]
fn two_conflicting_hunks_of_one_file_give_two_marker_blocks() {
    let m = merge(
        &[("f.txt", &numbered(9, &[]))],
        &[("f.txt", &numbered(9, &[(2, "2 ours"), (8, "8 ours")]))],
        &[("f.txt", &numbered(9, &[(2, "2 theirs"), (8, "8 theirs")]))],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.file("f.txt").unwrap(),
        "1\n<<<<<<< ours\n2 ours\n=======\n2 theirs\n>>>>>>> theirs\n3\n4\n5\n6\n7\n<<<<<<< ours\n8 ours\n=======\n8 theirs\n>>>>>>> theirs\n9\n"
    );
    assert_eq!(m.sections().0, ["f.txt"]);
}

#[test]
fn a_conflicting_hunk_and_a_merging_one_in_the_same_file_keep_the_merged_one_clean() {
    let m = merge(
        &[("f.txt", &numbered(9, &[]))],
        &[("f.txt", &numbered(9, &[(1, "1 ours"), (9, "9 ours")]))],
        &[("f.txt", &numbered(9, &[(1, "1 theirs"), (5, "5 theirs")]))],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.file("f.txt").unwrap(),
        "<<<<<<< ours\n1 ours\n=======\n1 theirs\n>>>>>>> theirs\n2\n3\n4\n5 theirs\n6\n7\n8\n9 ours\n"
    );
}

#[test]
fn files_that_merge_beside_a_conflict_get_an_auto_merging_line_and_no_place_in_the_list() {
    let m = merge(
        &[("both.txt", EIGHT), ("c.txt", "c\n")],
        &[
            ("both.txt", &numbered(8, &[(1, "1X")])),
            ("c.txt", "ours\n"),
        ],
        &[
            ("both.txt", &numbered(8, &[(8, "8X")])),
            ("c.txt", "theirs\n"),
        ],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.out.stdout,
        format!(
            "{}\nc.txt\n\nAuto-merging both.txt\nAuto-merging c.txt\nCONFLICT (content): Merge conflict in c.txt\n",
            m.tree()
        )
    );
    assert_eq!(
        m.file("both.txt").unwrap(),
        numbered(8, &[(1, "1X"), (8, "8X")])
    );
}

#[test]
fn a_file_added_on_both_sides_with_different_text_is_an_add_add_conflict() {
    let m = merge(
        &[("keep.txt", "k\n")],
        &[("keep.txt", "k\n"), ("api.txt", "ours api\n")],
        &[("keep.txt", "k\n"), ("api.txt", "theirs api\n")],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.out.stdout,
        format!(
            "{}\napi.txt\n\nAuto-merging api.txt\nCONFLICT (add/add): Merge conflict in api.txt\n",
            m.tree()
        )
    );
    assert_eq!(
        m.file("api.txt").unwrap(),
        "<<<<<<< ours\nours api\n=======\ntheirs api\n>>>>>>> theirs\n"
    );
}

#[test]
fn a_file_modified_by_ours_and_deleted_by_theirs_is_a_modify_delete_conflict_that_keeps_ours() {
    let m = merge(
        &[("md.txt", "m1\nm2\n"), ("keep.txt", "k\n")],
        &[("md.txt", "m1\nM2 ours\n"), ("keep.txt", "k\n")],
        &[("keep.txt", "k\n")],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.out.stdout,
        format!(
            "{}\nmd.txt\n\nCONFLICT (modify/delete): md.txt deleted in theirs and modified in ours.  Version ours of md.txt left in tree.\n",
            m.tree()
        )
    );
    assert_eq!(m.file("md.txt").unwrap(), "m1\nM2 ours\n");
}

#[test]
fn a_file_deleted_by_ours_and_modified_by_theirs_is_a_modify_delete_conflict_that_keeps_theirs() {
    let m = merge(
        &[("md.txt", "m1\nm2\n"), ("keep.txt", "k\n")],
        &[("keep.txt", "k\n")],
        &[("md.txt", "m1\nM2 theirs\n"), ("keep.txt", "k\n")],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.out.stdout,
        format!(
            "{}\nmd.txt\n\nCONFLICT (modify/delete): md.txt deleted in ours and modified in theirs.  Version theirs of md.txt left in tree.\n",
            m.tree()
        )
    );
    assert_eq!(m.file("md.txt").unwrap(), "m1\nM2 theirs\n");
}

#[test]
fn the_conflicted_paths_and_their_messages_come_sorted_by_path() {
    let base = [
        ("z.txt", "a\n"),
        ("a.txt", "a\n"),
        ("d/m.txt", "a\n"),
        ("md.txt", "m1\nm2\n"),
    ];
    let m = merge(
        &base,
        &[
            ("z.txt", "ours\n"),
            ("a.txt", "ours\n"),
            ("d/m.txt", "ours\n"),
        ],
        &[
            ("z.txt", "theirs\n"),
            ("a.txt", "theirs\n"),
            ("d/m.txt", "theirs\n"),
            ("md.txt", "m1\nM2\n"),
        ],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(
        m.out.stdout,
        format!(
            "{}\na.txt\nd/m.txt\nmd.txt\nz.txt\n\n\
             Auto-merging a.txt\nCONFLICT (content): Merge conflict in a.txt\n\
             Auto-merging d/m.txt\nCONFLICT (content): Merge conflict in d/m.txt\n\
             CONFLICT (modify/delete): md.txt deleted in ours and modified in theirs.  Version theirs of md.txt left in tree.\n\
             Auto-merging z.txt\nCONFLICT (content): Merge conflict in z.txt\n",
            m.tree()
        )
    );
}

#[test]
fn a_conflict_in_a_directory_lists_the_full_path() {
    let m = merge(
        &[("d/e/f.txt", EIGHT)],
        &[("d/e/f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("d/e/f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    assert_eq!(m.out.code, 1);
    assert_eq!(m.sections().0, ["d/e/f.txt"]);
    assert!(m.file("d/e/f.txt").unwrap().contains("<<<<<<< ours\n"));
}

#[test]
fn the_names_section_ends_at_a_blank_line_before_the_messages() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    let (names, messages) = m.sections();
    assert_eq!(names, ["f.txt"]);
    assert_eq!(
        messages,
        [
            "Auto-merging f.txt",
            "CONFLICT (content): Merge conflict in f.txt"
        ]
    );
}

#[test]
fn a_conflict_tree_differs_from_the_tree_of_either_side() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    let tree = m.tree();
    assert_ne!(tree, m.w.rev(m.repo.dir(), "ours^{tree}").to_string());
    assert_ne!(tree, m.w.rev(m.repo.dir(), "theirs^{tree}").to_string());
    assert_eq!(m.base, m.w.rev(m.repo.dir(), "main"));
}

#[test]
fn the_same_conflict_twice_prints_the_same_text() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    let again = m.run("ours", "theirs");
    assert_eq!(
        (again.code, again.stdout.as_str()),
        (1, m.out.stdout.as_str())
    );
}

#[test]
fn the_remote_tracking_names_of_the_product_resolve_and_label_the_markers() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let main = w.snap(
        &repo,
        Some(&base),
        &[("f.txt", &numbered(8, &[(5, "5 main")]))],
        "main2",
    );
    let feature = w.snap(
        &repo,
        Some(&base),
        &[("f.txt", &numbered(8, &[(5, "5 feature")]))],
        "feature",
    );
    w.tracking(&repo, "main", &main);
    w.tracking(&repo, "feature", &feature);
    let out = w.git(
        repo.dir(),
        &[MT.as_slice(), &["origin/main", "origin/feature"]].concat(),
    );
    assert_eq!(out.code, 1);
    let tree = out.stdout.lines().next().unwrap();
    assert_eq!(
        w.file_at(repo.dir(), tree, "f.txt").unwrap(),
        "1\n2\n3\n4\n<<<<<<< origin/main\n5 main\n=======\n5 feature\n>>>>>>> origin/feature\n6\n7\n8\n"
    );
}

// ---- what is not a merge -----------------------------------------------------------------------------

#[test]
fn unrelated_histories_are_refused_with_exit_128() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let orphan = w.snap(&repo, None, &[("o.txt", "o\n")], "orphan");
    w.branch(&repo, "orphan", &orphan);
    let out = w.git(repo.dir(), &[MT.as_slice(), &["main", "orphan"]].concat());
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", "fatal: refusing to merge unrelated histories\n")
    );
}

#[test]
fn a_name_that_is_no_commit_fails_without_a_tree_and_says_so() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(repo.dir(), &[MT.as_slice(), &["main", "nonexist"]].concat());
    assert_ne!(out.code, 0);
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr.contains("nonexist - not something we can merge"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn merge_tree_outside_a_repo_is_not_a_git_repository() {
    let w = world();
    let out = w.git(&w.root(), &[MT.as_slice(), &["main", "main"]].concat());
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (
            128,
            "fatal: not a git repository (or any of the parent directories): .git\n"
        )
    );
}

#[test]
fn a_flag_merge_tree_does_not_model_is_refused_and_recorded() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(
        repo.dir(),
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--bogus",
            "main",
            "main",
        ],
    );
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "fake-git: unsupported: merge-tree --bogus\n")
    );
    assert_eq!(w.route.unsupported(), ["merge-tree --bogus"]);
}

#[test]
fn merge_tree_without_write_tree_is_refused_and_recorded() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(repo.dir(), &["merge-tree", "main", "main"]);
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.starts_with("fake-git: unsupported: merge-tree"),
        "stderr: {}",
        out.stderr
    );
    assert_eq!(w.route.unsupported().len(), 1);
}

#[test]
fn merge_tree_without_name_only_is_refused_and_recorded() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(repo.dir(), &["merge-tree", "--write-tree", "main", "main"]);
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.starts_with("fake-git: unsupported: merge-tree"),
        "stderr: {}",
        out.stderr
    );
    assert_eq!(w.route.unsupported().len(), 1);
}

// ---- merge-base ----------------------------------------------------------------------------------------

#[test]
fn merge_base_prints_the_commit_two_branches_forked_from() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let a = w.snap(&repo, Some(&base), &[("f.txt", "a\n")], "a");
    let b = w.snap(&repo, Some(&a), &[("f.txt", "b\n")], "b");
    let c = w.snap(&repo, Some(&a), &[("f.txt", "c\n")], "c");
    w.branch(&repo, "left", &b);
    w.branch(&repo, "right", &c);
    let out = w.git(repo.dir(), &["merge-base", "left", "right"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, format!("{a}\n"), String::new())
    );
}

#[test]
fn merge_base_of_a_commit_and_its_descendant_is_the_ancestor() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let next = w.snap(&repo, Some(&base), &[("f.txt", "n\n")], "next");
    w.branch(&repo, "next", &next);
    for args in [["main", "next"], ["next", "main"]] {
        let out = w.git(repo.dir(), &["merge-base", args[0], args[1]]);
        assert_eq!((out.code, out.stdout), (0, format!("{base}\n")));
    }
}

#[test]
fn merge_base_of_a_commit_with_itself_is_that_commit() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(repo.dir(), &["merge-base", "main", "main"]);
    assert_eq!(
        (out.code, out.stdout),
        (0, format!("{}\n", w.rev(repo.dir(), "main")))
    );
}

#[test]
fn merge_base_takes_a_full_sha_and_a_remote_tracking_name() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let next = w.snap(&repo, Some(&base), &[("f.txt", "n\n")], "next");
    w.tracking(&repo, "main", &next);
    let out = w.git(repo.dir(), &["merge-base", base.as_str(), "origin/main"]);
    assert_eq!((out.code, out.stdout), (0, format!("{base}\n")));
}

#[test]
fn merge_base_of_unrelated_histories_exits_1_and_prints_nothing() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let orphan = w.snap(&repo, None, &[("o.txt", "o\n")], "orphan");
    w.branch(&repo, "orphan", &orphan);
    let out = w.git(repo.dir(), &["merge-base", "main", "orphan"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "")
    );
}

#[test]
fn merge_base_of_an_unknown_name_is_128() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let out = w.git(repo.dir(), &["merge-base", "main", "nonexist"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", "fatal: Not a valid object name nonexist\n")
    );
}

// ---- the product: git/worktree.rs::merge_conflicts_with ------------------------------------------------

#[test]
fn merge_conflicts_is_none_when_the_two_merge_cleanly() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(1, "1X")]))],
        &[("f.txt", &numbered(8, &[(8, "8X")]))],
    );
    assert_eq!(
        merge_conflicts_with(&m.w.runner, m.repo.dir(), "ours", "theirs").unwrap(),
        None
    );
}

#[test]
fn merge_conflicts_is_none_when_the_sha_already_contains_onto() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let next = w.snap(&repo, Some(&base), &[("f.txt", "n\n")], "next");
    assert_eq!(
        merge_conflicts_with(&w.runner, repo.dir(), "main", next.as_str()).unwrap(),
        None
    );
}

#[test]
fn merge_conflicts_is_none_when_onto_already_contains_the_sha() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let next = w.snap(&repo, Some(&base), &[("f.txt", "n\n")], "next");
    w.branch(&repo, "main", &next);
    assert_eq!(
        merge_conflicts_with(&w.runner, repo.dir(), "main", base.as_str()).unwrap(),
        None
    );
}

#[test]
fn merge_conflicts_lists_the_conflicted_files_sorted() {
    let base = [
        ("z.txt", "a\n"),
        ("a.txt", "a\n"),
        ("d/m.txt", "a\n"),
        ("ok.txt", "ok\n"),
    ];
    let m = merge(
        &base,
        &[
            ("z.txt", "ours\n"),
            ("a.txt", "ours\n"),
            ("d/m.txt", "ours\n"),
            ("ok.txt", "ok ours\n"),
        ],
        &[
            ("z.txt", "theirs\n"),
            ("a.txt", "theirs\n"),
            ("d/m.txt", "theirs\n"),
            ("ok.txt", "ok\n"),
        ],
    );
    assert_eq!(
        merge_conflicts_with(&m.w.runner, m.repo.dir(), "ours", "theirs").unwrap(),
        Some(vec![
            "a.txt".to_owned(),
            "d/m.txt".to_owned(),
            "z.txt".to_owned()
        ])
    );
}

#[test]
fn merge_conflicts_lists_an_add_add_and_a_modify_delete_path() {
    let m = merge(
        &[("md.txt", "m1\nm2\n")],
        &[("md.txt", "m1\nM2\n"), ("api.txt", "ours api\n")],
        &[("api.txt", "theirs api\n")],
    );
    assert_eq!(
        merge_conflicts_with(&m.w.runner, m.repo.dir(), "ours", "theirs").unwrap(),
        Some(vec!["api.txt".to_owned(), "md.txt".to_owned()])
    );
}

#[test]
fn merge_conflicts_reads_remote_tracking_names_in_the_repo_it_is_given() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let base = w.rev(repo.dir(), "main");
    let main = w.snap(
        &repo,
        Some(&base),
        &[("f.txt", &numbered(8, &[(5, "5 main")]))],
        "main2",
    );
    let feature = w.snap(
        &repo,
        Some(&base),
        &[("f.txt", &numbered(8, &[(5, "5 feature")]))],
        "feature",
    );
    w.tracking(&repo, "main", &main);
    w.tracking(&repo, "feature", &feature);
    assert_eq!(
        merge_conflicts_with(&w.runner, repo.dir(), "origin/main", "origin/feature").unwrap(),
        Some(vec!["f.txt".to_owned()])
    );
}

#[test]
fn merge_conflicts_of_unrelated_histories_is_git_failed_with_the_exit_in_the_details() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", EIGHT)]);
    let orphan = w.snap(&repo, None, &[("o.txt", "o\n")], "orphan");
    let error = merge_conflicts_with(&w.runner, repo.dir(), "main", orphan.as_str()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::GitFailed);
    assert_eq!(
        error.details().get("exit").and_then(|v| v.as_i64()),
        Some(128)
    );
}

#[test]
fn merge_conflicts_runs_one_merge_tree_call_and_no_other() {
    let m = merge(
        &[("f.txt", EIGHT)],
        &[("f.txt", &numbered(8, &[(5, "5 ours")]))],
        &[("f.txt", &numbered(8, &[(5, "5 theirs")]))],
    );
    let before = m.w.runner.calls().len();
    merge_conflicts_with(&m.w.runner, m.repo.dir(), "ours", "theirs").unwrap();
    let calls = m.w.runner.calls();
    assert_eq!(calls.len(), before + 1);
    let args = &calls.last().unwrap().args;
    assert!(args.iter().any(|a| a == "merge-tree"), "args: {args:?}");
}
