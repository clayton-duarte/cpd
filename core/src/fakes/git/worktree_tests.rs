//! `update-ref`, `branch`, `worktree`, `write-tree`, `read-tree` and the `index` file of `GitRoute`, through a
//! `FakeRunner` (`FakeRunner::with_git`), and rule S (`git/lead.rs::sync_lead_with`) against the fake.
//!
//! The expected texts are literals recorded once by hand from real git in a scratch directory (git 2.43's
//! texts; where a text could not be recorded, the test pins only its first words, and says so). No test
//! runs git (or any process), reads the wall clock or sleeps: every call is a `Command` handed to the
//! `FakeRunner`, which never starts it, and every nap rule S takes is a nap of the `ManualClock`.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{GitRoute, Oid, Repo, Universe};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::git::lead::{Synced, sync_lead_with};
use crate::host::deadline::Failed;
use crate::host::runner::{FakeRunner, Reply, Runner};

const ZEROS: &str = "0000000000000000000000000000000000000000";
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

struct World {
    tmp: TempDir,
    clock: ManualClock,
    universe: Arc<Mutex<Universe>>,
    route: Arc<GitRoute>,
    runner: FakeRunner,
    repos: usize,
}

fn world() -> World {
    let clock = ManualClock::new();
    let mut universe = Universe::new(Arc::new(clock.clone()));
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let runner = FakeRunner::new(clock.clone()).with_git(&route);
    World {
        tmp: tempfile::tempdir().unwrap(),
        clock,
        universe,
        route,
        runner,
        repos: 0,
    }
}

fn run(runner: &dyn Runner, cmd: &mut Command) -> Out {
    match runner.output(cmd, "git", Duration::from_secs(60)) {
        Ok(o) => Out {
            code: std::os::unix::process::ExitStatusExt::into_raw(o.status) >> 8,
            stdout: String::from_utf8(o.stdout).unwrap(),
            stderr: String::from_utf8(o.stderr).unwrap(),
        },
        Err(Failed::Io(e)) => panic!("git could not start: {e}"),
        Err(Failed::Timeout(e)) => panic!("git timed out: {e:?}"),
    }
}

fn p(path: &Path) -> &str {
    path.to_str().expect("a temp path is UTF-8")
}

impl World {
    /// The temp root, canonical (what real git prints for a path under it).
    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    fn u<R>(&self, f: impl FnOnce(&mut Universe) -> R) -> R {
        f(&mut self.universe.lock().unwrap())
    }
    /// A repo on the unborn branch `main`.
    fn empty(&mut self) -> Repo {
        self.repos += 1;
        Repo::init(
            &self.universe,
            &self.root().join(format!("repo{}", self.repos)),
        )
    }
    /// A repo on `main` with one commit, `README.md` = `one\n`.
    fn repo(&mut self) -> Repo {
        let repo = self.empty();
        repo.write("README.md", "one\n");
        repo.commit("one");
        repo
    }
    /// A commit of `files` over `parent` (or a root commit), moving no ref.
    fn commit_of(
        &self,
        repo: &Repo,
        parent: Option<&Oid>,
        files: &[(&str, &str)],
        message: &str,
    ) -> Oid {
        self.u(|u| {
            let blobs: Vec<(&str, Oid)> = files
                .iter()
                .map(|(p, t)| (*p, u.blob(repo.id(), t.as_bytes())))
                .collect();
            let entries: Vec<(&str, &Oid)> = blobs.iter().map(|(p, b)| (*p, b)).collect();
            let tree = u.tree(repo.id(), &entries);
            let parents: Vec<Oid> = parent.cloned().into_iter().collect();
            u.commit(repo.id(), &tree, &parents, message)
        })
    }
    /// A commit over `parent`: `README.md` = `two\n` and a new `b.txt`.
    fn second(&self, repo: &Repo, parent: &Oid) -> Oid {
        self.commit_of(
            repo,
            Some(parent),
            &[("README.md", "two\n"), ("b.txt", "b\n")],
            "two",
        )
    }
    /// `git -C <dir> <args...>`, the form of the product.
    fn git(&self, dir: &Path, args: &[&str]) -> Out {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        run(&self.runner, &mut cmd)
    }
    /// [`World::git`] that must exit 0; returns stdout.
    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git(dir, args);
        assert_eq!(out.code, 0, "git {args:?} said: {}", out.stderr);
        out.stdout
    }
    /// What `git rev-parse --verify <rev>` prints in `dir`.
    fn rev(&self, dir: &Path, rev: &str) -> String {
        self.ok(dir, &["rev-parse", "--verify", rev])
            .trim()
            .to_owned()
    }
    /// A linked worktree `name` on the new branch `branch` at `main`, made by `worktree add`.
    fn add(&self, repo: &Repo, name: &str, branch: &str) -> PathBuf {
        let path = self.root().join(name);
        self.ok(
            repo.dir(),
            &[
                "worktree",
                "add",
                "--no-track",
                "-b",
                branch,
                p(&path),
                "main",
            ],
        );
        path
    }
    fn index_of(&self, wt: &Path) -> PathBuf {
        self.u(|u| u.index_path(wt))
    }
    fn lock_of(&self, wt: &Path) -> PathBuf {
        let mut lock = self.index_of(wt).into_os_string();
        lock.push(".lock");
        PathBuf::from(lock)
    }
    /// The subcommand of every call the runner saw, in order.
    fn subcommands(&self) -> Vec<String> {
        self.runner
            .calls()
            .iter()
            .map(|call| {
                let mut args = call.args.iter();
                while let Some(arg) = args.next() {
                    match arg.as_str() {
                        "-C" | "-c" => {
                            args.next();
                        }
                        "--no-optional-locks" => {}
                        other => return other.to_owned(),
                    }
                }
                String::new()
            })
            .collect()
    }
}

// ---- update-ref -----------------------------------------------------------------------------------

#[test]
fn update_ref_moves_a_branch_and_prints_nothing() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let c2 = w.second(&repo, &Oid::new(&c1));
    let out = w.git(repo.dir(), &["update-ref", "refs/heads/main", c2.as_str()]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    assert_eq!(w.rev(repo.dir(), "refs/heads/main"), c2.to_string());
}

#[test]
fn update_ref_takes_a_revision_as_the_new_value() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(
        repo.dir(),
        &["update-ref", "refs/remotes/origin/main", "main"],
    );
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    assert_eq!(w.rev(repo.dir(), "origin/main"), w.rev(repo.dir(), "main"));
}

#[test]
fn update_ref_with_m_records_the_message_old_and_new_in_the_reflog() {
    let mut w = world();
    let repo = w.repo();
    let c1 = Oid::new(&w.rev(repo.dir(), "main"));
    let c2 = w.second(&repo, &c1);
    w.ok(
        repo.dir(),
        &[
            "update-ref",
            "-m",
            "merge: fast-forward",
            "refs/heads/main",
            c2.as_str(),
            c1.as_str(),
        ],
    );
    let log = w.u(|u| u.reflog(repo.id(), "refs/heads/main"));
    let tree2 = w.u(|u| u.commit_data(&c2).unwrap().tree);
    assert_eq!(log[0].message, "merge: fast-forward");
    assert_eq!(
        (log[0].old.clone(), log[0].new.clone(), log[0].tree.clone()),
        (Some(c1), c2, tree2.clone())
    );
    assert_eq!(
        w.ok(repo.dir(), &["log", "-g", "--format=%T", "refs/heads/main"])
            .lines()
            .next(),
        Some(tree2.as_str())
    );
}

#[test]
fn update_ref_without_m_still_logs_an_entry_with_an_empty_message() {
    let mut w = world();
    let repo = w.repo();
    let c2 = w.second(&repo, &Oid::new(&w.rev(repo.dir(), "main")));
    let before = w.u(|u| u.reflog(repo.id(), "refs/heads/main").len());
    w.ok(repo.dir(), &["update-ref", "refs/heads/main", c2.as_str()]);
    let log = w.u(|u| u.reflog(repo.id(), "refs/heads/main"));
    assert_eq!((log.len(), log[0].message.as_str()), (before + 1, ""));
}

#[test]
fn update_ref_with_the_all_zero_old_value_creates_a_ref_that_did_not_exist() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let out = w.git(
        repo.dir(),
        &["update-ref", "refs/heads/new", c1.as_str(), ZEROS],
    );
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    assert_eq!(w.rev(repo.dir(), "refs/heads/new"), c1);
    assert_eq!(
        w.u(|u| u.reflog(repo.id(), "refs/heads/new")[0].old.clone()),
        None
    );
}

#[test]
fn update_ref_with_a_stale_old_value_is_exit_128_cannot_lock_ref_and_moves_nothing() {
    let mut w = world();
    let repo = w.repo();
    let c1 = Oid::new(&w.rev(repo.dir(), "main"));
    let c2 = w.second(&repo, &c1);
    w.ok(repo.dir(), &["update-ref", "refs/heads/main", c2.as_str()]);
    // recorded: `git update-ref -m x refs/heads/main <c1> <c1>` while the branch is at <c2>
    let out = w.git(
        repo.dir(),
        &[
            "update-ref",
            "-m",
            "x",
            "refs/heads/main",
            c1.as_str(),
            c1.as_str(),
        ],
    );
    assert_eq!(out.code, 128);
    assert_eq!(
        out.stderr,
        format!(
            "fatal: update_ref failed for ref 'refs/heads/main': cannot lock ref 'refs/heads/main': is at {c2} but expected {c1}\n"
        )
    );
    assert_eq!(out.stdout, "");
    assert_eq!(w.rev(repo.dir(), "refs/heads/main"), c2.to_string());
}

#[test]
fn update_ref_with_an_old_value_for_a_ref_that_does_not_exist_is_exit_128_cannot_lock_ref() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    // the first words are recorded; the reason after the ref name is not pinned
    let out = w.git(
        repo.dir(),
        &["update-ref", "refs/heads/nope", c1.as_str(), c1.as_str()],
    );
    assert_eq!(out.code, 128);
    assert!(out.stderr.starts_with("fatal: update_ref failed for ref 'refs/heads/nope': cannot lock ref 'refs/heads/nope': "), "{}", out.stderr);
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/nope"]
        )
        .code,
        1
    );
}

#[test]
fn update_ref_with_the_all_zero_old_value_of_an_existing_ref_is_exit_128_cannot_lock_ref() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let out = w.git(
        repo.dir(),
        &["update-ref", "refs/heads/main", c1.as_str(), ZEROS],
    );
    assert_eq!(out.code, 128);
    assert!(out.stderr.starts_with("fatal: update_ref failed for ref 'refs/heads/main': cannot lock ref 'refs/heads/main': "), "{}", out.stderr);
}

#[test]
fn update_ref_to_an_object_the_repo_does_not_hold_is_exit_128_and_moves_nothing() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let out = w.git(
        repo.dir(),
        &[
            "update-ref",
            "refs/heads/main",
            "1111111111111111111111111111111111111111",
        ],
    );
    assert_eq!(out.code, 128);
    assert!(
        out.stderr
            .starts_with("fatal: update_ref failed for ref 'refs/heads/main': "),
        "{}",
        out.stderr
    );
    assert_eq!(w.rev(repo.dir(), "refs/heads/main"), c1);
}

#[test]
fn update_ref_of_a_branch_checked_out_in_a_linked_worktree_moves_that_worktrees_head() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    let c2 = w.second(&repo, &Oid::new(&w.rev(repo.dir(), "main")));
    w.ok(
        repo.dir(),
        &["update-ref", "-m", "move", "refs/heads/lead", c2.as_str()],
    );
    assert_eq!(w.rev(&wt, "HEAD"), c2.to_string());
    assert_eq!(
        w.ok(&wt, &["symbolic-ref", "--short", "-q", "HEAD"]),
        "lead\n"
    );
}

#[test]
fn update_ref_outside_a_repo_is_exit_128_not_a_git_repository() {
    let w = world();
    let out = w.git(&w.root(), &["update-ref", "refs/heads/x", ZEROS]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (
            128,
            "fatal: not a git repository (or any of the parent directories): .git\n"
        )
    );
}

#[test]
fn update_ref_with_a_flag_it_does_not_model_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["update-ref", "--stdin"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "fake-git: unsupported: update-ref --stdin\n")
    );
    assert_eq!(w.route.unsupported(), vec!["update-ref --stdin".to_owned()]);
}

// ---- branch -----------------------------------------------------------------------------------------

#[test]
fn branch_m_renames_the_ref_and_takes_the_reflog_with_it() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    repo.branch("old");
    let out = w.git(repo.dir(), &["branch", "-m", "old", "new"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    assert_eq!(w.rev(repo.dir(), "refs/heads/new"), c1);
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/old"]
        )
        .code,
        1
    );
    let tree = w.rev(repo.dir(), "main^{tree}");
    assert_eq!(
        w.ok(repo.dir(), &["log", "-g", "--format=%T", "refs/heads/new"])
            .lines()
            .next(),
        Some(tree.as_str())
    );
}

#[test]
fn branch_m_of_the_branch_checked_out_in_the_main_worktree_moves_its_head() {
    let mut w = world();
    let repo = w.repo();
    w.ok(repo.dir(), &["branch", "-m", "main", "trunk"]);
    assert_eq!(
        w.ok(repo.dir(), &["symbolic-ref", "--short", "-q", "HEAD"]),
        "trunk\n"
    );
}

#[test]
fn branch_m_of_the_branch_checked_out_in_a_linked_worktree_moves_that_worktrees_head() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    w.ok(repo.dir(), &["branch", "-m", "lead", "renamed"]);
    assert_eq!(
        w.ok(&wt, &["symbolic-ref", "--short", "-q", "HEAD"]),
        "renamed\n"
    );
    let list = w.ok(repo.dir(), &["worktree", "list", "--porcelain"]);
    assert!(list.contains("branch refs/heads/renamed\n"), "{list}");
}

#[test]
fn branch_m_to_a_name_that_exists_is_exit_128_and_changes_nothing() {
    let mut w = world();
    let repo = w.repo();
    repo.branch("old");
    repo.branch("taken");
    let out = w.git(repo.dir(), &["branch", "-m", "old", "taken"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: a branch named 'taken' already exists\n")
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/old"]
        )
        .code,
        0
    );
}

#[test]
fn branch_m_of_a_branch_that_does_not_exist_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    // the wording changed between git versions and was not recorded: only `fatal:` is pinned
    let out = w.git(repo.dir(), &["branch", "-m", "nope", "other"]);
    assert_eq!(out.code, 128);
    assert!(out.stderr.starts_with("fatal: "), "{}", out.stderr);
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/other"]
        )
        .code,
        1
    );
}

#[test]
fn branch_capital_d_deletes_the_branch_and_says_where_it_was() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    repo.branch("feat");
    let out = w.git(repo.dir(), &["branch", "-D", "feat"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr.as_str()),
        (0, format!("Deleted branch feat (was {}).\n", &c1[..7]), "")
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/feat"]
        )
        .code,
        1
    );
}

#[test]
fn branch_capital_d_deletes_an_unmerged_branch() {
    let mut w = world();
    let repo = w.repo();
    let c2 = w.second(&repo, &Oid::new(&w.rev(repo.dir(), "main")));
    w.u(|u| u.set_ref(repo.id(), "refs/heads/feat", &c2, "test"));
    assert_eq!(w.git(repo.dir(), &["branch", "-D", "feat"]).code, 0);
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/feat"]
        )
        .code,
        1
    );
}

#[test]
fn branch_capital_d_of_a_branch_that_does_not_exist_is_exit_1() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["branch", "-D", "nope"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "error: branch 'nope' not found.\n")
    );
}

#[test]
fn branch_d_deletes_a_branch_merged_into_head() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    repo.branch("feat");
    let c2 = w.second(&repo, &Oid::new(&c1));
    w.ok(repo.dir(), &["update-ref", "refs/heads/main", c2.as_str()]);
    let out = w.git(repo.dir(), &["branch", "-d", "feat"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr.as_str()),
        (0, format!("Deleted branch feat (was {}).\n", &c1[..7]), "")
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/feat"]
        )
        .code,
        1
    );
}

#[test]
fn branch_d_of_an_unmerged_branch_is_exit_1_not_fully_merged_and_keeps_it() {
    let mut w = world();
    let repo = w.repo();
    let c2 = w.second(&repo, &Oid::new(&w.rev(repo.dir(), "main")));
    w.u(|u| u.set_ref(repo.id(), "refs/heads/feat", &c2, "test"));
    let out = w.git(repo.dir(), &["branch", "-d", "feat"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    // recorded first line; the hints after it vary with the version and are not pinned
    assert_eq!(
        out.stderr.lines().next(),
        Some("error: the branch 'feat' is not fully merged.")
    );
    assert_eq!(w.rev(repo.dir(), "refs/heads/feat"), c2.to_string());
}

#[test]
fn branch_d_and_capital_d_of_a_branch_checked_out_in_a_linked_worktree_are_exit_1() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    for flag in ["-d", "-D"] {
        let out = w.git(repo.dir(), &["branch", flag, "lead"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr),
            (
                1,
                "",
                format!(
                    "error: Cannot delete branch 'lead' checked out at '{}'\n",
                    wt.display()
                )
            ),
            "{flag}"
        );
    }
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/lead"]
        )
        .code,
        0
    );
}

#[test]
fn branch_capital_d_of_the_branch_checked_out_in_the_main_worktree_is_exit_1() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "other", "other");
    let out = w.git(&wt, &["branch", "-D", "main"]);
    assert_eq!(
        (out.code, out.stderr),
        (
            1,
            format!(
                "error: Cannot delete branch 'main' checked out at '{}'\n",
                repo.dir().display()
            )
        )
    );
}

#[test]
fn branch_with_a_flag_it_does_not_model_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["branch", "--contains"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "fake-git: unsupported: branch --contains\n")
    );
    assert_eq!(w.route.unsupported(), vec!["branch --contains".to_owned()]);
}

// ---- worktree add ------------------------------------------------------------------------------------

#[test]
fn worktree_add_b_makes_a_directory_with_a_dot_git_file_pointing_at_its_admin_dir() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.root().join("lead");
    let out = w.git(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            "lead",
            p(&wt),
            "main",
        ],
    );
    // recorded (twin `worktree_round_trip`): `HEAD is now at` is on stdout, the first line for a new branch on stderr
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            format!("HEAD is now at {} one\n", &w.rev(repo.dir(), "main")[..7]).as_str()
        )
    );
    assert_eq!(
        out.stderr.lines().next(),
        Some("Preparing worktree (new branch 'lead')")
    );
    let admin = repo.dir().join(".git/worktrees/lead");
    assert_eq!(
        std::fs::read_to_string(wt.join(".git")).unwrap(),
        format!("gitdir: {}\n", admin.display())
    );
    assert!(admin.is_dir());
}

#[test]
fn worktree_add_b_checks_out_the_start_commit_into_real_files_and_a_clean_index() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    assert_eq!(
        std::fs::read_to_string(wt.join("README.md")).unwrap(),
        "one\n"
    );
    assert_eq!(w.rev(&wt, "HEAD"), w.rev(repo.dir(), "main"));
    assert_eq!(
        w.ok(&wt, &["symbolic-ref", "--short", "-q", "HEAD"]),
        "lead\n"
    );
    assert_eq!(w.ok(&wt, &["status", "--porcelain"]), "");
    let index = w.ok(
        &wt,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    assert_eq!(
        index,
        format!(
            "{}\n",
            repo.dir().join(".git/worktrees/lead/index").display()
        )
    );
    assert!(Path::new(index.trim()).is_file());
}

#[test]
fn worktree_add_b_creates_the_branch_at_the_start_and_logs_its_creation() {
    let mut w = world();
    let repo = w.repo();
    w.add(&repo, "lead", "lead");
    assert_eq!(
        w.rev(repo.dir(), "refs/heads/lead"),
        w.rev(repo.dir(), "main")
    );
    let log = w.u(|u| u.reflog(repo.id(), "refs/heads/lead"));
    assert_eq!(
        (log.len(), log[0].message.as_str()),
        (1, "branch: Created from main")
    );
    let tree = w.rev(repo.dir(), "main^{tree}");
    assert_eq!(
        w.ok(repo.dir(), &["log", "-g", "--format=%T", "refs/heads/lead"]),
        format!("{tree}\n")
    );
}

#[test]
fn worktree_add_b_at_a_sha_starts_the_branch_there() {
    let mut w = world();
    let repo = w.repo();
    let c1 = Oid::new(&w.rev(repo.dir(), "main"));
    let c2 = w.second(&repo, &c1);
    w.ok(repo.dir(), &["update-ref", "refs/heads/main", c2.as_str()]);
    let wt = w.root().join("attempt");
    w.ok(
        repo.dir(),
        &["worktree", "add", "-b", "attempt", p(&wt), c1.as_str()],
    );
    assert_eq!(w.rev(&wt, "HEAD"), c1.to_string());
    assert_eq!(w.rev(repo.dir(), "refs/heads/attempt"), c1.to_string());
    assert_eq!(
        std::fs::read_to_string(wt.join("README.md")).unwrap(),
        "one\n"
    );
    assert!(!wt.join("b.txt").exists());
}

#[test]
fn worktree_add_detach_lock_reason_makes_a_detached_locked_worktree() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let wt = w.root().join("gate");
    w.ok(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--detach",
            "--lock",
            "--reason",
            "cpd gate 4242",
            p(&wt),
            c1.as_str(),
        ],
    );
    assert_eq!(w.rev(&wt, "HEAD"), c1);
    assert_eq!(w.git(&wt, &["symbolic-ref", "-q", "HEAD"]).code, 1);
    let list = w.ok(repo.dir(), &["worktree", "list", "--porcelain"]);
    assert!(
        list.ends_with(&format!(
            "worktree {}\nHEAD {c1}\ndetached\nlocked cpd gate 4242\n\n",
            wt.display()
        )),
        "{list}"
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("README.md")).unwrap(),
        "one\n"
    );
}

#[test]
fn worktree_add_onto_an_existing_non_empty_path_is_exit_128_and_makes_no_branch() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.root().join("taken");
    std::fs::create_dir(&wt).unwrap();
    std::fs::write(wt.join("file.txt"), "x\n").unwrap();
    let out = w.git(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            "lead",
            p(&wt),
            "main",
        ],
    );
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr),
        (
            128,
            "",
            format!("fatal: '{}' already exists\n", wt.display())
        )
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/lead"]
        )
        .code,
        1
    );
    assert_eq!(
        w.ok(repo.dir(), &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

#[test]
fn worktree_add_onto_an_existing_empty_directory_works() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.root().join("empty");
    std::fs::create_dir(&wt).unwrap();
    assert_eq!(
        w.git(
            repo.dir(),
            &[
                "worktree",
                "add",
                "--no-track",
                "-b",
                "lead",
                p(&wt),
                "main"
            ]
        )
        .code,
        0
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("README.md")).unwrap(),
        "one\n"
    );
}

#[test]
fn worktree_add_b_of_a_taken_branch_is_exit_128_and_makes_no_directory() {
    let mut w = world();
    let repo = w.repo();
    repo.branch("lead");
    let wt = w.root().join("lead");
    let out = w.git(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            "lead",
            p(&wt),
            "main",
        ],
    );
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", "fatal: a branch named 'lead' already exists\n")
    );
    assert!(!wt.exists());
}

#[test]
fn worktree_add_of_a_start_that_is_not_a_revision_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.root().join("lead");
    let out = w.git(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            "lead",
            p(&wt),
            "nope",
        ],
    );
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: invalid reference: nope\n")
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/lead"]
        )
        .code,
        1
    );
}

#[test]
fn two_worktrees_of_one_name_get_two_admin_dirs() {
    let mut w = world();
    let repo = w.repo();
    let first = w.add(&repo, "wt", "one");
    let nested = w.root().join("elsewhere");
    std::fs::create_dir(&nested).unwrap();
    let second = nested.join("wt");
    w.ok(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            "two",
            p(&second),
            "main",
        ],
    );
    assert_eq!(
        std::fs::read_to_string(first.join(".git")).unwrap(),
        format!(
            "gitdir: {}\n",
            repo.dir().join(".git/worktrees/wt").display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(second.join(".git")).unwrap(),
        format!(
            "gitdir: {}\n",
            repo.dir().join(".git/worktrees/wt1").display()
        )
    );
}

#[test]
fn worktree_with_a_subcommand_or_flag_it_does_not_model_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.root().join("lead");
    assert_eq!(
        w.git(repo.dir(), &["worktree", "move", p(&wt), "x"]).code,
        1
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["worktree", "add", "--orphan", "-b", "x", p(&wt)]
        )
        .code,
        1
    );
    assert_eq!(
        w.route.unsupported(),
        vec![
            "worktree move".to_owned(),
            "worktree add --orphan".to_owned()
        ]
    );
}

// ---- worktree list ------------------------------------------------------------------------------------

#[test]
fn worktree_list_porcelain_of_one_worktree_is_one_block_ending_in_a_blank_line() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let out = w.git(repo.dir(), &["worktree", "list", "--porcelain"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr.as_str()),
        (
            0,
            format!(
                "worktree {}\nHEAD {c1}\nbranch refs/heads/main\n\n",
                repo.dir().display()
            ),
            ""
        )
    );
}

#[test]
fn worktree_list_porcelain_puts_the_main_worktree_first_then_the_linked_ones_by_the_name_of_their_admin_dir()
 {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let (b, a) = (w.add(&repo, "b", "feat-b"), w.add(&repo, "a", "feat-a"));
    let expected = format!(
        "worktree {}\nHEAD {c1}\nbranch refs/heads/main\n\nworktree {}\nHEAD {c1}\nbranch refs/heads/feat-a\n\nworktree {}\nHEAD {c1}\nbranch refs/heads/feat-b\n\n",
        repo.dir().display(),
        a.display(),
        b.display()
    );
    assert_eq!(
        w.ok(repo.dir(), &["worktree", "list", "--porcelain"]),
        expected
    );
    assert_eq!(
        w.ok(&a, &["worktree", "list", "--porcelain"]),
        expected,
        "from a linked worktree"
    );
}

#[test]
fn worktree_list_porcelain_of_an_unborn_branch_has_an_all_zero_head() {
    let mut w = world();
    let repo = w.empty();
    assert_eq!(
        w.ok(repo.dir(), &["worktree", "list", "--porcelain"]),
        format!(
            "worktree {}\nHEAD {ZEROS}\nbranch refs/heads/main\n\n",
            repo.dir().display()
        )
    );
}

#[test]
fn worktree_list_porcelain_prints_the_canonical_path_when_the_directory_resolves_through_a_symlink()
{
    // the same rule as macOS `/var` -> `/private/var`, made without depending on the platform
    let mut w = world();
    let repo = w.repo();
    let real = w.root().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = w.root().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let wt = link.join("wt");
    w.ok(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            "lead",
            p(&wt),
            "main",
        ],
    );
    let list = w.ok(repo.dir(), &["worktree", "list", "--porcelain"]);
    assert!(
        list.contains(&format!("worktree {}\n", real.join("wt").display())),
        "{list}"
    );
    assert!(!list.contains("link"), "{list}");
}

// ---- worktree remove, prune -------------------------------------------------------------------------

#[test]
fn worktree_remove_of_a_clean_worktree_deletes_its_directory_and_admin_dir_and_keeps_the_branch() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    let out = w.git(repo.dir(), &["worktree", "remove", p(&wt)]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    assert!(!wt.exists());
    assert!(!repo.dir().join(".git/worktrees/lead").exists());
    assert_eq!(
        w.ok(repo.dir(), &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/lead"]
        )
        .code,
        0
    );
}

#[test]
fn worktree_remove_of_a_tree_with_a_modified_file_without_force_is_exit_128_and_keeps_everything() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    std::fs::write(wt.join("README.md"), "changed\n").unwrap();
    let out = w.git(repo.dir(), &["worktree", "remove", p(&wt)]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr),
        (
            128,
            "",
            format!(
                "fatal: '{}' contains modified or untracked files, use --force to delete it\n",
                wt.display()
            )
        )
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("README.md")).unwrap(),
        "changed\n"
    );
    assert!(repo.dir().join(".git/worktrees/lead").is_dir());
}

#[test]
fn worktree_remove_of_a_tree_with_an_untracked_file_without_force_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    std::fs::write(wt.join("new.txt"), "x\n").unwrap();
    let out = w.git(repo.dir(), &["worktree", "remove", p(&wt)]);
    assert_eq!(
        (out.code, out.stderr),
        (
            128,
            format!(
                "fatal: '{}' contains modified or untracked files, use --force to delete it\n",
                wt.display()
            )
        )
    );
    assert!(wt.join("new.txt").exists());
}

#[test]
fn worktree_remove_force_deletes_a_dirty_worktree() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    std::fs::write(wt.join("README.md"), "changed\n").unwrap();
    std::fs::write(wt.join("new.txt"), "x\n").unwrap();
    let out = w.git(repo.dir(), &["worktree", "remove", "--force", p(&wt)]);
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    assert!(!wt.exists());
    assert!(!repo.dir().join(".git/worktrees/lead").exists());
}

#[test]
fn worktree_remove_of_a_locked_worktree_needs_force_twice() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let wt = w.root().join("gate");
    w.ok(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--detach",
            "--lock",
            "--reason",
            "cpd gate 7",
            p(&wt),
            c1.as_str(),
        ],
    );
    let refused = "fatal: cannot remove a locked working tree, lock reason: cpd gate 7\nuse 'remove -f -f' to override or unlock first\n";
    for args in [
        vec!["worktree", "remove", p(&wt)],
        vec!["worktree", "remove", "--force", p(&wt)],
    ] {
        let out = w.git(repo.dir(), &args);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (128, "", refused),
            "{args:?}"
        );
        assert!(wt.is_dir(), "{args:?}");
    }
    let out = w.git(
        repo.dir(),
        &["worktree", "remove", "--force", "--force", p(&wt)],
    );
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    assert!(!wt.exists());
    assert!(!repo.dir().join(".git/worktrees/gate").exists());
}

#[test]
fn worktree_remove_of_the_main_worktree_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["worktree", "remove", p(repo.dir())]);
    assert_eq!(
        (out.code, out.stderr),
        (
            128,
            format!("fatal: '{}' is a main working tree\n", repo.dir().display())
        )
    );
    assert!(repo.dir().join("README.md").exists());
}

#[test]
fn worktree_remove_of_a_path_that_is_no_worktree_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let nowhere = w.root().join("nowhere");
    let out = w.git(repo.dir(), &["worktree", "remove", p(&nowhere)]);
    assert_eq!(
        (out.code, out.stderr),
        (
            128,
            format!("fatal: '{}' is not a working tree\n", nowhere.display())
        )
    );
}

#[test]
fn worktree_prune_forgets_a_worktree_whose_directory_is_gone_and_prints_nothing() {
    let mut w = world();
    let repo = w.repo();
    let gone = w.add(&repo, "gone", "gone");
    let kept = w.add(&repo, "kept", "kept");
    std::fs::remove_dir_all(&gone).unwrap();
    let out = w.git(repo.dir(), &["worktree", "prune"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    let list = w.ok(repo.dir(), &["worktree", "list", "--porcelain"]);
    assert!(!list.contains(p(&gone)), "{list}");
    assert!(
        list.contains(&format!("worktree {}\n", kept.display())),
        "{list}"
    );
    assert!(!repo.dir().join(".git/worktrees/gone").exists());
    assert!(repo.dir().join(".git/worktrees/kept").is_dir());
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "refs/heads/gone"]
        )
        .code,
        0
    );
}

#[test]
fn worktree_prune_keeps_a_locked_worktree_whose_directory_is_gone() {
    let mut w = world();
    let repo = w.repo();
    let c1 = w.rev(repo.dir(), "main");
    let held = w.root().join("held");
    w.ok(
        repo.dir(),
        &[
            "worktree",
            "add",
            "--detach",
            "--lock",
            "--reason",
            "cpd gate 1",
            p(&held),
            c1.as_str(),
        ],
    );
    std::fs::remove_dir_all(&held).unwrap();
    w.ok(repo.dir(), &["worktree", "prune"]);
    assert!(repo.dir().join(".git/worktrees/held").is_dir());
    assert!(
        w.ok(repo.dir(), &["worktree", "list", "--porcelain"])
            .contains(&format!("worktree {}\n", held.display()))
    );
}

// ---- write-tree ----------------------------------------------------------------------------------------

#[test]
fn write_tree_prints_the_tree_of_the_index() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["write-tree"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr.as_str()),
        (0, format!("{}\n", w.rev(repo.dir(), "HEAD^{tree}")), "")
    );
}

#[test]
fn write_tree_of_an_empty_index_is_the_empty_tree() {
    let mut w = world();
    let repo = w.empty();
    assert_eq!(w.ok(repo.dir(), &["write-tree"]), format!("{EMPTY_TREE}\n"));
}

#[test]
fn write_tree_stores_the_tree_of_a_staged_file() {
    let mut w = world();
    let repo = w.repo();
    repo.write("b.txt", "b\n");
    repo.add("b.txt");
    let tree = w.ok(repo.dir(), &["write-tree"]).trim().to_owned();
    assert_ne!(tree, w.rev(repo.dir(), "HEAD^{tree}"));
    assert_eq!(w.git(repo.dir(), &["cat-file", "-e", &tree]).code, 0);
    assert_eq!(
        w.ok(repo.dir(), &["ls-tree", "-r", "--name-only", &tree]),
        "README.md\nb.txt\n"
    );
}

#[test]
fn write_tree_reads_the_index_of_the_worktree_it_runs_in() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    repo.write("b.txt", "b\n");
    repo.add("b.txt");
    assert_eq!(
        w.ok(&wt, &["write-tree"]),
        format!("{}\n", w.rev(&wt, "HEAD^{tree}"))
    );
    assert_ne!(
        w.ok(repo.dir(), &["write-tree"]),
        w.ok(&wt, &["write-tree"])
    );
}

#[test]
fn write_tree_with_unmerged_entries_is_exit_128_and_names_the_path() {
    let mut w = world();
    let repo = w.repo();
    let (base, ours, theirs) = w.u(|u| {
        (
            u.blob(repo.id(), b"base\n"),
            u.blob(repo.id(), b"ours\n"),
            u.blob(repo.id(), b"theirs\n"),
        )
    });
    // a conflicted index has the stages 1 to 3 of the path and no stage 0
    w.u(|u| {
        u.set_index(
            repo.dir(),
            vec![
                ("README.md".to_owned(), base, 1),
                ("README.md".to_owned(), ours, 2),
                ("README.md".to_owned(), theirs, 3),
            ],
        )
    });
    let out = w.git(repo.dir(), &["write-tree"]);
    assert_eq!((out.code, out.stdout.as_str()), (128, ""));
    assert!(
        out.stderr.starts_with("error: README.md: unmerged ("),
        "{}",
        out.stderr
    );
    assert_eq!(
        out.stderr.lines().last(),
        Some("fatal: git-write-tree: error building trees")
    );
}

#[test]
fn write_tree_while_index_lock_exists_is_exit_128_unable_to_create_the_lock() {
    let mut w = world();
    let repo = w.repo();
    let lock = w.lock_of(repo.dir());
    std::fs::write(&lock, "").unwrap();
    let out = w.git(repo.dir(), &["write-tree"]);
    assert_eq!((out.code, out.stdout.as_str()), (128, ""));
    // recorded first line; the paragraph after it is not pinned. The product tells "busy" by `index.lock` in stderr.
    assert_eq!(
        out.stderr.lines().next(),
        Some(format!("fatal: Unable to create '{}': File exists.", lock.display()).as_str())
    );
}

#[test]
fn write_tree_with_a_flag_it_does_not_model_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["write-tree", "--prefix=docs/"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "fake-git: unsupported: write-tree --prefix=docs/\n")
    );
    assert_eq!(
        w.route.unsupported(),
        vec!["write-tree --prefix=docs/".to_owned()]
    );
}

// ---- read-tree -m -u ---------------------------------------------------------------------------------

/// A Lead worktree `lead` on its branch, with the branch moved to a second commit by `update-ref` in the
/// main checkout (the worktree and its index still hold the first): `(world, repo, lead, c1, c2)`.
fn moved_lead() -> (World, Repo, PathBuf, Oid, Oid) {
    let mut w = world();
    let repo = w.repo();
    let lead = w.add(&repo, "lead", "lead");
    let c1 = Oid::new(&w.rev(repo.dir(), "main"));
    let c2 = w.second(&repo, &c1);
    w.ok(
        repo.dir(),
        &[
            "update-ref",
            "-m",
            "merge: fast-forward",
            "refs/heads/lead",
            c2.as_str(),
            c1.as_str(),
        ],
    );
    (w, repo, lead, c1, c2)
}

fn tree(w: &World, dir: &Path, commit: &Oid) -> String {
    w.rev(dir, &format!("{commit}^{{tree}}"))
}

#[test]
fn read_tree_m_u_moves_the_index_and_the_working_tree_from_the_first_tree_to_the_second() {
    let (w, _repo, lead, c1, c2) = moved_lead();
    let (from, to) = (tree(&w, &lead, &c1), tree(&w, &lead, &c2));
    let out = w.git(&lead, &["read-tree", "-m", "-u", &from, &to]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "two\n"
    );
    assert_eq!(std::fs::read_to_string(lead.join("b.txt")).unwrap(), "b\n");
    assert_eq!(w.ok(&lead, &["status", "--porcelain"]), "");
    assert_eq!(w.ok(&lead, &["write-tree"]), format!("{to}\n"));
}

#[test]
fn read_tree_m_u_takes_a_commit_as_the_second_argument_as_the_product_does() {
    let (w, _repo, lead, c1, c2) = moved_lead();
    let from = tree(&w, &lead, &c1);
    w.ok(&lead, &["read-tree", "-m", "-u", &from, c2.as_str()]);
    assert_eq!(
        w.ok(&lead, &["write-tree"]),
        format!("{}\n", tree(&w, &lead, &c2))
    );
}

#[test]
fn read_tree_m_u_deletes_a_file_the_second_tree_does_not_have() {
    let (w, repo, lead, c1, _c2) = moved_lead();
    let c3 = w.commit_of(&repo, Some(&c1), &[], "empty");
    let (from, to) = (tree(&w, &lead, &c1), tree(&w, &lead, &c3));
    w.ok(&lead, &["read-tree", "-m", "-u", &from, &to]);
    assert!(!lead.join("README.md").exists());
    assert_eq!(w.ok(&lead, &["write-tree"]), format!("{EMPTY_TREE}\n"));
}

#[test]
fn read_tree_m_u_refuses_to_overwrite_a_file_with_local_changes_and_leaves_it_alone() {
    let (w, _repo, lead, c1, c2) = moved_lead();
    std::fs::write(lead.join("README.md"), "mine\n").unwrap();
    let (from, to) = (tree(&w, &lead, &c1), tree(&w, &lead, &c2));
    let out = w.git(&lead, &["read-tree", "-m", "-u", &from, &to]);
    // only the failure and the path are pinned: the exact wording was not recorded
    assert_ne!(out.code, 0);
    assert!(out.stderr.contains("README.md"), "{}", out.stderr);
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "mine\n"
    );
    assert!(!lead.join("b.txt").exists());
}

#[test]
fn read_tree_of_a_tree_the_repo_does_not_hold_is_exit_128_failed_to_unpack() {
    let (w, _repo, lead, c1, _c2) = moved_lead();
    let from = tree(&w, &lead, &c1);
    let missing = "2222222222222222222222222222222222222222";
    let out = w.git(&lead, &["read-tree", "-m", "-u", &from, missing]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr),
        (
            128,
            "",
            format!("fatal: failed to unpack tree object {missing}\n")
        )
    );
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "one\n"
    );
}

#[test]
fn read_tree_while_index_lock_exists_is_exit_128_unable_to_create_the_lock() {
    let (w, _repo, lead, c1, c2) = moved_lead();
    let lock = w.lock_of(&lead);
    std::fs::write(&lock, "").unwrap();
    let (from, to) = (tree(&w, &lead, &c1), tree(&w, &lead, &c2));
    let out = w.git(&lead, &["read-tree", "-m", "-u", &from, &to]);
    assert_eq!(out.code, 128);
    assert_eq!(
        out.stderr.lines().next(),
        Some(format!("fatal: Unable to create '{}': File exists.", lock.display()).as_str())
    );
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "one\n"
    );
}

#[test]
fn read_tree_with_a_flag_it_does_not_model_is_unsupported_exit_1_and_recorded() {
    let (w, _repo, lead, _c1, _c2) = moved_lead();
    let out = w.git(&lead, &["read-tree", "--reset", "HEAD"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "fake-git: unsupported: read-tree --reset\n")
    );
    assert_eq!(w.route.unsupported(), vec!["read-tree --reset".to_owned()]);
}

// ---- the index file -------------------------------------------------------------------------------------

#[test]
fn the_index_is_rewritten_by_rename_so_a_read_tree_gives_it_a_new_inode() {
    let (w, _repo, lead, c1, c2) = moved_lead();
    let index = w.index_of(&lead);
    let before = std::fs::metadata(&index).unwrap();
    let (from, to) = (tree(&w, &lead, &c1), tree(&w, &lead, &c2));
    w.ok(&lead, &["read-tree", "-m", "-u", &from, &to]);
    let after = std::fs::metadata(&index).unwrap();
    assert_ne!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert!(
        !index.with_file_name("index.new").exists(),
        "no temporary file is left"
    );
    assert!(!w.lock_of(&lead).exists(), "no lock is left");
}

#[test]
fn a_worktree_add_leaves_an_index_file_in_the_admin_dir_that_rev_parse_git_path_names() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.add(&repo, "lead", "lead");
    let named = w.ok(
        &wt,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    assert_eq!(named.trim(), p(&w.index_of(&wt)));
    assert!(w.index_of(&wt).is_file());
}

// ---- rule S against the fake: git/lead.rs::sync_lead_with ------------------------------------------------

fn sync(w: &World, lead: &Path) -> Result<Synced, crate::error::CpdError> {
    sync_lead_with(&w.runner, &w.clock, lead, "lead")
}

#[test]
fn rule_s_brings_the_lead_worktree_up_to_the_moved_lead_branch() {
    let (w, _repo, lead, _c1, c2) = moved_lead();
    assert_eq!(sync(&w, &lead).unwrap(), Synced::Done);
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "two\n"
    );
    assert_eq!(std::fs::read_to_string(lead.join("b.txt")).unwrap(), "b\n");
    assert_eq!(w.ok(&lead, &["status", "--porcelain"]), "");
    assert_eq!(
        w.ok(&lead, &["write-tree"]),
        format!("{}\n", tree(&w, &lead, &c2))
    );
    assert!(w.clock.naps().is_empty(), "no nap");
}

#[test]
fn rule_s_on_a_lead_worktree_already_at_its_branch_is_done_without_a_read_tree() {
    let mut w = world();
    let repo = w.repo();
    let lead = w.add(&repo, "lead", "lead");
    assert_eq!(sync(&w, &lead).unwrap(), Synced::Done);
    assert!(
        !w.subcommands().contains(&"read-tree".to_owned()),
        "{:?}",
        w.subcommands()
    );
}

#[test]
fn rule_s_runs_read_tree_with_the_index_tree_and_the_head() {
    let (w, _repo, lead, c1, c2) = moved_lead();
    sync(&w, &lead).unwrap();
    let call = w
        .runner
        .calls()
        .into_iter()
        .find(|c| c.args.iter().any(|a| a == "read-tree"))
        .expect("a read-tree call");
    let tail: Vec<&str> = call
        .args
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        [
            "read-tree",
            "-m",
            "-u",
            tree(&w, &lead, &c1).as_str(),
            c2.as_str()
        ]
    );
}

#[test]
fn rule_s_stops_on_local_changes_and_overwrites_nothing() {
    let (w, _repo, lead, _c1, _c2) = moved_lead();
    std::fs::write(lead.join("README.md"), "mine\n").unwrap();
    let err = sync(&w, &lead).unwrap_err();
    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(
        err.details().get("worktree").and_then(|v| v.as_str()),
        Some(p(&lead))
    );
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "mine\n"
    );
    assert!(!lead.join("b.txt").exists());
    assert!(!w.subcommands().contains(&"read-tree".to_owned()));
}

#[test]
fn rule_s_stops_when_the_index_holds_a_tree_the_lead_branch_never_had() {
    let (w, repo, lead, _c1, _c2) = moved_lead();
    // staged and on disk alike (so the working tree equals the index), but no commit of the branch has it
    std::fs::write(lead.join("README.md"), "staged\n").unwrap();
    w.u(|u| {
        let blob = u.blob(repo.id(), b"staged\n");
        u.stage(&lead, "README.md", &blob, 0);
    });
    let err = sync(&w, &lead).unwrap_err();
    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "staged\n"
    );
    assert!(!w.subcommands().contains(&"read-tree".to_owned()));
}

#[test]
fn rule_s_with_index_lock_held_ends_busy_after_twenty_naps_of_100_to_500_ms() {
    let (w, _repo, lead, _c1, _c2) = moved_lead();
    std::fs::write(w.lock_of(&lead), "").unwrap();
    assert_eq!(sync(&w, &lead).unwrap(), Synced::Busy);
    let naps = w.clock.naps();
    assert_eq!(naps.len(), 20);
    assert!(
        naps.iter()
            .all(|n| (Duration::from_millis(100)..=Duration::from_millis(500)).contains(n)),
        "{naps:?}"
    );
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "one\n"
    );
    assert!(!w.subcommands().contains(&"read-tree".to_owned()));
}

#[test]
fn rule_s_retries_a_round_whose_git_call_met_index_lock_and_then_finishes() {
    let (mut w, _repo, lead, _c1, c2) = moved_lead();
    let held = format!(
        "fatal: Unable to create '{}': File exists.\n",
        w.lock_of(&lead).display()
    );
    w.runner = w.runner.clone().once(
        "git",
        |c| c.args.iter().any(|a| a == "write-tree"),
        Reply::exit(128, "", &held),
    );
    assert_eq!(sync(&w, &lead).unwrap(), Synced::Done);
    assert_eq!(w.clock.naps().len(), 1);
    assert_eq!(
        std::fs::read_to_string(lead.join("README.md")).unwrap(),
        "two\n"
    );
    assert_eq!(
        w.ok(&lead, &["write-tree"]),
        format!("{}\n", tree(&w, &lead, &c2))
    );
}

#[test]
fn rule_s_stops_at_a_git_failure_that_is_not_a_held_index_lock() {
    let (mut w, _repo, lead, _c1, _c2) = moved_lead();
    w.runner = w.runner.clone().once(
        "git",
        |c| c.args.iter().any(|a| a == "write-tree"),
        Reply::exit(128, "", "fatal: unable to read the index\n"),
    );
    let err = sync(&w, &lead).unwrap_err();
    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(
        err.details().get("stderr").and_then(|v| v.as_str()),
        Some("fatal: unable to read the index")
    );
    assert!(w.clock.naps().is_empty());
}
