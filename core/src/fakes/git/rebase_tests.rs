//! `rebase <onto>`, `rebase --continue` and `rebase --abort` of `GitRoute` through a `FakeRunner`
//! (`FakeRunner::with_git`), and the product's `pr/rebase.rs::{rebase_onto_with, unresolved_with,
//! in_progress_with}` and `merge/mod.rs::rebase_with` against the fake.
//!
//! The expected texts are literals recorded once by hand from real git in a scratch directory. The git at
//! hand was 2.54.0 (Apple Git-157), not 2.43; none of these texts changed between the two as far as I know
//! (unverified on 2.43). What was recorded, and what the tests pin of it:
//!
//! - a clean rebase exits 0 and prints `Successfully rebased and updated refs/heads/<b>.` on stderr (after a
//!   `Rebasing (n/m)` progress that ends in `\r`, which is not pinned; a fast-forward prints no progress), or
//!   `... updated detached HEAD.` when `HEAD` is detached. A `HEAD` that already contains `onto` prints
//!   `Current branch <b> is up to date.` on STDOUT (`HEAD is up to date.` when detached) and exits 0.
//! - a conflict exits 1. STDOUT carries `Auto-merging <p>` and `CONFLICT (content): Merge conflict in <p>` (or
//!   `CONFLICT (add/add): ...`, `CONFLICT (modify/delete): <p> deleted in HEAD and modified in <sha7>
//!   (<subject>).  Version <sha7> (<subject>) of <p> left in tree.` and its mirror `deleted in <sha7>
//!   (<subject>) and modified in HEAD.  Version HEAD of <p> left in tree.`); stderr says `Could not apply
//!   <sha7>... ` (git 2.54 writes `error: could not apply` first and a `# ` before the subject; only the
//!   words every version shares are pinned). The markers are `<<<<<<< HEAD` and `>>>>>>> <sha7> (<subject>)`,
//!   `<subject>` the first line of the message. Stage 2 (ours) is `HEAD`'s version, stage 3 the commit's.
//! - the stop leaves `HEAD` detached, the branch where it was, `rebase-merge/orig-head` (the path
//!   `rev-parse --path-format=absolute --git-path rebase-merge/orig-head` prints) holding the starting tip
//!   and a newline, and no `rebase-merge/` once it ends. Linked worktrees keep it under
//!   `<git dir>/worktrees/<id>/`, the main one under `<git dir>/`.
//! - `--continue` with an unmerged path exits 1 and prints, on STDOUT, `<p>: needs merge` and `You must edit
//!   all merge conflicts and then` / `mark them as resolved using git add`. The Spec names `Committing is
//!   not possible because you have unmerged files` for this; that is the text of `git commit`, which real git
//!   does not print for `rebase --continue` (recorded: it prints the lines above), so the test follows git.
//!   `--continue` commits whatever is staged (markers included: the product checks those itself), keeps the
//!   author and the whole message of the commit, and goes on with the next one.
//! - `--continue` and `--abort` with no rebase: exit 128 `fatal: no rebase in progress`; a second `rebase` while
//!   one is stopped: exit 128 and `fatal: It seems that there is already a rebase-merge directory`; `rebase
//!   nonexist`: 128 `fatal: invalid upstream 'nonexist'`; with unstaged changes: 1 `error: cannot rebase: You
//!   have unstaged changes.`.
//! - `--abort` is a `reset --hard` to `orig-head` with `HEAD` back on the branch: the conflicted files, the
//!   stages and the edits of tracked files are gone, untracked files stay; it prints nothing.
//! - a merge commit of `onto..HEAD` is not replayed (no `--rebase-merges`): its side commits are, flattened;
//!   their order is not pinned.
//!
//! No test runs git (or any process), reads the wall clock or sleeps: every call is a `Command` handed to the
//! `FakeRunner`, which never starts it.

use std::path::PathBuf;

use super::scenario::*;
use super::{Oid, Repo};
use crate::error::ErrorCode;
use crate::merge::rebase_with;
use crate::pr::{in_progress_with, rebase_onto_with, unresolved_with};

type Files<'a> = &'a [(&'a str, &'a str)];

struct Scene {
    w: World,
    repo: Repo,
    /// The linked worktree on `topic`.
    lead: PathBuf,
    base: Oid,
    onto: Oid,
    /// The topic's commits, oldest first; `topic` is the last.
    tops: Vec<Oid>,
}

/// `base` on `main`; `main` then moves to `onto`; `topic` is `topic` commits over `base` (author, message,
/// every file of the commit), checked out in a linked worktree.
fn scene(base: Files, onto: Files, topic: &[(&str, &str, Files)]) -> Scene {
    let mut w = world();
    let repo = w.repo(base);
    let base_oid = w.rev(repo.dir(), "main");
    let onto_oid = w.snap(&repo, Some(&base_oid), onto, "main moves");
    w.branch(&repo, "main", &onto_oid);
    let mut tops: Vec<Oid> = Vec::new();
    for (author, message, files) in topic {
        let parent = tops.last().unwrap_or(&base_oid).clone();
        tops.push(w.by(&repo, author, Some(&parent), files, message));
    }
    let lead = w.worktree(&repo, "topic", tops.last().expect("a topic has a commit"));
    Scene { w, repo, lead, base: base_oid, onto: onto_oid, tops }
}

impl Scene {
    fn rebase(&self) -> Out {
        self.w.git_no_editor(&self.lead, &["rebase", "main"])
    }
    fn cont(&self) -> Out {
        self.w.git_no_editor(&self.lead, &["rebase", "--continue"])
    }
    fn abort(&self) -> Out {
        self.w.git_no_editor(&self.lead, &["rebase", "--abort"])
    }
    fn head(&self) -> Oid {
        self.w.rev(&self.lead, "HEAD")
    }
    fn tip(&self) -> Oid {
        self.w.rev(&self.lead, "refs/heads/topic")
    }
    /// The commits from the one over `stop` to `tip` following first parents, oldest first.
    fn chain(&self, tip: &Oid, stop: &Oid) -> Vec<Oid> {
        let mut out = Vec::new();
        let mut at = tip.clone();
        while &at != stop {
            out.push(at.clone());
            at = self.w.commit_of(&at).parents[0].clone();
        }
        out.reverse();
        out
    }
    fn attached(&self) -> Option<String> {
        let out = self.w.git(&self.lead, &["symbolic-ref", "-q", "HEAD"]);
        (out.code == 0).then(|| out.stdout.trim().to_owned())
    }
    fn in_progress(&self) -> bool {
        self.w.git_path(&self.lead, "rebase-merge").exists()
    }
    fn status(&self) -> String {
        self.w.ok(&self.lead, &["status", "--porcelain"])
    }
    fn unmerged(&self) -> String {
        self.w.ok(&self.lead, &["diff", "--name-only", "--diff-filter=U"])
    }
    fn file(&self, rev: &str, path: &str) -> Option<String> {
        self.w.file_at(&self.lead, rev, path)
    }
}

fn f8(edit: &str) -> String {
    numbered(8, &[(8, edit)])
}

/// The scene of the recorded conflicts. `base`: `f.txt` = 1..8, `g.txt`, `del.txt`. `main` edits line 8 of `f.txt`
/// to `MAIN` and deletes `del.txt`. `topic`: alice's `topic one` edits line 8 of `f.txt` to `TOPIC` (a content
/// conflict); bob's `topic two` adds `n.txt` and modifies `del.txt` (a modify/delete conflict, once the first is
/// resolved).
fn conflicts() -> Scene {
    let base = numbered(8, &[]);
    let (main_f, topic_f) = (f8("MAIN"), f8("TOPIC"));
    scene(
        &[("f.txt", &base), ("g.txt", "g\n"), ("del.txt", "del\n")],
        &[("f.txt", &main_f), ("g.txt", "g\n")],
        &[
            ("alice", "topic one\n\nwhy it edits the end", &[("f.txt", &topic_f), ("g.txt", "g\n"), ("del.txt", "del\n")]),
            (
                "bob",
                "topic two\n\nbody line",
                &[("f.txt", &topic_f), ("g.txt", "g\n"), ("del.txt", "del2\n"), ("n.txt", "new\n")],
            ),
        ],
    )
}

/// The same, but the commit that conflicts is the second: alice's `add n` is clean, bob's `edit f` conflicts.
fn late_conflict() -> Scene {
    let base = numbered(8, &[]);
    let (main_f, topic_f) = (f8("MAIN"), f8("TOPIC"));
    scene(
        &[("f.txt", &base), ("g.txt", "g\n")],
        &[("f.txt", &main_f), ("g.txt", "g\n")],
        &[
            ("alice", "add n", &[("f.txt", &base), ("g.txt", "g\n"), ("n.txt", "new\n")]),
            ("bob", "edit f", &[("f.txt", &topic_f), ("g.txt", "g\n"), ("n.txt", "new\n")]),
        ],
    )
}

/// `base` on `main`, `main` moved over it, and one topic commit that edits line 1 (alice) and one that adds `n.txt`.
fn clean() -> Scene {
    let base = numbered(8, &[]);
    let (main_f, topic_f) = (f8("MAIN"), numbered(8, &[(1, "A-topic")]));
    scene(
        &[("f.txt", &base), ("g.txt", "g\n")],
        &[("f.txt", &main_f), ("g.txt", "g\n")],
        &[
            ("alice", "topic edits a", &[("f.txt", &topic_f), ("g.txt", "g\n")]),
            ("bob", "topic adds n\n\nwith a body", &[("f.txt", &topic_f), ("g.txt", "g\n"), ("n.txt", "new\n")]),
        ],
    )
}

fn stopped_text(s: &Scene) -> String {
    format!("<<<<<<< HEAD\nMAIN\n=======\nTOPIC\n>>>>>>> {} (topic one)\n", short(&s.tops[0]))
}

fn resolve_first(s: &Scene) {
    s.w.write(&s.lead, "f.txt", &f8("RESOLVED"));
    s.w.add(&s.lead, "f.txt");
}

// ---- a clean rebase -------------------------------------------------------------------------------------

#[test]
fn a_clean_rebase_replays_every_commit_of_onto_to_head_over_onto() {
    let s = clean();
    let out = s.rebase();
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert!(out.stderr.contains("Successfully rebased and updated refs/heads/topic.\n"), "stderr: {:?}", out.stderr);
    let new = s.chain(&s.tip(), &s.onto);
    assert_eq!(new.len(), 2);
    assert_eq!(s.w.commit_of(&new[0]).parents, std::slice::from_ref(&s.onto));
    assert_eq!(s.w.commit_of(&new[1]).parents, [new[0].clone()]);
    assert!(new.iter().all(|c| !s.tops.contains(c)), "the replayed commits are new ones");
}

#[test]
fn the_replayed_commits_keep_their_author_and_their_whole_message() {
    let s = clean();
    s.rebase();
    let new = s.chain(&s.tip(), &s.onto);
    let (first, second) = (s.w.commit_of(&new[0]), s.w.commit_of(&new[1]));
    assert_eq!((first.author.as_str(), first.message.as_str()), ("alice <alice@x.invalid>", "topic edits a"));
    assert_eq!((second.author.as_str(), second.message.as_str()), ("bob <bob@x.invalid>", "topic adds n\n\nwith a body"));
}

#[test]
fn each_replayed_commit_holds_its_change_over_the_tree_of_onto() {
    let s = clean();
    s.rebase();
    let new = s.chain(&s.tip(), &s.onto);
    let merged = numbered(8, &[(1, "A-topic"), (8, "MAIN")]);
    assert_eq!(s.w.paths_at(&s.lead, new[0].as_str()), ["f.txt", "g.txt"]);
    assert_eq!(s.file(new[0].as_str(), "f.txt").unwrap(), merged);
    assert_eq!(s.w.paths_at(&s.lead, new[1].as_str()), ["f.txt", "g.txt", "n.txt"]);
    assert_eq!(s.file(new[1].as_str(), "f.txt").unwrap(), merged);
    assert_eq!(s.file(new[1].as_str(), "n.txt").unwrap(), "new\n");
}

#[test]
fn after_a_clean_rebase_head_is_still_on_the_branch_and_the_branch_is_the_new_tip() {
    let s = clean();
    s.rebase();
    assert_eq!(s.attached().as_deref(), Some("refs/heads/topic"));
    assert_eq!(s.head(), s.tip());
    assert_ne!(s.tip(), s.tops[1]);
}

#[test]
fn after_a_clean_rebase_the_working_tree_and_the_index_are_the_new_tree() {
    let s = clean();
    s.rebase();
    assert_eq!(s.w.read(&s.lead, "f.txt").unwrap(), numbered(8, &[(1, "A-topic"), (8, "MAIN")]));
    assert_eq!(s.w.read(&s.lead, "n.txt").unwrap(), "new\n");
    assert_eq!(s.status(), "");
    assert!(s.w.stages(&s.lead).iter().all(|(_, stage)| *stage == 0));
}

#[test]
fn a_clean_rebase_leaves_no_rebase_state() {
    let s = clean();
    s.rebase();
    assert!(!s.w.git_path(&s.lead, "rebase-merge").exists());
    assert!(!s.w.git_path(&s.lead, "rebase-apply").exists());
    assert!(!s.w.git_path(&s.lead, "rebase-merge/orig-head").exists());
}

#[test]
fn the_reflog_of_the_branch_ends_with_the_rebased_tree() {
    let s = clean();
    s.rebase();
    let trees = s.w.ok(&s.lead, &["log", "-g", "--format=%T", "refs/heads/topic"]);
    assert_eq!(trees.lines().next().unwrap(), s.w.rev(&s.lead, "HEAD^{tree}").as_str());
}

#[test]
fn a_replayed_commit_that_deletes_a_file_deletes_it_from_the_working_tree() {
    let base = numbered(8, &[]);
    let main_f = f8("MAIN");
    let s = scene(
        &[("f.txt", &base), ("g.txt", "g\n")],
        &[("f.txt", &main_f), ("g.txt", "g\n")],
        &[("alice", "drop g", &[("f.txt", &base)])],
    );
    let out = s.rebase();
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(s.w.paths_at(&s.lead, "HEAD"), ["f.txt"]);
    assert_eq!(s.w.read(&s.lead, "g.txt"), None);
    assert_eq!(s.status(), "");
}

#[test]
fn a_merge_commit_between_onto_and_head_is_not_replayed_its_side_commits_are() {
    let mut w = world();
    let eight = numbered(8, &[]);
    let repo = w.repo(&[("f.txt", &eight)]);
    let base = w.rev(repo.dir(), "main");
    let onto = w.snap(&repo, Some(&base), &[("f.txt", &f8("MAIN"))], "main moves");
    w.branch(&repo, "main", &onto);
    let t1 = w.by(&repo, "alice", Some(&base), &[("f.txt", &numbered(8, &[(1, "T")]))], "t1");
    let s1 = w.by(&repo, "bob", Some(&base), &[("f.txt", &numbered(8, &[(5, "S")]))], "s1");
    let merge = w.join(&repo, &[&t1, &s1], &[("f.txt", &numbered(8, &[(1, "T"), (5, "S")]))], "Merge s1");
    let lead = w.worktree(&repo, "topic", &merge);
    let out = w.git_no_editor(&lead, &["rebase", "main"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let head = w.rev(&lead, "HEAD");
    let mut messages = Vec::new();
    let mut at = head;
    while at != onto {
        let commit = w.commit_of(&at);
        assert_eq!(commit.parents.len(), 1, "a replayed history is linear");
        messages.push(commit.message);
        at = commit.parents[0].clone();
    }
    messages.sort();
    assert_eq!(messages, ["s1", "t1"]);
    assert_eq!(w.read(&lead, "f.txt").unwrap(), numbered(8, &[(1, "T"), (5, "S"), (8, "MAIN")]));
}

#[test]
fn a_rebase_in_the_main_worktree_keeps_its_state_in_the_git_dir() {
    let mut w = world();
    let eight = numbered(8, &[]);
    let repo = w.repo(&[("f.txt", &eight)]);
    let base = w.rev(repo.dir(), "main");
    let side = w.snap(&repo, Some(&base), &[("f.txt", &f8("SIDE"))], "side moves");
    w.branch(&repo, "side", &side);
    repo.write("f.txt", &f8("MAIN"));
    repo.commit("main edits");
    let out = w.git_no_editor(repo.dir(), &["rebase", "side"]);
    assert_eq!(out.code, 1);
    let git_dir = w.u(|u| u.git_dir(repo.id()));
    assert_eq!(w.git_path(repo.dir(), "rebase-merge"), git_dir.join("rebase-merge"));
    assert!(git_dir.join("rebase-merge/orig-head").exists());
}

// ---- fast-forward, up to date ---------------------------------------------------------------------------

#[test]
fn a_head_that_is_an_ancestor_of_onto_fast_forwards_to_it() {
    let mut w = world();
    let eight = numbered(8, &[]);
    let repo = w.repo(&[("f.txt", &eight), ("g.txt", "g\n")]);
    let base = w.rev(repo.dir(), "main");
    let lead = w.worktree(&repo, "topic", &base);
    let onto = w.snap(&repo, Some(&base), &[("f.txt", &f8("MAIN")), ("g.txt", "g\n"), ("n.txt", "n\n")], "main moves");
    w.branch(&repo, "main", &onto);
    let out = w.git_no_editor(&lead, &["rebase", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert!(out.stderr.contains("Successfully rebased and updated refs/heads/topic.\n"), "stderr: {:?}", out.stderr);
    assert_eq!(w.rev(&lead, "HEAD"), onto);
    assert_eq!(w.rev(&lead, "refs/heads/topic"), onto);
    assert_eq!(w.read(&lead, "n.txt").unwrap(), "n\n");
    assert_eq!(w.read(&lead, "f.txt").unwrap(), f8("MAIN"));
    assert_eq!(w.ok(&lead, &["status", "--porcelain"]), "");
}

#[test]
fn a_head_that_already_contains_onto_is_up_to_date_and_nothing_moves() {
    let s = clean();
    // `topic` is over `base`; make `main` an ancestor of it.
    let first = s.w.snap(&s.repo, Some(&s.base), &[("f.txt", &numbered(8, &[(1, "A-topic")])), ("g.txt", "g\n")], "another");
    s.w.branch(&s.repo, "main", &first);
    let top = s.w.snap(&s.repo, Some(&first), &[("f.txt", &numbered(8, &[(1, "A-topic")])), ("g.txt", "g\n"), ("z.txt", "z\n")], "above");
    s.w.branch(&s.repo, "topic", &top);
    s.w.u(|u| u.set_head(&s.lead, super::Head::Branch("topic".to_owned())));
    let before = s.tip();
    let out = s.rebase();
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (0, "Current branch topic is up to date.\n", ""));
    assert_eq!(s.tip(), before);
}

#[test]
fn a_branch_at_onto_is_up_to_date() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", "1\n")]);
    let base = w.rev(repo.dir(), "main");
    let lead = w.worktree(&repo, "topic", &base);
    let out = w.git_no_editor(&lead, &["rebase", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "Current branch topic is up to date.\n"));
    assert_eq!(w.rev(&lead, "HEAD"), base);
}

#[test]
fn a_detached_head_is_rebased_in_place_and_stays_detached() {
    let mut w = world();
    let eight = numbered(8, &[]);
    let repo = w.repo(&[("f.txt", &eight), ("g.txt", "g\n")]);
    let base = w.rev(repo.dir(), "main");
    let onto = w.snap(&repo, Some(&base), &[("f.txt", &f8("MAIN")), ("g.txt", "g\n")], "main moves");
    w.branch(&repo, "main", &onto);
    let top = w.by(&repo, "alice", Some(&base), &[("f.txt", &numbered(8, &[(1, "A")])), ("g.txt", "g\n")], "edit one");
    let gate = w.detached(&repo, &top);
    let out = w.git_no_editor(&gate, &["rebase", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert!(out.stderr.contains("Successfully rebased and updated detached HEAD.\n"), "stderr: {:?}", out.stderr);
    assert_eq!(w.git(&gate, &["symbolic-ref", "-q", "HEAD"]).code, 1);
    let head = w.rev(&gate, "HEAD");
    let commit = w.commit_of(&head);
    assert_eq!((commit.parents, commit.message.as_str(), commit.author.as_str()), (vec![onto.clone()], "edit one", "alice <alice@x.invalid>"));
    assert_eq!(w.read(&gate, "f.txt").unwrap(), numbered(8, &[(1, "A"), (8, "MAIN")]));
    assert_eq!(w.rev(repo.dir(), "refs/heads/main"), onto);
}

#[test]
fn a_detached_head_that_contains_onto_is_up_to_date() {
    let mut w = world();
    let repo = w.repo(&[("f.txt", "1\n")]);
    let base = w.rev(repo.dir(), "main");
    let gate = w.detached(&repo, &base);
    let out = w.git_no_editor(&gate, &["rebase", "main"]);
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (0, "HEAD is up to date.\n", ""));
}

#[test]
fn onto_may_be_a_remote_tracking_name() {
    let s = clean();
    s.w.tracking(&s.repo, "main", &s.onto);
    let out = s.w.git_no_editor(&s.lead, &["rebase", "origin/main"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(s.w.commit_of(&s.chain(&s.tip(), &s.onto)[0]).parents, std::slice::from_ref(&s.onto));
}

#[test]
fn onto_may_be_a_full_sha() {
    let s = clean();
    let out = s.w.git_no_editor(&s.lead, &["rebase", s.onto.as_str()]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(s.chain(&s.tip(), &s.onto).len(), 2);
}

// ---- what a rebase refuses -------------------------------------------------------------------------------

#[test]
fn rebasing_onto_a_name_that_is_nothing_is_128() {
    let s = clean();
    let out = s.w.git_no_editor(&s.lead, &["rebase", "nonexist"]);
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (128, "", "fatal: invalid upstream 'nonexist'\n"));
    assert_eq!(s.tip(), s.tops[1]);
}

#[test]
fn unstaged_changes_to_a_tracked_file_refuse_the_rebase_and_change_nothing() {
    let s = clean();
    s.w.write(&s.lead, "g.txt", "edited\n");
    let out = s.rebase();
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert!(out.stderr.contains("error: cannot rebase: You have unstaged changes.\n"), "stderr: {:?}", out.stderr);
    assert_eq!(s.tip(), s.tops[1]);
    assert!(!s.in_progress());
    assert_eq!(s.w.read(&s.lead, "g.txt").unwrap(), "edited\n");
}

#[test]
fn the_editor_options_of_the_product_are_accepted_by_rebase_and_continue() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    s.cont();
    assert_eq!(s.w.route.unsupported(), Vec::<String>::new());
}

#[test]
fn an_interactive_rebase_is_refused_and_recorded() {
    let s = clean();
    let out = s.w.git_no_editor(&s.lead, &["rebase", "-i", "main"]);
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (1, "", "fake-git: unsupported: rebase -i\n"));
    assert_eq!(s.w.route.unsupported(), ["rebase -i"]);
    assert_eq!(s.tip(), s.tops[1]);
}

#[test]
fn a_rebase_with_onto_is_refused_and_recorded() {
    let s = clean();
    let out = s.w.git_no_editor(&s.lead, &["rebase", "--onto", "main", "main"]);
    assert_eq!(out.code, 1);
    assert_eq!(s.w.route.unsupported(), ["rebase --onto"]);
}

#[test]
fn a_rebase_outside_a_repo_is_not_a_git_repository() {
    let w = world();
    let out = w.git(&w.root(), &["rebase", "main"]);
    assert_eq!((out.code, out.stderr.as_str()), (128, "fatal: not a git repository (or any of the parent directories): .git\n"));
}

// ---- a conflict -------------------------------------------------------------------------------------------

#[test]
fn a_conflict_stops_the_rebase_with_exit_1_and_gits_messages() {
    let s = conflicts();
    let out = s.rebase();
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout, "Auto-merging f.txt\nCONFLICT (content): Merge conflict in f.txt\n");
    assert!(out.stderr.contains(&format!("Could not apply {}... ", short(&s.tops[0]))), "stderr: {:?}", out.stderr);
}

#[test]
fn the_conflicting_file_holds_markers_labelled_head_and_the_commit() {
    let s = conflicts();
    s.rebase();
    assert_eq!(s.w.read(&s.lead, "f.txt").unwrap(), format!("1\n2\n3\n4\n5\n6\n7\n{}", stopped_text(&s)));
}

#[test]
fn the_index_of_a_conflict_holds_the_three_stages_of_the_path() {
    let s = conflicts();
    s.rebase();
    assert_eq!(
        s.w.stages(&s.lead),
        [("f.txt".to_owned(), 1), ("f.txt".to_owned(), 2), ("f.txt".to_owned(), 3), ("g.txt".to_owned(), 0)]
    );
    assert_eq!(s.w.staged(&s.lead, "f.txt", 1).unwrap(), numbered(8, &[]));
    assert_eq!(s.w.staged(&s.lead, "f.txt", 2).unwrap(), f8("MAIN"));
    assert_eq!(s.w.staged(&s.lead, "f.txt", 3).unwrap(), f8("TOPIC"));
}

#[test]
fn a_stopped_rebase_has_head_detached_at_onto_and_the_branch_where_it_was() {
    let s = conflicts();
    s.rebase();
    assert_eq!(s.attached(), None);
    assert_eq!(s.head(), s.onto);
    assert_eq!(s.tip(), s.tops[1]);
}

#[test]
fn a_stopped_rebase_writes_orig_head_where_rev_parse_git_path_says() {
    let s = conflicts();
    s.rebase();
    assert!(s.w.git_path(&s.lead, "rebase-merge").is_dir());
    let orig = s.w.git_path(&s.lead, "rebase-merge/orig-head");
    assert_eq!(std::fs::read_to_string(orig).unwrap(), format!("{}\n", s.tops[1]));
}

#[test]
fn the_rebase_state_of_a_linked_worktree_is_under_its_admin_dir() {
    let s = conflicts();
    s.rebase();
    let admin = s.w.u(|u| u.admin_dir(&s.lead));
    assert_eq!(s.w.git_path(&s.lead, "rebase-merge"), admin.join("rebase-merge"));
    assert!(admin.join("rebase-merge/orig-head").exists());
}

#[test]
fn status_and_diff_filter_u_name_the_conflicted_path() {
    let s = conflicts();
    s.rebase();
    assert_eq!(s.unmerged(), "f.txt\n");
    assert_eq!(s.status(), "UU f.txt\n");
}

#[test]
fn a_conflict_at_the_second_commit_leaves_the_first_replayed_and_head_on_it() {
    let s = late_conflict();
    let out = s.rebase();
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout, "Auto-merging f.txt\nCONFLICT (content): Merge conflict in f.txt\n");
    assert!(out.stderr.contains(&format!("Could not apply {}... ", short(&s.tops[1]))), "stderr: {:?}", out.stderr);
    let head = s.head();
    let first = s.w.commit_of(&head);
    assert_eq!((first.parents, first.message.as_str(), first.author.as_str()), (vec![s.onto.clone()], "add n", "alice <alice@x.invalid>"));
    assert_eq!(s.attached(), None);
    assert_eq!(s.tip(), s.tops[1]);
    assert_eq!(
        s.w.stages(&s.lead),
        [("f.txt".to_owned(), 1), ("f.txt".to_owned(), 2), ("f.txt".to_owned(), 3), ("g.txt".to_owned(), 0), ("n.txt".to_owned(), 0)]
    );
    assert_eq!(std::fs::read_to_string(s.w.git_path(&s.lead, "rebase-merge/orig-head")).unwrap(), format!("{}\n", s.tops[1]));
}

#[test]
fn a_file_the_commit_modified_and_onto_deleted_is_a_modify_delete_conflict_with_the_commits_version() {
    let base = numbered(8, &[]);
    let s = scene(
        &[("f.txt", &base), ("del.txt", "del\n")],
        &[("f.txt", &base)],
        &[("bob", "topic two", &[("f.txt", &base), ("del.txt", "del2\n")])],
    );
    let out = s.rebase();
    let sha = short(&s.tops[0]);
    assert_eq!(out.code, 1);
    assert_eq!(
        out.stdout,
        format!("CONFLICT (modify/delete): del.txt deleted in HEAD and modified in {sha} (topic two).  Version {sha} (topic two) of del.txt left in tree.\n")
    );
    assert_eq!(s.w.stages(&s.lead), [("del.txt".to_owned(), 1), ("del.txt".to_owned(), 3), ("f.txt".to_owned(), 0)]);
    assert_eq!(s.w.read(&s.lead, "del.txt").unwrap(), "del2\n");
    assert_eq!(s.status(), "DU del.txt\n");
}

#[test]
fn a_file_the_commit_deleted_and_onto_modified_is_a_modify_delete_conflict_with_heads_version() {
    let base = numbered(8, &[]);
    let s = scene(
        &[("f.txt", &base), ("del.txt", "del\n")],
        &[("f.txt", &base), ("del.txt", "del-main\n")],
        &[("alice", "drop del", &[("f.txt", &base)])],
    );
    let out = s.rebase();
    let sha = short(&s.tops[0]);
    assert_eq!(out.code, 1);
    assert_eq!(
        out.stdout,
        format!("CONFLICT (modify/delete): del.txt deleted in {sha} (drop del) and modified in HEAD.  Version HEAD of del.txt left in tree.\n")
    );
    assert_eq!(s.w.stages(&s.lead), [("del.txt".to_owned(), 1), ("del.txt".to_owned(), 2), ("f.txt".to_owned(), 0)]);
    assert_eq!(s.w.read(&s.lead, "del.txt").unwrap(), "del-main\n");
    assert_eq!(s.status(), "UD del.txt\n");
}

#[test]
fn a_file_added_by_onto_and_by_the_commit_is_an_add_add_conflict_with_two_stages() {
    let s = scene(
        &[("f.txt", "f\n")],
        &[("f.txt", "f\n"), ("api.txt", "main api\n")],
        &[("alice", "add api", &[("f.txt", "f\n"), ("api.txt", "topic api\n")])],
    );
    let out = s.rebase();
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout, "Auto-merging api.txt\nCONFLICT (add/add): Merge conflict in api.txt\n");
    assert_eq!(s.w.stages(&s.lead), [("api.txt".to_owned(), 2), ("api.txt".to_owned(), 3), ("f.txt".to_owned(), 0)]);
    assert_eq!(
        s.w.read(&s.lead, "api.txt").unwrap(),
        format!("<<<<<<< HEAD\nmain api\n=======\ntopic api\n>>>>>>> {} (add api)\n", short(&s.tops[0]))
    );
    assert_eq!(s.status(), "AA api.txt\n");
}

#[test]
fn a_second_rebase_while_one_is_stopped_is_refused_with_128() {
    let s = conflicts();
    s.rebase();
    let out = s.rebase();
    assert_eq!((out.code, out.stdout.as_str()), (128, ""));
    assert!(out.stderr.starts_with("fatal: It seems that there is already a rebase-merge directory"), "stderr: {:?}", out.stderr);
    assert_eq!(s.unmerged(), "f.txt\n");
}

#[test]
fn rebasing_again_after_an_abort_stops_at_the_same_place() {
    let s = conflicts();
    let first = s.rebase();
    s.abort();
    let again = s.rebase();
    assert_eq!((again.code, again.stdout.as_str()), (first.code, first.stdout.as_str()));
    assert_eq!(s.w.read(&s.lead, "f.txt").unwrap(), format!("1\n2\n3\n4\n5\n6\n7\n{}", stopped_text(&s)));
}

// ---- --continue -------------------------------------------------------------------------------------------

#[test]
fn continue_with_an_unmerged_path_exits_1_and_changes_nothing() {
    let s = conflicts();
    s.rebase();
    let out = s.cont();
    assert_eq!(out.code, 1);
    assert!(
        out.stdout.contains("f.txt: needs merge\nYou must edit all merge conflicts and then\nmark them as resolved using git add\n"),
        "stdout: {:?}",
        out.stdout
    );
    assert_eq!(s.head(), s.onto);
    assert!(s.in_progress());
    assert_eq!(s.unmerged(), "f.txt\n");
}

#[test]
fn continue_commits_the_index_with_the_original_author_and_message() {
    let s = late_conflict();
    s.rebase();
    s.w.write(&s.lead, "f.txt", &f8("RESOLVED"));
    s.w.add(&s.lead, "f.txt");
    let out = s.cont();
    assert_eq!(out.code, 0, "stdout: {} stderr: {}", out.stdout, out.stderr);
    assert!(out.stderr.contains("Successfully rebased and updated refs/heads/topic.\n"), "stderr: {:?}", out.stderr);
    let new = s.chain(&s.tip(), &s.onto);
    assert_eq!(new.len(), 2);
    let second = s.w.commit_of(&new[1]);
    assert_eq!((second.author.as_str(), second.message.as_str(), second.parents), ("bob <bob@x.invalid>", "edit f", vec![new[0].clone()]));
    assert_eq!(s.file(new[1].as_str(), "f.txt").unwrap(), f8("RESOLVED"));
    assert_eq!(s.file(new[1].as_str(), "n.txt").unwrap(), "new\n");
}

#[test]
fn continue_keeps_the_whole_message_of_a_commit_with_a_body() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    s.cont();
    let first = s.chain(&s.w.rev(&s.lead, "HEAD"), &s.onto);
    assert_eq!(first.len(), 1);
    let commit = s.w.commit_of(&first[0]);
    assert_eq!((commit.author.as_str(), commit.message.as_str()), ("alice <alice@x.invalid>", "topic one\n\nwhy it edits the end"));
}

#[test]
fn after_continue_head_is_back_on_the_branch_and_the_rebase_state_is_gone() {
    let s = late_conflict();
    s.rebase();
    resolve_first_late(&s);
    assert_eq!(s.cont().code, 0);
    assert_eq!(s.attached().as_deref(), Some("refs/heads/topic"));
    assert_eq!(s.head(), s.tip());
    assert!(!s.in_progress());
    assert!(!s.w.git_path(&s.lead, "rebase-merge/orig-head").exists());
    assert_eq!(s.unmerged(), "");
    assert_eq!(s.status(), "");
}

fn resolve_first_late(s: &Scene) {
    s.w.write(&s.lead, "f.txt", &f8("RESOLVED"));
    s.w.add(&s.lead, "f.txt");
}

#[test]
fn continue_puts_the_resolved_tree_in_the_reflog_of_the_branch() {
    let s = late_conflict();
    s.rebase();
    resolve_first_late(&s);
    s.cont();
    let trees = s.w.ok(&s.lead, &["log", "-g", "--format=%T", "refs/heads/topic"]);
    assert_eq!(trees.lines().next().unwrap(), s.w.rev(&s.lead, "HEAD^{tree}").as_str());
}

#[test]
fn continue_commits_a_staged_file_that_still_has_markers() {
    let s = late_conflict();
    s.rebase();
    s.w.add(&s.lead, "f.txt");
    assert_eq!(s.cont().code, 0);
    let text = s.file("HEAD", "f.txt").unwrap();
    assert!(text.contains("<<<<<<< HEAD\n") && text.contains(">>>>>>> "), "f.txt: {text}");
}

#[test]
fn continue_goes_on_with_the_next_commit_and_stops_again_when_it_conflicts() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    let out = s.cont();
    let sha = short(&s.tops[1]);
    assert_eq!(out.code, 1);
    assert!(
        out.stdout.contains(&format!(
            "CONFLICT (modify/delete): del.txt deleted in HEAD and modified in {sha} (topic two).  Version {sha} (topic two) of del.txt left in tree.\n"
        )),
        "stdout: {:?}",
        out.stdout
    );
    assert!(out.stderr.contains(&format!("Could not apply {sha}... ")), "stderr: {:?}", out.stderr);
    let first = s.w.commit_of(&s.head());
    assert_eq!((first.parents, first.message.as_str(), first.author.as_str()), (vec![s.onto.clone()], "topic one\n\nwhy it edits the end", "alice <alice@x.invalid>"));
    assert_eq!(s.file("HEAD", "f.txt").unwrap(), f8("RESOLVED"));
    assert_eq!(s.w.stages(&s.lead), [("del.txt".to_owned(), 1), ("del.txt".to_owned(), 3), ("f.txt".to_owned(), 0), ("g.txt".to_owned(), 0), ("n.txt".to_owned(), 0)]);
    assert_eq!(std::fs::read_to_string(s.w.git_path(&s.lead, "rebase-merge/orig-head")).unwrap(), format!("{}\n", s.tops[1]));
    assert_eq!(s.attached(), None);
    assert_eq!(s.status(), "DU del.txt\nA  n.txt\n");
}

#[test]
fn a_conflict_resolved_by_removing_the_file_leaves_it_out_of_the_commit() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    s.cont();
    s.w.remove(&s.lead, "del.txt");
    let out = s.cont();
    assert_eq!(out.code, 0, "stdout: {} stderr: {}", out.stdout, out.stderr);
    assert_eq!(s.w.paths_at(&s.lead, "HEAD"), ["f.txt", "g.txt", "n.txt"]);
    let second = s.w.commit_of(&s.tip());
    assert_eq!((second.author.as_str(), second.message.as_str()), ("bob <bob@x.invalid>", "topic two\n\nbody line"));
}

#[test]
fn a_conflict_resolved_by_adding_the_commits_file_keeps_it() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    s.cont();
    s.w.add(&s.lead, "del.txt");
    assert_eq!(s.cont().code, 0);
    assert_eq!(s.file("HEAD", "del.txt").unwrap(), "del2\n");
}

#[test]
fn continue_with_no_rebase_in_progress_is_128() {
    let s = clean();
    let out = s.cont();
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (128, "", "fatal: no rebase in progress\n"));
}

// ---- --abort ----------------------------------------------------------------------------------------------

#[test]
fn abort_with_no_rebase_in_progress_is_128() {
    let s = clean();
    let out = s.abort();
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (128, "", "fatal: no rebase in progress\n"));
}

#[test]
fn abort_prints_nothing_and_puts_head_back_on_the_branch_at_orig_head() {
    let s = conflicts();
    s.rebase();
    let out = s.abort();
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (0, "", ""));
    assert_eq!(s.attached().as_deref(), Some("refs/heads/topic"));
    assert_eq!(s.head(), s.tops[1]);
    assert_eq!(s.tip(), s.tops[1]);
}

#[test]
fn abort_restores_the_files_and_the_index() {
    let s = conflicts();
    s.rebase();
    s.abort();
    assert_eq!(s.w.read(&s.lead, "f.txt").unwrap(), f8("TOPIC"));
    assert_eq!(s.w.read(&s.lead, "del.txt").unwrap(), "del2\n");
    assert_eq!(s.w.read(&s.lead, "n.txt").unwrap(), "new\n");
    assert!(s.w.stages(&s.lead).iter().all(|(_, stage)| *stage == 0));
    assert_eq!(s.status(), "");
    assert_eq!(s.unmerged(), "");
}

#[test]
fn abort_removes_the_rebase_state() {
    let s = conflicts();
    s.rebase();
    s.abort();
    assert!(!s.in_progress());
    assert!(!s.w.git_path(&s.lead, "rebase-merge/orig-head").exists());
}

#[test]
fn abort_after_the_first_commit_was_replayed_drops_it_and_restores_the_original_tip() {
    let s = late_conflict();
    s.rebase();
    assert_ne!(s.head(), s.onto, "the first commit was replayed");
    s.abort();
    assert_eq!(s.head(), s.tops[1]);
    assert_eq!(s.tip(), s.tops[1]);
    assert_eq!(s.w.read(&s.lead, "f.txt").unwrap(), f8("TOPIC"));
    assert_eq!(s.w.read(&s.lead, "n.txt").unwrap(), "new\n");
    assert_eq!(s.status(), "");
}

#[test]
fn abort_leaves_untracked_files_and_discards_edits_to_tracked_ones() {
    let s = conflicts();
    s.rebase();
    s.w.write(&s.lead, "untracked.txt", "mine\n");
    s.w.write(&s.lead, "g.txt", "edited\n");
    assert_eq!(s.abort().code, 0);
    assert_eq!(s.w.read(&s.lead, "untracked.txt").unwrap(), "mine\n");
    assert_eq!(s.w.read(&s.lead, "g.txt").unwrap(), "g\n");
}

#[test]
fn abort_of_a_detached_rebase_puts_head_back_at_orig_head_detached() {
    let mut w = world();
    let eight = numbered(8, &[]);
    let repo = w.repo(&[("f.txt", &eight)]);
    let base = w.rev(repo.dir(), "main");
    let onto = w.snap(&repo, Some(&base), &[("f.txt", &f8("MAIN"))], "main moves");
    w.branch(&repo, "main", &onto);
    let top = w.by(&repo, "alice", Some(&base), &[("f.txt", &f8("TOPIC"))], "topic one");
    let gate = w.detached(&repo, &top);
    assert_eq!(w.git_no_editor(&gate, &["rebase", "main"]).code, 1);
    let out = w.git_no_editor(&gate, &["rebase", "--abort"]);
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (0, "", ""));
    assert_eq!(w.rev(&gate, "HEAD"), top);
    assert_eq!(w.git(&gate, &["symbolic-ref", "-q", "HEAD"]).code, 1);
    assert_eq!(w.read(&gate, "f.txt").unwrap(), f8("TOPIC"));
}

// ---- the product: pr/rebase.rs::rebase_onto_with ----------------------------------------------------------

/// A gate worktree (detached at the Lead Branch's tip) of a scene whose topic is the given commits.
fn gate_of(s: &mut Scene) -> PathBuf {
    let tip = s.tops.last().unwrap().clone();
    s.w.detached(&s.repo, &tip)
}

#[test]
fn rebase_onto_is_none_when_the_commits_replay_cleanly_and_head_is_the_rebased_tip() {
    let mut s = clean();
    let gate = gate_of(&mut s);
    assert_eq!(rebase_onto_with(&s.w.runner, &gate, "main").unwrap(), None);
    let head = s.w.rev(&gate, "HEAD");
    assert_ne!(head, s.tops[1]);
    assert_eq!(s.chain(&head, &s.onto).len(), 2);
    assert_eq!(s.w.read(&gate, "f.txt").unwrap(), numbered(8, &[(1, "A-topic"), (8, "MAIN")]));
    assert_eq!(s.w.route.unsupported(), Vec::<String>::new());
}

#[test]
fn rebase_onto_names_the_conflicted_files_and_aborts_the_rebase() {
    let mut s = conflicts();
    let gate = gate_of(&mut s);
    let tip = s.tops[1].clone();
    assert_eq!(rebase_onto_with(&s.w.runner, &gate, "main").unwrap(), Some(vec!["f.txt".to_owned()]));
    assert!(!s.w.git_path(&gate, "rebase-merge").exists());
    assert_eq!(s.w.rev(&gate, "HEAD"), tip);
    assert_eq!(s.w.git(&gate, &["symbolic-ref", "-q", "HEAD"]).code, 1);
    assert_eq!(s.w.read(&gate, "f.txt").unwrap(), f8("TOPIC"));
    assert_eq!(s.w.ok(&gate, &["status", "--porcelain"]), "");
}

#[test]
fn rebase_onto_names_every_conflicted_file_sorted() {
    let base_a = numbered(8, &[]);
    let (main_f, topic_f) = (f8("MAIN"), f8("TOPIC"));
    let mut s = scene(
        &[("z.txt", &base_a), ("a.txt", &base_a), ("ok.txt", "ok\n")],
        &[("z.txt", &main_f), ("a.txt", &main_f), ("ok.txt", "ok\n")],
        &[("alice", "edit all", &[("z.txt", &topic_f), ("a.txt", &topic_f), ("ok.txt", "ok topic\n")])],
    );
    let gate = gate_of(&mut s);
    assert_eq!(rebase_onto_with(&s.w.runner, &gate, "main").unwrap(), Some(vec!["a.txt".to_owned(), "z.txt".to_owned()]));
}

#[test]
fn rebase_onto_a_name_that_is_nothing_is_git_failed_without_a_conflict() {
    let mut s = clean();
    let gate = gate_of(&mut s);
    let error = rebase_onto_with(&s.w.runner, &gate, "nonexist").unwrap_err();
    assert_eq!(error.code(), ErrorCode::GitFailed);
    assert!(error.message().contains("without a conflict"), "message: {}", error.message());
    assert_eq!(error.details().get("exit").and_then(|v| v.as_i64()), Some(128));
}

#[test]
fn rebase_onto_a_base_the_commits_already_contain_is_none_and_moves_nothing() {
    let mut s = clean();
    let gate = gate_of(&mut s);
    let tip = s.tops[1].clone();
    assert_eq!(rebase_onto_with(&s.w.runner, &gate, s.base.as_str()).unwrap(), None);
    assert_eq!(s.w.rev(&gate, "HEAD"), tip);
}

// ---- the product: pr/rebase.rs::in_progress_with and unresolved_with ---------------------------------------

#[test]
fn in_progress_is_false_where_no_rebase_ran() {
    let s = clean();
    assert!(!in_progress_with(&s.w.runner, &s.lead).unwrap());
}

#[test]
fn in_progress_is_true_while_a_rebase_is_stopped() {
    let s = conflicts();
    s.rebase();
    assert!(in_progress_with(&s.w.runner, &s.lead).unwrap());
}

#[test]
fn in_progress_is_false_again_after_an_abort_and_after_the_last_continue() {
    let s = late_conflict();
    s.rebase();
    s.abort();
    assert!(!in_progress_with(&s.w.runner, &s.lead).unwrap());
    s.rebase();
    resolve_first_late(&s);
    assert_eq!(s.cont().code, 0);
    assert!(!in_progress_with(&s.w.runner, &s.lead).unwrap());
}

#[test]
fn in_progress_is_true_when_a_rebase_apply_directory_exists() {
    let s = clean();
    assert!(!in_progress_with(&s.w.runner, &s.lead).unwrap());
    std::fs::create_dir_all(s.w.git_path(&s.lead, "rebase-apply")).unwrap();
    assert!(in_progress_with(&s.w.runner, &s.lead).unwrap());
}

#[test]
fn in_progress_asks_the_git_dir_of_the_worktree_it_is_given() {
    let mut s = conflicts();
    s.rebase();
    let other = s.w.detached(&s.repo, &s.base);
    assert!(in_progress_with(&s.w.runner, &s.lead).unwrap());
    assert!(!in_progress_with(&s.w.runner, &other).unwrap());
}

#[test]
fn unresolved_is_empty_where_no_rebase_ran() {
    let s = clean();
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), Vec::<String>::new());
}

#[test]
fn unresolved_lists_the_unmerged_paths_of_a_stopped_rebase_once() {
    let s = conflicts();
    s.rebase();
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), ["f.txt"]);
}

#[test]
fn unresolved_is_empty_once_the_file_is_resolved_and_added() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), Vec::<String>::new());
}

#[test]
fn unresolved_lists_a_staged_file_that_still_holds_the_ours_marker() {
    let s = conflicts();
    s.rebase();
    s.w.add(&s.lead, "f.txt");
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), ["f.txt"]);
}

#[test]
fn unresolved_lists_a_changed_file_that_holds_only_the_theirs_marker() {
    let s = conflicts();
    s.rebase();
    s.w.write(&s.lead, "f.txt", &format!("{}>>>>>>> topic\n", numbered(7, &[])));
    s.w.add(&s.lead, "f.txt");
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), ["f.txt"]);
}

#[test]
fn unresolved_does_not_take_a_lone_separator_line_for_a_marker() {
    let s = conflicts();
    s.rebase();
    s.w.write(&s.lead, "f.txt", "Title\n=======\ntext\n");
    s.w.add(&s.lead, "f.txt");
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), Vec::<String>::new());
}

#[test]
fn unresolved_does_not_take_an_indented_marker_for_one() {
    let s = conflicts();
    s.rebase();
    s.w.write(&s.lead, "f.txt", "  <<<<<<< indented\nx\n  >>>>>>> indented\n");
    s.w.add(&s.lead, "f.txt");
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), Vec::<String>::new());
}

#[test]
fn unresolved_skips_a_file_deleted_in_the_working_tree() {
    let s = conflicts();
    s.rebase();
    resolve_first(&s);
    s.cont();
    s.w.remove(&s.lead, "del.txt");
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), Vec::<String>::new());
}

#[test]
fn unresolved_lists_the_union_of_unmerged_and_marked_files_sorted_without_repeats() {
    let base = numbered(8, &[]);
    let (main_f, topic_f) = (f8("MAIN"), f8("TOPIC"));
    let s = scene(
        &[("f.txt", &base), ("a.txt", "a\n"), ("m.txt", "m\n")],
        &[("f.txt", &main_f), ("a.txt", "a\n"), ("m.txt", "m\n")],
        &[("alice", "edit", &[("f.txt", &topic_f), ("a.txt", "a\n"), ("m.txt", "m topic\n")])],
    );
    s.rebase();
    assert_eq!(s.unmerged(), "f.txt\n");
    s.w.write(&s.lead, "m.txt", "<<<<<<< HEAD\nm\n=======\nm topic\n>>>>>>> x\n");
    s.w.add(&s.lead, "m.txt");
    s.w.write(&s.lead, "a.txt", ">>>>>>> stray\n");
    s.w.add(&s.lead, "a.txt");
    assert_eq!(unresolved_with(&s.w.runner, &s.lead).unwrap(), ["a.txt", "f.txt", "m.txt"]);
}

// ---- the product: merge/mod.rs::rebase_with ----------------------------------------------------------------

#[test]
fn the_merge_step_rebase_is_none_after_a_clean_replay_onto_a_sha() {
    let s = clean();
    assert_eq!(rebase_with(&s.w.runner, &s.lead, s.onto.as_str()).unwrap(), None);
    assert_eq!(s.chain(&s.tip(), &s.onto).len(), 2);
    assert_eq!(s.attached().as_deref(), Some("refs/heads/topic"));
    assert_eq!(s.w.route.unsupported(), Vec::<String>::new());
}

#[test]
fn the_merge_step_rebase_names_the_conflicted_files_and_aborts() {
    let s = conflicts();
    assert_eq!(rebase_with(&s.w.runner, &s.lead, s.onto.as_str()).unwrap(), Some(vec!["f.txt".to_owned()]));
    assert!(!s.in_progress());
    assert_eq!(s.attached().as_deref(), Some("refs/heads/topic"));
    assert_eq!(s.head(), s.tops[1]);
    assert_eq!(s.w.read(&s.lead, "f.txt").unwrap(), f8("TOPIC"));
    assert_eq!(s.status(), "");
}

#[test]
fn the_merge_step_rebase_names_every_conflicted_file_sorted() {
    let base = numbered(8, &[]);
    let (main_f, topic_f) = (f8("MAIN"), f8("TOPIC"));
    let s = scene(
        &[("z.txt", &base), ("a.txt", &base)],
        &[("z.txt", &main_f), ("a.txt", &main_f)],
        &[("alice", "edit both", &[("z.txt", &topic_f), ("a.txt", &topic_f)])],
    );
    assert_eq!(rebase_with(&s.w.runner, &s.lead, s.onto.as_str()).unwrap(), Some(vec!["a.txt".to_owned(), "z.txt".to_owned()]));
}

#[test]
fn the_merge_step_rebase_onto_a_sha_that_is_nothing_is_git_failed_without_a_conflict() {
    let s = clean();
    let error = rebase_with(&s.w.runner, &s.lead, "0123456789abcdef0123456789abcdef01234567").unwrap_err();
    assert_eq!(error.code(), ErrorCode::GitFailed);
    assert!(error.message().contains("without a conflict"), "message: {}", error.message());
    assert_eq!(error.details().get("exit").and_then(|v| v.as_i64()), Some(128));
    assert_eq!(s.tip(), s.tops[1]);
}

#[test]
fn the_merge_step_rebase_of_a_branch_that_contains_the_sha_is_none_and_moves_nothing() {
    let s = clean();
    assert_eq!(rebase_with(&s.w.runner, &s.lead, s.base.as_str()).unwrap(), None);
    assert_eq!(s.tip(), s.tops[1]);
}
