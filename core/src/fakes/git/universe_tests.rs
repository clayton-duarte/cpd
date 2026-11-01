//! The Universe through its API only (no argv): objects, repos, refs, revisions, ancestry.
//!
//! Literal ids are git's own: recorded once by hand from real git in a scratch directory (git 2.54.0
//! Apple Git-157 was the one at hand; object ids do not change between git versions, so 2.43 gives them
//! too: unverified on 2.43) with the author and committer `cpd test <test@cpd.invalid>` at
//! `1700000000 +0000`. No test runs git, reads the wall clock or sleeps.

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use super::{Head, Oid, RepoId, RevError, Selection, Universe, UnknownObject, parse, range};
use crate::clock::ManualClock;

const HELLO_BLOB: &str = "ce013625030ba8dba906f756967f9e9ca394464a"; // `printf 'hello\n' | git hash-object --stdin`
const EMPTY_BLOB: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"; // `git hash-object /dev/null`
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"; // `git hash-object -t tree /dev/null`
const A_TXT_TREE: &str = "2e81171448eb9f2ee3821e3d447aa6b2fe3ddba1"; // a tree of `a.txt` = "hello\n"
const INIT_ON_EMPTY_TREE: &str = "dc4d4367eee9adc86209a21de11cb2196514f34a"; // `git commit-tree <empty tree> -m init`
const INIT_ON_A_TXT_TREE: &str = "4ee0c5f0266122bbbec36d02376bcb7c53af0a6b"; // `git commit-tree <A_TXT_TREE> -m init`

struct Fx {
    tmp: TempDir,
    clock: ManualClock,
    u: Universe,
    repos: usize,
}

fn fx() -> Fx {
    let clock = ManualClock::new();
    let mut u = Universe::new(Arc::new(clock.clone()));
    u.set_global("user.name", "cpd test");
    u.set_global("user.email", "test@cpd.invalid");
    Fx {
        tmp: tempfile::tempdir().unwrap(),
        clock,
        u,
        repos: 0,
    }
}

impl Fx {
    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    /// A new non-bare repo in its own directory under the temp root.
    fn repo(&mut self) -> (RepoId, PathBuf) {
        self.repos += 1;
        let dir = self.root().join(format!("repo{}", self.repos));
        (self.u.init(&dir, false), dir)
    }
    /// A commit of `files` on `parents` one virtual second after the last one.
    fn commit(
        &mut self,
        repo: RepoId,
        parents: &[&Oid],
        files: &[(&str, &str)],
        message: &str,
    ) -> Oid {
        self.clock.advance(Duration::from_secs(1));
        let blobs: Vec<(&str, Oid)> = files
            .iter()
            .map(|(p, t)| (*p, self.u.blob(repo, t.as_bytes())))
            .collect();
        let entries: Vec<(&str, &Oid)> = blobs.iter().map(|(p, b)| (*p, b)).collect();
        let tree = self.u.tree(repo, &entries);
        let parents: Vec<Oid> = parents.iter().map(|p| (*p).clone()).collect();
        self.u.commit(repo, &tree, &parents, message)
    }
    fn branch(&mut self, repo: RepoId, name: &str, at: &Oid) {
        self.u
            .set_ref(repo, &format!("refs/heads/{name}"), at, "branch");
    }
    /// A repo on `main` with one commit, and its main worktree.
    fn on_main(&mut self) -> (RepoId, PathBuf, Oid) {
        let (repo, dir) = self.repo();
        let c = self.commit(repo, &[], &[("README.md", "one\n")], "one");
        self.branch(repo, "main", &c);
        (repo, dir, c)
    }
}

fn oid(hex: &str) -> Oid {
    Oid::new(hex)
}

// ---- objects ----------------------------------------------------------------

#[test]
fn a_blob_has_the_id_git_gives_the_same_bytes() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    assert_eq!(fx.u.blob(repo, b"hello\n"), oid(HELLO_BLOB));
}

#[test]
fn an_empty_blob_has_the_id_git_gives_it() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    assert_eq!(fx.u.blob(repo, b""), oid(EMPTY_BLOB));
}

#[test]
fn an_id_is_forty_lowercase_hex_characters() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let id = fx.u.blob(repo, b"x").to_string();
    assert_eq!(id.len(), 40);
    assert!(
        id.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{id}"
    );
}

#[test]
fn the_same_bytes_have_one_id_in_every_repo() {
    let mut fx = fx();
    let (a, _) = fx.repo();
    let (b, _) = fx.repo();
    assert_eq!(fx.u.blob(a, b"same"), fx.u.blob(b, b"same"));
}

#[test]
fn different_bytes_have_different_ids() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    assert_ne!(fx.u.blob(repo, b"one"), fx.u.blob(repo, b"two"));
}

#[test]
fn a_blob_reads_back_its_bytes() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let id = fx.u.blob(repo, b"hello\n");
    assert_eq!(fx.u.blob_bytes(&id), Some(b"hello\n".to_vec()));
}

#[test]
fn a_tree_without_entries_is_gits_empty_tree() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    assert_eq!(fx.u.tree(repo, &[]), oid(EMPTY_TREE));
}

#[test]
fn a_tree_of_one_blob_has_the_id_git_gives_it() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let blob = fx.u.blob(repo, b"hello\n");
    assert_eq!(fx.u.tree(repo, &[("a.txt", &blob)]), oid(A_TXT_TREE));
}

#[test]
fn a_tree_id_does_not_depend_on_the_order_of_its_entries() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let a = fx.u.blob(repo, b"a");
    let b = fx.u.blob(repo, b"b");
    assert_eq!(
        fx.u.tree(repo, &[("a.txt", &a), ("b.txt", &b)]),
        fx.u.tree(repo, &[("b.txt", &b), ("a.txt", &a)])
    );
}

#[test]
fn a_tree_is_flat_a_path_with_a_slash_is_one_entry_and_entries_read_back_sorted() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let a = fx.u.blob(repo, b"a");
    let b = fx.u.blob(repo, b"b");
    let tree = fx.u.tree(repo, &[("z.txt", &b), ("dir/sub/a.txt", &a)]);
    assert_eq!(
        fx.u.tree_entries(&tree),
        Some(vec![
            ("dir/sub/a.txt".to_owned(), a),
            ("z.txt".to_owned(), b)
        ])
    );
}

#[test]
fn a_commit_on_the_empty_tree_has_the_id_git_gives_it() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let tree = fx.u.tree(repo, &[]);
    assert_eq!(
        fx.u.commit(repo, &tree, &[], "init"),
        oid(INIT_ON_EMPTY_TREE)
    );
}

#[test]
fn a_commit_of_one_file_has_the_id_git_gives_it() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let blob = fx.u.blob(repo, b"hello\n");
    let tree = fx.u.tree(repo, &[("a.txt", &blob)]);
    assert_eq!(
        fx.u.commit(repo, &tree, &[], "init"),
        oid(INIT_ON_A_TXT_TREE)
    );
}

#[test]
fn a_commit_reads_back_its_tree_parents_author_message_and_time() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let first = fx.commit(repo, &[], &[("a.txt", "a")], "first");
    let tree = fx.u.tree(repo, &[]);
    let second =
        fx.u.commit(repo, &tree, std::slice::from_ref(&first), "second");
    let data = fx.u.commit_data(&second).unwrap();
    assert_eq!(data.tree, tree);
    assert_eq!(data.parents, vec![first]);
    assert_eq!(data.author, "cpd test <test@cpd.invalid>");
    assert_eq!(data.message, "second");
}

#[test]
fn a_commit_id_changes_with_its_message_its_tree_and_its_parents() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let tree = fx.u.tree(repo, &[]);
    let base = fx.u.commit(repo, &tree, &[], "m");
    let other_message = fx.u.commit(repo, &tree, &[], "n");
    let blob = fx.u.blob(repo, b"x");
    let other_tree = fx.u.tree(repo, &[("x", &blob)]);
    let with_other_tree = fx.u.commit(repo, &other_tree, &[], "m");
    let with_parent = fx.u.commit(repo, &tree, std::slice::from_ref(&base), "m");
    let ids = [&base, &other_message, &with_other_tree, &with_parent];
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn a_commit_is_timed_by_the_injected_clock_from_the_epoch() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let tree = fx.u.tree(repo, &[]);
    let at_start = fx.u.commit(repo, &tree, &[], "start");
    fx.clock.advance(Duration::from_secs(90));
    let later = fx.u.commit(repo, &tree, &[], "later");
    assert_eq!(fx.u.commit_data(&at_start).unwrap().time, Universe::EPOCH);
    assert_eq!(fx.u.commit_data(&later).unwrap().time, Universe::EPOCH + 90);
}

#[test]
fn a_commit_time_counts_the_naps_of_the_clock_too() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let tree = fx.u.tree(repo, &[]);
    crate::clock::Clock::sleep(&fx.clock, Duration::from_secs(5));
    let c = fx.u.commit(repo, &tree, &[], "napped");
    assert_eq!(fx.u.commit_data(&c).unwrap().time, Universe::EPOCH + 5);
}

#[test]
fn a_commit_takes_its_author_from_the_global_user_config() {
    let mut fx = fx();
    fx.u.set_global("user.name", "Ada");
    fx.u.set_global("user.email", "ada@example.test");
    let (repo, _) = fx.repo();
    let tree = fx.u.tree(repo, &[]);
    let c = fx.u.commit(repo, &tree, &[], "m");
    assert_eq!(
        fx.u.commit_data(&c).unwrap().author,
        "Ada <ada@example.test>"
    );
}

#[test]
fn a_repos_own_user_config_overrides_the_global_one() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_set(repo, "user.name", "Local");
    let tree = fx.u.tree(repo, &[]);
    let c = fx.u.commit(repo, &tree, &[], "m");
    assert_eq!(
        fx.u.commit_data(&c).unwrap().author,
        "Local <test@cpd.invalid>"
    );
}

#[test]
fn an_unknown_object_reads_back_as_none() {
    let fx = fx();
    let nothing = oid("0123456789012345678901234567890123456789");
    assert!(fx.u.commit_data(&nothing).is_none());
    assert!(fx.u.tree_entries(&nothing).is_none());
    assert!(fx.u.blob_bytes(&nothing).is_none());
}

// ---- which repo holds which object --------------------------------------------

#[test]
fn a_repo_holds_what_was_made_in_it_and_not_what_was_made_in_another() {
    let mut fx = fx();
    let (a, _) = fx.repo();
    let (b, _) = fx.repo();
    let c = fx.commit(a, &[], &[("a.txt", "a")], "in a");
    assert!(fx.u.has(a, &c));
    assert!(!fx.u.has(b, &c));
}

#[test]
fn a_repo_holds_no_object_nobody_made() {
    let mut fx = fx();
    let (a, _) = fx.repo();
    assert!(
        !fx.u
            .has(a, &oid("0123456789012345678901234567890123456789"))
    );
}

#[test]
fn copying_a_commit_gives_the_other_repo_its_tree_blobs_and_ancestors() {
    let mut fx = fx();
    let (a, _) = fx.repo();
    let (b, _) = fx.repo();
    let first = fx.commit(a, &[], &[("a.txt", "a")], "first");
    let second = fx.commit(a, &[&first], &[("b.txt", "b")], "second");
    let tree = fx.u.commit_data(&second).unwrap().tree;
    let blob = fx.u.tree_entries(&tree).unwrap()[0].1.clone();
    fx.u.copy(a, b, &second);
    for held in [&second, &first, &tree, &blob] {
        assert!(fx.u.has(b, held), "{held}");
    }
}

#[test]
fn copying_a_commit_does_not_copy_its_descendants() {
    let mut fx = fx();
    let (a, _) = fx.repo();
    let (b, _) = fx.repo();
    let first = fx.commit(a, &[], &[("a.txt", "a")], "first");
    let second = fx.commit(a, &[&first], &[("b.txt", "b")], "second");
    fx.u.copy(a, b, &first);
    assert!(!fx.u.has(b, &second));
}

// ---- repos and worktrees ---------------------------------------------------------

#[test]
fn a_repo_is_a_real_dot_git_directory() {
    let mut fx = fx();
    let (repo, dir) = fx.repo();
    assert!(dir.join(".git").is_dir());
    assert_eq!(fx.u.git_dir(repo), dir.join(".git"));
    assert!(!fx.u.is_bare(repo));
}

#[test]
fn a_bare_repo_is_its_own_git_dir() {
    let mut fx = fx();
    let dir = fx.root().join("origin.git");
    let repo = fx.u.init(&dir, true);
    assert!(fx.u.is_bare(repo));
    assert_eq!(fx.u.git_dir(repo), dir);
    assert!(dir.is_dir());
    assert!(fx.u.worktrees(repo).is_empty());
}

#[test]
fn nothing_but_an_empty_git_dir_exists_on_disk_for_objects_refs_and_config() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    fx.u.config_add(repo, "remote.origin.url", "https://github.com/acme/x.git");
    let names: Vec<String> = std::fs::read_dir(dir.join(".git"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    for absent in ["objects", "refs", "config", "packed-refs"] {
        assert!(!names.iter().any(|n| n == absent), "{absent} in {names:?}");
    }
}

#[test]
fn a_new_repo_is_on_the_unborn_branch_main() {
    let mut fx = fx();
    let (repo, dir) = fx.repo();
    assert_eq!(fx.u.head(&dir), Head::Branch("main".to_owned()));
    assert_eq!(fx.u.ref_oid(repo, "refs/heads/main"), None);
}

#[test]
fn a_path_inside_a_worktree_belongs_to_its_repo() {
    let mut fx = fx();
    let (repo, dir) = fx.repo();
    std::fs::create_dir_all(dir.join("src/deep")).unwrap();
    assert_eq!(fx.u.repo_of(&dir), Some(repo));
    assert_eq!(fx.u.repo_of(&dir.join("src/deep")), Some(repo));
}

#[test]
fn a_path_inside_a_bare_repo_belongs_to_it() {
    let mut fx = fx();
    let dir = fx.root().join("origin.git");
    let repo = fx.u.init(&dir, true);
    assert_eq!(fx.u.repo_of(&dir.join("refs")), Some(repo));
}

#[test]
fn a_path_in_no_repo_belongs_to_none() {
    let mut fx = fx();
    let _ = fx.repo();
    assert_eq!(fx.u.repo_of(&fx.root()), None);
    assert_eq!(fx.u.repo_of(&fx.root().join("elsewhere")), None);
}

#[test]
fn a_repo_nested_in_another_repos_worktree_is_its_own() {
    let mut fx = fx();
    let (outer, outer_dir) = fx.repo();
    let inner_dir = outer_dir.join("vendor/inner");
    let inner = fx.u.init(&inner_dir, false);
    assert_ne!(outer, inner);
    assert_eq!(fx.u.repo_of(&inner_dir), Some(inner));
    assert_eq!(fx.u.repo_of(&outer_dir.join("vendor")), Some(outer));
}

#[test]
fn a_linked_worktree_is_a_directory_with_a_dot_git_file_naming_its_admin_dir() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let wt_path = fx.root().join("wt");
    let wt =
        fx.u.add_worktree(repo, &wt_path, Head::Branch("feat".to_owned()));
    assert_eq!(
        wt,
        super::Worktree {
            path: wt_path.clone(),
            id: Some("wt".to_owned())
        }
    );
    let admin = dir.join(".git/worktrees/wt");
    assert!(admin.is_dir());
    assert_eq!(
        std::fs::read_to_string(wt_path.join(".git")).unwrap(),
        format!("gitdir: {}\n", admin.display())
    );
    assert_eq!(fx.u.admin_dir(&wt_path), admin);
}

#[test]
fn a_taken_worktree_id_gets_the_first_free_number() {
    let mut fx = fx();
    let (repo, _, _) = fx.on_main();
    let first = fx.u.add_worktree(
        repo,
        &fx.root().join("a/wt"),
        Head::Branch("one".to_owned()),
    );
    let second = fx.u.add_worktree(
        repo,
        &fx.root().join("b/wt"),
        Head::Branch("two".to_owned()),
    );
    assert_eq!(first.id.as_deref(), Some("wt"));
    assert_eq!(second.id.as_deref(), Some("wt1"));
}

#[test]
fn a_worktree_is_locked_with_a_reason_and_unlocked() {
    let mut fx = fx();
    let (repo, _, _) = fx.on_main();
    let wt_path = fx.root().join("wt");
    fx.u.add_worktree(repo, &wt_path, Head::Branch("feat".to_owned()));
    assert_eq!(fx.u.lock_reason(&wt_path), None);
    fx.u.lock_worktree(&wt_path, "why");
    assert_eq!(fx.u.lock_reason(&wt_path).as_deref(), Some("why"));
    fx.u.unlock_worktree(&wt_path);
    assert_eq!(fx.u.lock_reason(&wt_path), None);
}

#[test]
fn the_main_worktree_comes_first_and_the_main_admin_dir_is_the_git_dir() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let wt_path = fx.root().join("wt");
    fx.u.add_worktree(repo, &wt_path, Head::Branch("feat".to_owned()));
    let listed = fx.u.worktrees(repo);
    assert_eq!(
        listed[0],
        super::Worktree {
            path: dir.clone(),
            id: None
        }
    );
    assert_eq!(listed[1].path, wt_path);
    assert_eq!(fx.u.admin_dir(&dir), dir.join(".git"));
}

#[test]
fn a_linked_worktree_path_resolves_to_the_repo_it_was_added_to() {
    let mut fx = fx();
    let (repo, _, _) = fx.on_main();
    let wt_path = fx.root().join("wt");
    fx.u.add_worktree(repo, &wt_path, Head::Branch("feat".to_owned()));
    assert_eq!(fx.u.repo_of(&wt_path), Some(repo));
}

// ---- refs, HEAD, reflogs ----------------------------------------------------------

#[test]
fn a_ref_reads_back_what_it_was_set_to_and_none_when_unset() {
    let mut fx = fx();
    let (repo, _, c) = fx.on_main();
    assert_eq!(fx.u.ref_oid(repo, "refs/heads/main"), Some(c));
    assert_eq!(fx.u.ref_oid(repo, "refs/heads/other"), None);
}

#[test]
fn a_deleted_ref_is_gone() {
    let mut fx = fx();
    let (repo, _, _) = fx.on_main();
    fx.u.delete_ref(repo, "refs/heads/main");
    assert_eq!(fx.u.ref_oid(repo, "refs/heads/main"), None);
}

#[test]
fn refs_under_a_prefix_are_sorted_by_name() {
    let mut fx = fx();
    let (repo, _, c) = fx.on_main();
    fx.u.set_ref(repo, "refs/heads/b", &c, "x");
    fx.u.set_ref(repo, "refs/heads/a", &c, "x");
    fx.u.set_ref(repo, "refs/tags/v1", &c, "x");
    let names: Vec<String> =
        fx.u.refs(repo, "refs/heads/")
            .into_iter()
            .map(|(n, _)| n)
            .collect();
    assert_eq!(names, ["refs/heads/a", "refs/heads/b", "refs/heads/main"]);
}

#[test]
fn two_repos_keep_their_own_refs() {
    let mut fx = fx();
    let (a, _, c) = fx.on_main();
    let (b, _) = fx.repo();
    assert_eq!(fx.u.ref_oid(a, "refs/heads/main"), Some(c));
    assert_eq!(fx.u.ref_oid(b, "refs/heads/main"), None);
}

#[test]
fn a_symbolic_ref_reads_back_its_target() {
    let mut fx = fx();
    let (repo, _, _) = fx.on_main();
    assert_eq!(fx.u.symbolic_ref(repo, "refs/remotes/origin/HEAD"), None);
    fx.u.set_symbolic_ref(repo, "refs/remotes/origin/HEAD", "refs/remotes/origin/main");
    assert_eq!(
        fx.u.symbolic_ref(repo, "refs/remotes/origin/HEAD")
            .as_deref(),
        Some("refs/remotes/origin/main")
    );
}

#[test]
fn each_worktree_has_its_own_head_and_the_refs_are_shared() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let wt_path = fx.root().join("wt");
    fx.u.add_worktree(repo, &wt_path, Head::Branch("feat".to_owned()));
    assert_eq!(fx.u.head(&dir), Head::Branch("main".to_owned()));
    assert_eq!(fx.u.head(&wt_path), Head::Branch("feat".to_owned()));
    fx.u.set_head(&wt_path, Head::Detached(c.clone()));
    assert_eq!(fx.u.head(&wt_path), Head::Detached(c.clone()));
    assert_eq!(fx.u.head(&dir), Head::Branch("main".to_owned()));
    fx.u.set_ref(repo, "refs/heads/shared", &c, "x");
    assert_eq!(fx.u.ref_oid(repo, "refs/heads/shared"), Some(c));
}

#[test]
fn setting_a_ref_appends_a_reflog_entry_with_the_old_value_the_tree_and_the_message() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    let first = fx.commit(repo, &[], &[("a.txt", "a")], "first");
    let second = fx.commit(repo, &[&first], &[("a.txt", "b")], "second");
    fx.u.set_ref(repo, "refs/heads/main", &first, "commit (initial): first");
    fx.u.set_ref(repo, "refs/heads/main", &second, "commit: second");
    let log = fx.u.reflog(repo, "refs/heads/main");
    let tree_of = |c: &Oid| fx.u.commit_data(c).unwrap().tree;
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].old, Some(first.clone()));
    assert_eq!(log[0].new, second.clone());
    assert_eq!(log[0].tree, tree_of(&second));
    assert_eq!(log[0].message, "commit: second");
    assert_eq!(log[1].old, None);
    assert_eq!(log[1].new, first.clone());
    assert_eq!(log[1].tree, tree_of(&first));
}

#[test]
fn a_reflog_is_per_ref_and_empty_for_a_ref_that_never_moved() {
    let mut fx = fx();
    let (repo, _, _) = fx.on_main();
    assert_eq!(fx.u.reflog(repo, "refs/heads/main").len(), 1);
    assert!(fx.u.reflog(repo, "refs/heads/other").is_empty());
}

#[test]
fn the_remote_tips_are_the_refs_under_that_remote_only() {
    let mut fx = fx();
    let (repo, _, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.u.set_ref(repo, "refs/remotes/origin/main", &c, "fetch");
    fx.u.set_ref(repo, "refs/remotes/origin/feat", &d, "fetch");
    fx.u.set_ref(repo, "refs/remotes/upstream/main", &d, "fetch");
    let mut tips = fx.u.remote_tips(repo, "origin");
    tips.sort();
    let mut want = vec![c, d];
    want.sort();
    assert_eq!(tips, want);
}

// ---- config -------------------------------------------------------------------------

#[test]
fn config_is_multi_valued_in_the_order_added() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(
        repo,
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    );
    fx.u.config_add(
        repo,
        "remote.origin.fetch",
        "+refs/heads/x:refs/remotes/origin/x",
    );
    assert_eq!(
        fx.u.config_values(repo, "remote.origin.fetch"),
        [
            "+refs/heads/*:refs/remotes/origin/*",
            "+refs/heads/x:refs/remotes/origin/x"
        ]
    );
}

#[test]
fn setting_a_key_replaces_all_its_values_and_an_unset_key_has_none() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(repo, "k.v", "1");
    fx.u.config_add(repo, "k.v", "2");
    fx.u.config_set(repo, "k.v", "3");
    assert_eq!(fx.u.config_values(repo, "k.v"), ["3"]);
    assert!(fx.u.config_values(repo, "k.other").is_empty());
}

#[test]
fn global_values_come_before_the_repos_own() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.set_global("core.x", "global");
    fx.u.config_add(repo, "core.x", "local");
    assert_eq!(fx.u.config_values(repo, "core.x"), ["global", "local"]);
}

#[test]
fn a_global_value_is_seen_by_every_repo_and_a_repos_own_by_none_other() {
    let mut fx = fx();
    let (a, _) = fx.repo();
    let (b, _) = fx.repo();
    fx.u.set_global("core.x", "global");
    fx.u.config_add(a, "core.y", "mine");
    assert_eq!(fx.u.config_values(b, "core.x"), ["global"]);
    assert!(fx.u.config_values(b, "core.y").is_empty());
}

#[test]
fn a_key_is_case_insensitive_in_its_section_and_variable_but_not_its_subsection() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(repo, "url.Base.insteadOf", "https://x/");
    assert_eq!(
        fx.u.config_values(repo, "URL.Base.INSTEADOF"),
        ["https://x/"]
    );
    assert!(fx.u.config_values(repo, "url.base.insteadOf").is_empty());
}

#[test]
fn a_url_is_rewritten_by_instead_of() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(
        repo,
        "url./tmp/origin.git.insteadOf",
        "https://github.com/acme/x.git",
    );
    assert_eq!(
        fx.u.resolve_url(repo, "https://github.com/acme/x.git"),
        "/tmp/origin.git"
    );
}

#[test]
fn instead_of_rewrites_a_prefix_and_keeps_the_rest() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(repo, "url./srv/git/.insteadOf", "https://github.com/acme/");
    assert_eq!(
        fx.u.resolve_url(repo, "https://github.com/acme/x.git"),
        "/srv/git/x.git"
    );
}

#[test]
fn the_longest_instead_of_prefix_wins() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(repo, "url./short/.insteadOf", "https://github.com/");
    fx.u.config_add(repo, "url./long/.insteadOf", "https://github.com/acme/");
    assert_eq!(
        fx.u.resolve_url(repo, "https://github.com/acme/x.git"),
        "/long/x.git"
    );
}

#[test]
fn a_url_no_instead_of_matches_is_kept() {
    let mut fx = fx();
    let (repo, _) = fx.repo();
    fx.u.config_add(
        repo,
        "url./tmp/o.git.insteadOf",
        "https://github.com/acme/x.git",
    );
    assert_eq!(
        fx.u.resolve_url(repo, "https://github.com/acme/y.git"),
        "https://github.com/acme/y.git"
    );
}

// ---- the index file ------------------------------------------------------------------

#[test]
fn staging_writes_a_real_index_file_in_the_git_dir() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let blob = fx.u.blob(repo, b"x");
    fx.u.stage(&dir, "a.txt", &blob, 0);
    assert_eq!(fx.u.index_path(&dir), dir.join(".git/index"));
    assert!(dir.join(".git/index").is_file());
}

#[test]
fn a_linked_worktrees_index_is_in_its_admin_dir() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let wt = fx.root().join("wt");
    fx.u.add_worktree(repo, &wt, Head::Branch("feat".to_owned()));
    let blob = fx.u.blob(repo, b"x");
    fx.u.stage(&wt, "a.txt", &blob, 0);
    assert_eq!(fx.u.index_path(&wt), dir.join(".git/worktrees/wt/index"));
    assert!(dir.join(".git/worktrees/wt/index").is_file());
}

#[test]
fn every_change_of_the_index_gives_the_file_a_new_inode() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let blob = fx.u.blob(repo, b"x");
    let path = dir.join(".git/index");
    fx.u.stage(&dir, "a.txt", &blob, 0);
    let first = std::fs::metadata(&path).unwrap().ino();
    fx.u.stage(&dir, "b.txt", &blob, 0);
    let second = std::fs::metadata(&path).unwrap().ino();
    assert_ne!(
        first, second,
        "the index is rewritten by rename, never in place"
    );
}

#[test]
fn the_index_lists_entries_by_path_then_stage_and_a_stage_replaces_only_itself() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let base = fx.u.blob(repo, b"base");
    let ours = fx.u.blob(repo, b"ours");
    let theirs = fx.u.blob(repo, b"theirs");
    fx.u.stage(&dir, "b.txt", &base, 0);
    fx.u.stage(&dir, "a.txt", &theirs, 3);
    fx.u.stage(&dir, "a.txt", &base, 1);
    fx.u.stage(&dir, "a.txt", &ours, 2);
    fx.u.stage(&dir, "a.txt", &theirs, 2);
    assert_eq!(
        fx.u.index(&dir),
        vec![
            ("a.txt".to_owned(), base.clone(), 1),
            ("a.txt".to_owned(), theirs.clone(), 2),
            ("a.txt".to_owned(), theirs, 3),
            ("b.txt".to_owned(), base, 0),
        ]
    );
}

// ---- revisions -----------------------------------------------------------------------

#[test]
fn head_resolves_to_the_branch_it_names() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    assert_eq!(parse(&fx.u, &dir, "HEAD"), Ok(c.clone()));
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.branch(repo, "main", &d);
    assert_eq!(parse(&fx.u, &dir, "HEAD"), Ok(d));
}

#[test]
fn head_of_an_unborn_branch_is_unknown() {
    let mut fx = fx();
    let (_, dir) = fx.repo();
    assert!(matches!(
        parse(&fx.u, &dir, "HEAD"),
        Err(RevError::Unknown(_))
    ));
}

#[test]
fn a_detached_head_resolves_to_its_commit() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.u.set_head(&dir, Head::Detached(d.clone()));
    assert_eq!(parse(&fx.u, &dir, "HEAD"), Ok(d));
}

#[test]
fn head_is_read_in_the_worktree_asked() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.branch(repo, "feat", &d);
    let wt = fx.root().join("wt");
    fx.u.add_worktree(repo, &wt, Head::Branch("feat".to_owned()));
    assert_eq!(parse(&fx.u, &wt, "HEAD"), Ok(d));
    assert_eq!(parse(&fx.u, &dir, "HEAD"), Ok(c));
}

#[test]
fn a_full_sha_of_an_object_the_repo_holds_resolves_to_itself() {
    let mut fx = fx();
    let (_, dir, c) = fx.on_main();
    assert_eq!(parse(&fx.u, &dir, c.as_str()), Ok(c));
}

#[test]
fn a_full_sha_followed_by_caret_commit_of_an_object_the_repo_does_not_hold_is_unknown() {
    let mut fx = fx();
    let (other, _) = fx.repo();
    let foreign = fx.commit(other, &[], &[("a", "a")], "elsewhere");
    let (_, dir, _) = fx.on_main();
    assert!(matches!(
        parse(&fx.u, &dir, &format!("{foreign}^{{commit}}")),
        Err(RevError::Unknown(_))
    ));
}

#[test]
fn a_branch_name_resolves_through_refs_heads() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.branch(repo, "feat", &d);
    assert_eq!(parse(&fx.u, &dir, "feat"), Ok(d.clone()));
    assert_eq!(parse(&fx.u, &dir, "refs/heads/feat"), Ok(d.clone()));
    assert_eq!(parse(&fx.u, &dir, "heads/feat"), Ok(d));
}

#[test]
fn a_remote_tracking_name_resolves_through_refs_remotes() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    fx.u.set_ref(repo, "refs/remotes/origin/main", &c, "fetch");
    assert_eq!(parse(&fx.u, &dir, "origin/main"), Ok(c.clone()));
    assert_eq!(parse(&fx.u, &dir, "refs/remotes/origin/main"), Ok(c));
}

#[test]
fn a_remote_name_alone_resolves_through_its_head_symref() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    fx.u.set_ref(repo, "refs/remotes/origin/main", &c, "fetch");
    fx.u.set_symbolic_ref(repo, "refs/remotes/origin/HEAD", "refs/remotes/origin/main");
    assert_eq!(parse(&fx.u, &dir, "origin"), Ok(c));
}

#[test]
fn a_tag_wins_over_a_branch_of_the_same_name() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.u.set_ref(repo, "refs/tags/v1", &c, "tag");
    fx.branch(repo, "v1", &d);
    assert_eq!(parse(&fx.u, &dir, "v1"), Ok(c));
}

#[test]
fn a_branch_wins_over_a_remote_tracking_ref_of_the_same_name() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.u.set_ref(repo, "refs/remotes/main", &d, "fetch");
    assert_eq!(parse(&fx.u, &dir, "main"), Ok(c));
}

#[test]
fn a_name_under_refs_wins_over_a_branch_of_the_same_name() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.u.set_ref(repo, "refs/main", &d, "x");
    assert_eq!(parse(&fx.u, &dir, "main"), Ok(d));
}

#[test]
fn a_name_that_exists_nowhere_is_unknown() {
    let mut fx = fx();
    let (_, dir, _) = fx.on_main();
    assert!(matches!(
        parse(&fx.u, &dir, "nope"),
        Err(RevError::Unknown(_))
    ));
}

#[test]
fn caret_commit_of_a_commit_is_itself_and_of_a_blob_is_a_wrong_type() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    assert_eq!(parse(&fx.u, &dir, "HEAD^{commit}"), Ok(c));
    let blob = fx.u.blob(repo, b"x");
    assert!(matches!(
        parse(&fx.u, &dir, &format!("{blob}^{{commit}}")),
        Err(RevError::WrongType(_))
    ));
}

#[test]
fn caret_tree_of_a_commit_is_its_tree_and_of_a_tree_is_itself() {
    let mut fx = fx();
    let (_, dir, c) = fx.on_main();
    let tree = fx.u.commit_data(&c).unwrap().tree;
    assert_eq!(parse(&fx.u, &dir, "HEAD^{tree}"), Ok(tree.clone()));
    assert_eq!(
        parse(&fx.u, &dir, &format!("{c}^{{tree}}")),
        Ok(tree.clone())
    );
    assert_eq!(parse(&fx.u, &dir, &format!("{tree}^{{tree}}")), Ok(tree));
}

#[test]
fn caret_tree_of_a_blob_is_a_wrong_type() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let blob = fx.u.blob(repo, b"x");
    assert!(matches!(
        parse(&fx.u, &dir, &format!("{blob}^{{tree}}")),
        Err(RevError::WrongType(_))
    ));
}

#[test]
fn a_rev_colon_path_is_the_blob_at_that_path() {
    let mut fx = fx();
    let (repo, dir, _) = fx.on_main();
    let c = fx.commit(
        repo,
        &[],
        &[("README.md", "one\n"), ("dir/sub/f.txt", "deep\n")],
        "two files",
    );
    fx.branch(repo, "main", &c);
    let deep = fx.u.blob(repo, b"deep\n");
    assert_eq!(parse(&fx.u, &dir, "HEAD:dir/sub/f.txt"), Ok(deep));
    assert_eq!(
        parse(&fx.u, &dir, &format!("{c}:README.md")),
        Ok(fx.u.blob(repo, b"one\n"))
    );
}

#[test]
fn a_rev_colon_a_path_not_in_the_tree_is_unknown() {
    let mut fx = fx();
    let (_, dir, _) = fx.on_main();
    assert!(matches!(
        parse(&fx.u, &dir, "HEAD:nope.txt"),
        Err(RevError::Unknown(_))
    ));
}

#[test]
fn two_dots_select_what_b_has_that_a_has_not() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.branch(repo, "feat", &d);
    assert_eq!(
        range(&fx.u, &dir, "main..feat"),
        Ok(Selection {
            include: vec![d],
            exclude: vec![c]
        })
    );
}

#[test]
fn three_dots_select_both_sides_minus_their_merge_base() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let on_main = fx.commit(repo, &[&c], &[("m", "m")], "on main");
    let on_feat = fx.commit(repo, &[&c], &[("f", "f")], "on feat");
    fx.branch(repo, "main", &on_main);
    fx.branch(repo, "feat", &on_feat);
    assert_eq!(
        range(&fx.u, &dir, "main...feat"),
        Ok(Selection {
            include: vec![on_main, on_feat],
            exclude: vec![c]
        })
    );
}

#[test]
fn a_range_side_left_empty_is_head() {
    let mut fx = fx();
    let (repo, dir, c) = fx.on_main();
    let d = fx.commit(repo, &[&c], &[("a", "a")], "d");
    fx.branch(repo, "feat", &d);
    assert_eq!(
        range(&fx.u, &dir, "main.."),
        Ok(Selection {
            include: vec![c.clone()],
            exclude: vec![c]
        })
    );
}

#[test]
fn a_range_with_an_unknown_side_is_unknown() {
    let mut fx = fx();
    let (_, dir, _) = fx.on_main();
    assert!(matches!(
        range(&fx.u, &dir, "main..nope"),
        Err(RevError::Unknown(_))
    ));
}

// ---- ancestry ------------------------------------------------------------------------

/// `base <- a1 <- a2` and `base <- b1`: returns (repo, base, a1, a2, b1).
fn forked(fx: &mut Fx) -> (RepoId, Oid, Oid, Oid, Oid) {
    let (repo, _, base) = fx.on_main();
    let a1 = fx.commit(repo, &[&base], &[("a", "1")], "a1");
    let a2 = fx.commit(repo, &[&a1], &[("a", "2")], "a2");
    let b1 = fx.commit(repo, &[&base], &[("b", "1")], "b1");
    (repo, base, a1, a2, b1)
}

#[test]
fn the_merge_base_of_two_branches_is_where_they_forked() {
    let mut fx = fx();
    let (repo, base, _, a2, b1) = forked(&mut fx);
    assert_eq!(fx.u.merge_base(repo, &a2, &b1), Some(base));
}

#[test]
fn the_merge_base_of_a_commit_and_its_descendant_is_the_commit() {
    let mut fx = fx();
    let (repo, _, a1, a2, _) = forked(&mut fx);
    assert_eq!(fx.u.merge_base(repo, &a1, &a2), Some(a1.clone()));
    assert_eq!(fx.u.merge_base(repo, &a2, &a1), Some(a1));
}

#[test]
fn unrelated_histories_have_no_merge_base() {
    let mut fx = fx();
    let (repo, _, base) = fx.on_main();
    let stranger = fx.commit(repo, &[], &[("z", "z")], "root two");
    assert_eq!(fx.u.merge_base(repo, &base, &stranger), None);
}

#[test]
fn an_ancestor_is_an_ancestor_and_a_descendant_is_not() {
    let mut fx = fx();
    let (repo, base, _, a2, _) = forked(&mut fx);
    assert_eq!(fx.u.is_ancestor(repo, &base, &a2), Ok(true));
    assert_eq!(fx.u.is_ancestor(repo, &a2, &base), Ok(false));
}

#[test]
fn a_commit_is_its_own_ancestor() {
    let mut fx = fx();
    let (repo, base, ..) = forked(&mut fx);
    assert_eq!(fx.u.is_ancestor(repo, &base, &base), Ok(true));
}

#[test]
fn a_sibling_is_not_an_ancestor() {
    let mut fx = fx();
    let (repo, _, _, a2, b1) = forked(&mut fx);
    assert_eq!(fx.u.is_ancestor(repo, &a2, &b1), Ok(false));
}

#[test]
fn an_ancestor_test_of_an_object_the_repo_does_not_hold_is_an_error_naming_it() {
    let mut fx = fx();
    let (repo, base, ..) = forked(&mut fx);
    let (other, _) = fx.repo();
    let foreign = fx.commit(other, &[], &[("q", "q")], "elsewhere");
    assert_eq!(
        fx.u.is_ancestor(repo, &base, &foreign),
        Err(UnknownObject(foreign.clone()))
    );
    assert_eq!(
        fx.u.is_ancestor(repo, &foreign, &base),
        Err(UnknownObject(foreign))
    );
}

#[test]
fn rev_list_walks_a_linear_history_newest_first() {
    let mut fx = fx();
    let (repo, base, a1, a2, _) = forked(&mut fx);
    let sel = Selection {
        include: vec![a2.clone()],
        exclude: vec![],
    };
    assert_eq!(fx.u.rev_list(repo, &sel, false), vec![a2, a1, base]);
}

#[test]
fn rev_list_not_leaves_out_what_the_excluded_side_reaches() {
    let mut fx = fx();
    let (repo, base, a1, a2, _) = forked(&mut fx);
    let sel = Selection {
        include: vec![a2.clone()],
        exclude: vec![base],
    };
    assert_eq!(fx.u.rev_list(repo, &sel, false), vec![a2, a1]);
}

#[test]
fn rev_list_of_two_dots_is_what_one_side_has_that_the_other_has_not() {
    let mut fx = fx();
    let (repo, _, a1, a2, b1) = forked(&mut fx);
    fx.branch(repo, "a", &a2);
    fx.branch(repo, "b", &b1);
    let dir = fx.root().join("repo1");
    let sel = range(&fx.u, &dir, "b..a").unwrap();
    assert_eq!(fx.u.rev_list(repo, &sel, false), vec![a2, a1]);
}

#[test]
fn rev_list_of_three_dots_is_each_sides_own_commits() {
    let mut fx = fx();
    let (repo, _, a1, a2, b1) = forked(&mut fx);
    fx.branch(repo, "a", &a2);
    fx.branch(repo, "b", &b1);
    let dir = fx.root().join("repo1");
    let sel = range(&fx.u, &dir, "a...b").unwrap();
    let mut got = fx.u.rev_list(repo, &sel, false);
    got.sort();
    let mut want = vec![a2, a1, b1];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn rev_list_no_merges_drops_the_commits_with_two_parents() {
    let mut fx = fx();
    let (repo, base, a1, _, b1) = forked(&mut fx);
    let merge = fx.commit(repo, &[&a1, &b1], &[("a", "1"), ("b", "1")], "merge");
    let sel = Selection {
        include: vec![merge.clone()],
        exclude: vec![base],
    };
    let with = fx.u.rev_list(repo, &sel, false);
    let without = fx.u.rev_list(repo, &sel, true);
    assert_eq!(with.len(), 3);
    assert!(with.contains(&merge));
    assert_eq!(without.len(), 2);
    assert!(!without.contains(&merge));
}

#[test]
fn rev_list_not_the_remotes_lists_what_is_unpushed() {
    let mut fx = fx();
    let (repo, _, base) = fx.on_main();
    fx.u.set_ref(repo, "refs/remotes/origin/main", &base, "fetch");
    let local = fx.commit(repo, &[&base], &[("a", "a")], "local");
    let sel = Selection {
        include: vec![local.clone()],
        exclude: fx.u.remote_tips(repo, "origin"),
    };
    assert_eq!(fx.u.rev_list(repo, &sel, true), vec![local]);
}

#[test]
fn rev_list_of_what_a_remote_already_has_is_empty() {
    let mut fx = fx();
    let (repo, _, base) = fx.on_main();
    fx.u.set_ref(repo, "refs/remotes/origin/main", &base, "fetch");
    let sel = Selection {
        include: vec![base],
        exclude: fx.u.remote_tips(repo, "origin"),
    };
    assert!(fx.u.rev_list(repo, &sel, true).is_empty());
}
