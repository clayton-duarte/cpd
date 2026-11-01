//! `ls-remote`, `fetch`, `push` and the per-repo object sets of `GitRoute`, through a `FakeRunner`
//! (`FakeRunner::with_git`), and the product's `git/worktree.rs::push_commit_with` (rule P: `Pushed`,
//! `Contained`, `Rejected`) and `pr/rebase.rs::remote_contains_with` against the fake.
//!
//! The expected texts are literals recorded once by hand from real git in a scratch directory, against a
//! local bare repo (plain `push`, `push --porcelain`, `--force-with-lease`, `--delete`, `fetch`, `ls-remote`).
//! The git at hand was 2.54.0 (Apple Git-157), not 2.43; none of these texts changed between the two as far
//! as I know (unverified on 2.43). Two texts are the route's own, from the Spec and not from real git: the
//! unresolvable url's `fatal: unable to access '<url>': Could not resolve host` (real git also prints the
//! host and a trailing `/`; it waits on DNS), and the `To <url>` of a url that `insteadOf` rewrote.
//!
//! The fixtures leave `remote.origin.fetch` unset (`Repo::bare_origin` does not add it), where a real
//! `git remote add` adds `+refs/heads/*:refs/remotes/origin/*`: the route reads an unset refspec as that
//! default, so a fetch or a push updates `refs/remotes/origin/<branch>`.
//!
//! No test runs git (or any process), reads the wall clock or sleeps: every call is a `Command` handed to
//! the `FakeRunner`, which never starts it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{GitRoute, Oid, Repo, Universe};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::git::{Push, push_commit_with};
use crate::host::deadline::Failed;
use crate::host::runner::{Call, FakeRunner, Reply, Runner};
use crate::pr::remote_contains_with;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

struct World {
    tmp: TempDir,
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
    let runner = FakeRunner::new(clock).with_git(&route);
    World {
        tmp: tempfile::tempdir().unwrap(),
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

/// The first seven hex digits of a sha: how git abbreviates one in a summary of a small repo.
fn short(sha: impl std::fmt::Display) -> String {
    sha.to_string()[..7].to_owned()
}

/// The text `git push` prints before the line of a ref.
fn to(url: &str) -> String {
    format!("To {url}\n")
}

/// A line of `git push --porcelain`: `<flag>\t<from>:<to>\t<summary>`.
fn porcelain(flag: &str, from: &str, to: &str, summary: &str) -> String {
    format!("{flag}\t{from}:{to}\t{summary}\n")
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// A second writer's view of the world: where the remote is and what it holds that `a` has not got.
struct Diverged {
    a: Repo,
    origin: Repo,
    url: String,
    /// The commit both started from, pushed by `a`.
    c1: Oid,
    /// `a`'s own commit over `c1`, not pushed.
    a2: Oid,
    /// The other writer's commit over `c1`, which is the remote's tip and which `a` does not hold.
    b2: Oid,
}

impl World {
    /// The temp root, canonical (what real git prints for a path under it).
    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    fn u<R>(&self, f: impl FnOnce(&mut Universe) -> R) -> R {
        f(&mut self.universe.lock().unwrap())
    }
    /// A repo on `main` with one commit, `README.md` = `one\n`.
    fn repo(&mut self) -> Repo {
        self.repos += 1;
        let repo = Repo::init(
            &self.universe,
            &self.root().join(format!("repo{}", self.repos)),
        );
        repo.write("README.md", "one\n");
        repo.commit("one");
        repo
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
    fn rev(&self, dir: &Path, rev: &str) -> Oid {
        Oid::new(self.ok(dir, &["rev-parse", "--verify", rev]).trim())
    }
    /// Whether `git rev-parse --verify --quiet <rev>` finds it in `dir`.
    fn has_rev(&self, dir: &Path, rev: &str) -> bool {
        self.git(dir, &["rev-parse", "--verify", "--quiet", rev])
            .code
            == 0
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

    /// A repo on `main` (one commit) whose `origin` is the bare repo `origin.git`: nothing pushed yet.
    /// Returns the repo, the origin and the origin's url as configured.
    fn linked(&mut self) -> (Repo, Repo, String) {
        let repo = self.repo();
        let origin = repo.bare_origin(&self.root().join("origin.git"));
        let url = p(origin.dir()).to_owned();
        (repo, origin, url)
    }
    /// A commit over the tip of `repo`'s branch: `README.md` = `text` (also its message).
    fn next(&self, repo: &Repo, text: &str) -> Oid {
        repo.write("README.md", text);
        repo.commit(text)
    }
    /// A commit of `README.md` = `text` over `parent`, moving no ref.
    fn loose(&self, repo: &Repo, parent: &Oid, text: &str) -> Oid {
        self.u(|u| {
            let blob = u.blob(repo.id(), text.as_bytes());
            let tree = u.tree(repo.id(), &[("README.md", &blob)]);
            u.commit(repo.id(), &tree, std::slice::from_ref(parent), text)
        })
    }
    fn tip(&self, repo: &Repo, name: &str) -> Option<Oid> {
        self.u(|u| u.ref_oid(repo.id(), name))
    }
    fn holds(&self, repo: &Repo, oid: &Oid) -> bool {
        self.u(|u| u.has(repo.id(), oid))
    }
    /// Moves `refs/heads/<branch>` of `repo` to `oid` without a call (the commits are already there).
    fn point(&self, repo: &Repo, branch: &str, oid: &Oid) {
        self.u(|u| u.set_ref(repo.id(), &format!("refs/heads/{branch}"), oid, "test"));
    }
    /// A second writer of `origin`: a repo that holds `at` (a commit of `source`), has `main` there and
    /// the same `origin`.
    fn writer(&mut self, origin: &Repo, source: &Repo, at: &Oid) -> Repo {
        self.repos += 1;
        let writer = Repo::init(
            &self.universe,
            &self.root().join(format!("repo{}", self.repos)),
        );
        writer.config_add("remote.origin.url", p(origin.dir()));
        writer.write("README.md", "one\n");
        self.u(|u| {
            u.copy(source.id(), writer.id(), at);
            u.set_ref(writer.id(), "refs/heads/main", at, "clone");
        });
        writer
    }
    /// The remote at `b2`, which `a` does not hold; `a` has `a2` over `c1`, not pushed.
    fn diverged(&mut self) -> Diverged {
        let (a, origin, url) = self.linked();
        a.push();
        let c1 = self.rev(a.dir(), "main");
        let b = self.writer(&origin, &a, &c1);
        let b2 = self.next(&b, "by b\n");
        b.push();
        let a2 = self.next(&a, "by a\n");
        Diverged {
            a,
            origin,
            url,
            c1,
            a2,
            b2,
        }
    }
    /// `a` fetched what `origin` holds of `oid`, without a call.
    fn learn(&self, origin: &Repo, a: &Repo, oid: &Oid) {
        self.u(|u| u.copy(origin.id(), a.id(), oid));
    }
}

// ---- unsupported flags ------------------------------------------------------------------------------------

#[test]
fn an_unlisted_flag_of_fetch_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    let out = w.git(repo.dir(), &["fetch", "--bogus", "origin", "main"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "fake-git: unsupported: fetch --bogus\n")
    );
    assert_eq!(w.route.unsupported(), vec!["fetch --bogus".to_owned()]);
}

#[test]
fn an_unlisted_flag_of_push_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    let out = w.git(repo.dir(), &["push", "--bogus", "origin", "main"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "fake-git: unsupported: push --bogus\n")
    );
    assert_eq!(w.route.unsupported(), vec!["push --bogus".to_owned()]);
}

// ---- a url that resolves to no repo -----------------------------------------------------------------------

const GITHUB: &str = "https://github.com/acme/x.git";

#[test]
fn fetch_of_an_unresolvable_url_is_exit_128_could_not_resolve_host() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let out = w.git(repo.dir(), &["fetch", "origin", "main"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (
            128,
            "",
            format!("fatal: unable to access '{GITHUB}': Could not resolve host\n").as_str()
        )
    );
}

#[test]
fn push_of_an_unresolvable_url_is_exit_128_could_not_resolve_host() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let out = w.git(repo.dir(), &["push", "origin", "main"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (
            128,
            "",
            format!("fatal: unable to access '{GITHUB}': Could not resolve host\n").as_str()
        )
    );
}

#[test]
fn a_porcelain_push_of_an_unresolvable_url_prints_no_ref_line() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let sha = w.rev(repo.dir(), "main");
    let out = w.git(
        repo.dir(),
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("{sha}:refs/heads/main"),
        ],
    );
    assert_eq!((out.code, out.stdout.as_str()), (128, ""));
    assert_eq!(
        out.stderr,
        format!("fatal: unable to access '{GITHUB}': Could not resolve host\n")
    );
}

#[test]
fn an_unresolvable_fetch_leaves_the_remote_tracking_ref_where_it_was() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let before = w.rev(repo.dir(), "origin/main");
    w.next(&repo, "two\n");
    w.git(repo.dir(), &["fetch", "origin", "main"]);
    assert_eq!(w.rev(repo.dir(), "origin/main"), before);
}

#[test]
fn an_unresolvable_push_leaves_the_remote_tracking_ref_where_it_was() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let before = w.rev(repo.dir(), "origin/main");
    w.next(&repo, "two\n");
    w.git(repo.dir(), &["push", "origin", "main"]);
    assert_eq!(w.rev(repo.dir(), "origin/main"), before);
}

#[test]
fn a_url_that_insteadof_rewrites_to_a_repo_is_resolved_for_fetch_and_push() {
    let mut w = world();
    let repo = w.repo();
    let dir = w.root().join("origin.git");
    let origin = w.u(|u| u.init(&dir, true));
    repo.keyed("x");
    repo.config_add(&format!("url.{}.insteadOf", dir.display()), GITHUB);
    let sha = w.rev(repo.dir(), "main");
    assert_eq!(w.git(repo.dir(), &["push", "origin", "main"]).code, 0);
    assert_eq!(
        w.u(|u| u.ref_oid(origin, "refs/heads/main")),
        Some(sha.clone())
    );
    assert_eq!(w.git(repo.dir(), &["fetch", "origin", "main"]).code, 0);
    assert_eq!(w.rev(repo.dir(), "origin/main"), sha);
}

#[test]
fn a_repo_with_no_origin_cannot_fetch_or_push() {
    let mut w = world();
    let repo = w.repo();
    for args in [["fetch", "origin", "main"], ["push", "origin", "main"]] {
        let out = w.git(repo.dir(), &args);
        assert_eq!(out.code, 128, "{args:?}");
        assert_eq!(
            lines(&out.stderr)[..2],
            [
                "fatal: 'origin' does not appear to be a git repository",
                "fatal: Could not read from remote repository."
            ],
            "{args:?}"
        );
    }
}

// ---- ls-remote --------------------------------------------------------------------------------------------

#[test]
fn ls_remote_lists_what_a_push_put_on_the_remote() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let main = w.tip(&origin, "refs/heads/main").unwrap();
    assert_eq!(
        w.ok(repo.dir(), &["ls-remote", "--heads", "origin"]),
        format!("{main}\trefs/heads/main\n")
    );
}

#[test]
fn ls_remote_without_heads_lists_head_first_then_every_branch_in_name_order() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    w.ok(repo.dir(), &["push", "origin", "b"]);
    let main = w.tip(&origin, "refs/heads/main").unwrap();
    let b = w.tip(&origin, "refs/heads/b").unwrap();
    assert_eq!(
        w.ok(repo.dir(), &["ls-remote", "origin"]),
        format!("{main}\tHEAD\n{b}\trefs/heads/b\n{main}\trefs/heads/main\n")
    );
}

#[test]
fn ls_remote_exit_code_of_a_branch_that_was_never_pushed_is_exit_2() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let out = w.git(
        repo.dir(),
        &["ls-remote", "--exit-code", "--heads", "origin", "b"],
    );
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (2, "", "")
    );
}

#[test]
fn ls_remote_prints_the_tip_the_last_push_left() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let two = w.next(&repo, "two\n");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    assert_eq!(
        w.ok(repo.dir(), &["ls-remote", "origin", "refs/heads/main"]),
        format!("{two}\trefs/heads/main\n")
    );
}

// ---- fetch ------------------------------------------------------------------------------------------------

#[test]
fn fetch_prints_nothing_on_stdout_and_exits_0() {
    let mut w = world();
    let d = w.diverged();
    let out = w.git(d.a.dir(), &["fetch", "origin", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
}

#[test]
fn fetch_copies_the_remote_tips_whole_closure_into_the_repo() {
    let mut w = world();
    let d = w.diverged();
    let tree = w.u(|u| u.commit_data(&d.b2).unwrap().tree);
    let blobs: Vec<Oid> = w.u(|u| {
        u.tree_entries(&tree)
            .unwrap()
            .into_iter()
            .map(|(_, o)| o)
            .collect()
    });
    assert!(!w.holds(&d.a, &d.b2));
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    assert!(w.holds(&d.a, &d.b2));
    assert!(w.holds(&d.a, &tree));
    assert!(blobs.iter().all(|blob| w.holds(&d.a, blob)));
}

#[test]
fn fetch_makes_the_fetched_commit_known_to_cat_file_which_was_exit_128_before() {
    let mut w = world();
    let d = w.diverged();
    let spec = format!("{}^{{commit}}", d.b2);
    // recorded: `git cat-file -e <unknown sha>^{commit}` is exit 128 `fatal: Not a valid object name ...`
    assert_eq!(w.git(d.a.dir(), &["cat-file", "-e", &spec]).code, 128);
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    assert_eq!(w.git(d.a.dir(), &["cat-file", "-e", &spec]).code, 0);
}

#[test]
fn fetch_updates_the_remote_tracking_ref_to_the_remote_tip() {
    let mut w = world();
    let d = w.diverged();
    assert_eq!(w.rev(d.a.dir(), "origin/main"), d.c1);
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    assert_eq!(w.rev(d.a.dir(), "refs/remotes/origin/main"), d.b2);
}

#[test]
fn fetch_of_a_branch_the_repo_never_had_makes_its_remote_tracking_ref() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "b"]);
    w.u(|u| u.delete_ref(repo.id(), "refs/remotes/origin/b"));
    assert!(!w.has_rev(repo.dir(), "origin/b"));
    w.ok(repo.dir(), &["fetch", "origin", "b"]);
    assert_eq!(w.rev(repo.dir(), "origin/b"), w.rev(repo.dir(), "b"));
}

#[test]
fn fetch_leaves_the_local_branches_and_head_alone() {
    let mut w = world();
    let d = w.diverged();
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    assert_eq!(w.rev(d.a.dir(), "refs/heads/main"), d.a2);
    assert_eq!(w.rev(d.a.dir(), "HEAD"), d.a2);
    assert_eq!(
        w.ok(d.a.dir(), &["symbolic-ref", "--short", "-q", "HEAD"]),
        "main\n"
    );
}

#[test]
fn fetch_of_one_branch_does_not_copy_the_commits_only_another_branch_has() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    repo.push();
    let c1 = w.rev(repo.dir(), "main");
    let other = w.writer(&origin, &repo, &c1);
    let side = w.next(&other, "side\n");
    other.branch("side");
    w.ok(other.dir(), &["push", "origin", "side"]);
    assert_eq!(w.tip(&origin, "refs/heads/side"), Some(side.clone()));
    w.ok(repo.dir(), &["fetch", "origin", "main"]);
    assert!(!w.holds(&repo, &side));
    assert!(!w.has_rev(repo.dir(), "origin/side"));
}

#[test]
fn fetch_twice_gives_the_same_result() {
    let mut w = world();
    let d = w.diverged();
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    let out = w.git(d.a.dir(), &["fetch", "origin", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert_eq!(w.rev(d.a.dir(), "origin/main"), d.b2);
}

#[test]
fn fetch_of_a_branch_the_remote_does_not_have_is_exit_128_couldnt_find_remote_ref() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.push();
    let out = w.git(repo.dir(), &["fetch", "origin", "nope"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", "fatal: couldn't find remote ref nope\n")
    );
}

#[test]
fn after_a_fetch_origin_main_contains_the_common_ancestor_and_not_the_unpushed_commit() {
    let mut w = world();
    let d = w.diverged();
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    let out = w.git(
        d.a.dir(),
        &["merge-base", "--is-ancestor", d.c1.as_str(), "origin/main"],
    );
    assert_eq!(out.code, 0);
    assert_eq!(
        w.git(
            d.a.dir(),
            &["merge-base", "--is-ancestor", d.a2.as_str(), "origin/main"]
        )
        .code,
        1
    );
}

// ---- per-repo object sets ---------------------------------------------------------------------------------

#[test]
fn a_commit_made_here_is_not_in_the_remotes_object_set_until_it_is_pushed() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    assert!(w.holds(&repo, &one));
    assert!(!w.holds(&origin, &one));
    w.ok(repo.dir(), &["push", "origin", "main"]);
    assert!(w.holds(&origin, &one));
}

#[test]
fn a_push_copies_the_commit_its_tree_its_blobs_and_its_ancestors() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let c1 = w.rev(repo.dir(), "main");
    repo.write("dir/b.txt", "b\n");
    let c2 = w.next(&repo, "two\n");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let tree = w.u(|u| u.commit_data(&c2).unwrap().tree);
    let blobs: Vec<Oid> = w.u(|u| {
        u.tree_entries(&tree)
            .unwrap()
            .into_iter()
            .map(|(_, o)| o)
            .collect()
    });
    assert!(w.holds(&origin, &c2) && w.holds(&origin, &c1) && w.holds(&origin, &tree));
    assert!(blobs.len() >= 2 && blobs.iter().all(|blob| w.holds(&origin, blob)));
}

#[test]
fn a_push_of_one_branch_does_not_copy_the_commits_only_another_branch_has() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    repo.branch("side");
    let main2 = w.next(&repo, "main two\n");
    let side = w.loose(&repo, &w.rev(repo.dir(), "side"), "side two\n");
    w.point(&repo, "side", &side);
    w.ok(repo.dir(), &["push", "origin", "main"]);
    assert!(w.holds(&origin, &main2));
    assert!(!w.holds(&origin, &side));
}

#[test]
fn two_repos_that_push_to_one_remote_hold_each_others_commits_only_after_a_fetch() {
    let mut w = world();
    let d = w.diverged();
    assert!(w.holds(&d.origin, &d.b2) && w.holds(&d.origin, &d.c1));
    assert!(!w.holds(&d.a, &d.b2));
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    assert!(w.holds(&d.a, &d.b2));
}

#[test]
fn a_rejected_push_copies_nothing_to_the_remote() {
    let mut w = world();
    let d = w.diverged();
    let out = w.git(d.a.dir(), &["push", "origin", "main"]);
    assert_eq!(out.code, 1);
    assert!(!w.holds(&d.origin, &d.a2));
}

// ---- push, plain ------------------------------------------------------------------------------------------

#[test]
fn push_of_a_new_branch_prints_to_the_url_and_the_new_branch_line_on_stderr() {
    let mut w = world();
    let (repo, _origin, url) = w.linked();
    let out = w.git(repo.dir(), &["push", "origin", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert_eq!(
        out.stderr,
        format!("{} * [new branch]      main -> main\n", to(&url))
    );
}

#[test]
fn push_of_a_new_branch_makes_the_branch_on_the_remote_and_the_remote_tracking_ref_here() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let main = w.rev(repo.dir(), "main");
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(main.clone()));
    assert_eq!(w.rev(repo.dir(), "refs/remotes/origin/main"), main);
}

#[test]
fn push_of_what_the_remote_has_says_everything_up_to_date() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let out = w.git(repo.dir(), &["push", "origin", "main"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "Everything up-to-date\n")
    );
}

#[test]
fn push_of_a_fast_forward_prints_the_old_and_new_abbreviations() {
    let mut w = world();
    let (repo, origin, url) = w.linked();
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let one = w.rev(repo.dir(), "main");
    let two = w.next(&repo, "two\n");
    let out = w.git(repo.dir(), &["push", "origin", "main"]);
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stderr,
        format!(
            "{}   {}..{}  main -> main\n",
            to(&url),
            short(&one),
            short(&two)
        )
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn push_rejected_as_fetch_first_when_the_remote_tip_is_not_held_here() {
    let mut w = world();
    let d = w.diverged();
    let out = w.git(d.a.dir(), &["push", "origin", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert_eq!(
        lines(&out.stderr)[..3],
        [
            format!("To {}", d.url).as_str(),
            " ! [rejected]        main -> main (fetch first)",
            format!("error: failed to push some refs to '{}'", d.url).as_str()
        ]
    );
}

#[test]
fn push_rejected_as_non_fast_forward_when_the_remote_tip_is_held_here_and_diverged() {
    let mut w = world();
    let d = w.diverged();
    w.learn(&d.origin, &d.a, &d.b2);
    let out = w.git(d.a.dir(), &["push", "origin", "main"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert_eq!(
        lines(&out.stderr)[..3],
        [
            format!("To {}", d.url).as_str(),
            " ! [rejected]        main -> main (non-fast-forward)",
            format!("error: failed to push some refs to '{}'", d.url).as_str()
        ]
    );
}

#[test]
fn a_rejected_push_leaves_the_remote_branch_and_the_remote_tracking_ref_where_they_were() {
    let mut w = world();
    let d = w.diverged();
    w.git(d.a.dir(), &["push", "origin", "main"]);
    assert_eq!(w.tip(&d.origin, "refs/heads/main"), Some(d.b2.clone()));
    assert_eq!(w.rev(d.a.dir(), "origin/main"), d.c1);
}

#[test]
fn push_of_a_branch_that_does_not_exist_is_exit_1_src_refspec_does_not_match_any() {
    let mut w = world();
    let (repo, _origin, url) = w.linked();
    let out = w.git(repo.dir(), &["push", "origin", "nope"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert_eq!(
        out.stderr,
        format!(
            "error: src refspec nope does not match any\nerror: failed to push some refs to '{url}'\n"
        )
    );
}

// ---- push --porcelain -------------------------------------------------------------------------------------

fn porcelain_push(w: &World, repo: &Repo, sha: &Oid, extra: &[&str]) -> Out {
    let spec = format!("{sha}:refs/heads/main");
    let mut args = vec!["push", "--porcelain"];
    args.extend_from_slice(extra);
    args.extend(["origin", spec.as_str()]);
    w.git(repo.dir(), &args)
}

#[test]
fn porcelain_push_of_a_new_branch_is_a_star_and_new_branch_then_done() {
    let mut w = world();
    let (repo, origin, url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    let out = porcelain_push(&w, &repo, &one, &[]);
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stdout,
        format!(
            "{}{}Done\n",
            to(&url),
            porcelain("*", one.as_str(), "refs/heads/main", "[new branch]")
        )
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(one));
}

#[test]
fn porcelain_push_of_what_the_remote_has_is_an_equals_and_up_to_date() {
    let mut w = world();
    let (repo, _origin, url) = w.linked();
    repo.push();
    let one = w.rev(repo.dir(), "main");
    let out = porcelain_push(&w, &repo, &one, &[]);
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stdout,
        format!(
            "{}{}Done\n",
            to(&url),
            porcelain("=", one.as_str(), "refs/heads/main", "[up to date]")
        )
    );
}

#[test]
fn porcelain_push_of_a_fast_forward_is_a_space_and_the_old_and_new_abbreviations() {
    let mut w = world();
    let (repo, origin, url) = w.linked();
    repo.push();
    let one = w.rev(repo.dir(), "main");
    let two = w.next(&repo, "two\n");
    let out = porcelain_push(&w, &repo, &two, &[]);
    assert_eq!(out.code, 0);
    let summary = format!("{}..{}", short(&one), short(&two));
    assert_eq!(
        out.stdout,
        format!(
            "{}{}Done\n",
            to(&url),
            porcelain(" ", two.as_str(), "refs/heads/main", &summary)
        )
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn porcelain_push_publishes_the_sha_it_names_not_the_tip_of_the_branch() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    let out = porcelain_push(&w, &repo, &one, &[]);
    assert_eq!(out.code, 0);
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(one.clone()));
    assert_eq!(w.rev(repo.dir(), "refs/remotes/origin/main"), one);
}

#[test]
fn porcelain_push_rejected_as_fetch_first_is_a_bang_with_the_reason_and_exit_1() {
    let mut w = world();
    let d = w.diverged();
    let out = porcelain_push(&w, &d.a, &d.a2, &[]);
    assert_eq!(out.code, 1);
    assert!(out.stdout.starts_with(&to(&d.url)), "{}", out.stdout);
    let line = porcelain(
        "!",
        d.a2.as_str(),
        "refs/heads/main",
        "[rejected] (fetch first)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&d.origin, "refs/heads/main"), Some(d.b2));
}

#[test]
fn porcelain_push_rejected_as_non_fast_forward_is_a_bang_with_the_reason_and_exit_1() {
    let mut w = world();
    let d = w.diverged();
    w.learn(&d.origin, &d.a, &d.b2);
    let out = porcelain_push(&w, &d.a, &d.a2, &[]);
    assert_eq!(out.code, 1);
    let line = porcelain(
        "!",
        d.a2.as_str(),
        "refs/heads/main",
        "[rejected] (non-fast-forward)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&d.origin, "refs/heads/main"), Some(d.b2));
}

#[test]
fn porcelain_push_of_an_older_commit_the_remote_has_gone_past_is_rejected_non_fast_forward() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    repo.push();
    let two = w.rev(repo.dir(), "main");
    let out = porcelain_push(&w, &repo, &one, &[]);
    assert_eq!(out.code, 1);
    let line = porcelain(
        "!",
        one.as_str(),
        "refs/heads/main",
        "[rejected] (non-fast-forward)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn a_porcelain_push_that_succeeds_updates_the_remote_tracking_ref() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    porcelain_push(&w, &repo, &one, &[]);
    assert_eq!(w.rev(repo.dir(), "refs/remotes/origin/main"), one);
}

// ---- push --force-with-lease -------------------------------------------------------------------------------

/// The remote at `two` (over `one`, both pushed); `alt` is another commit over `one`, held here only.
fn leased(w: &mut World) -> (Repo, Repo, String, Oid, Oid, Oid) {
    let (repo, origin, url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    repo.push();
    let two = w.rev(repo.dir(), "main");
    let alt = w.loose(&repo, &one, "alt\n");
    (repo, origin, url, one, two, alt)
}

#[test]
fn a_lease_that_matches_the_remote_tip_lets_a_rewrite_through_as_a_forced_update() {
    let mut w = world();
    let (repo, origin, url, _one, two, alt) = leased(&mut w);
    let lease = format!("--force-with-lease=refs/heads/main:{two}");
    let out = porcelain_push(&w, &repo, &alt, &[&lease]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let summary = format!("{}...{} (forced update)", short(&two), short(&alt));
    assert_eq!(
        out.stdout,
        format!(
            "{}{}Done\n",
            to(&url),
            porcelain("+", alt.as_str(), "refs/heads/main", &summary)
        )
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(alt));
}

#[test]
fn a_stale_lease_is_rejected_with_stale_info_and_exit_1() {
    let mut w = world();
    let (repo, origin, _url, one, two, alt) = leased(&mut w);
    let lease = format!("--force-with-lease=refs/heads/main:{one}");
    let out = porcelain_push(&w, &repo, &alt, &[&lease]);
    assert_eq!(out.code, 1);
    let line = porcelain(
        "!",
        alt.as_str(),
        "refs/heads/main",
        "[rejected] (stale info)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn a_stale_lease_rejects_even_a_fast_forward() {
    let mut w = world();
    let (repo, origin, _url, one, two, _alt) = leased(&mut w);
    let three = w.next(&repo, "three\n");
    let lease = format!("--force-with-lease=refs/heads/main:{one}");
    let out = porcelain_push(&w, &repo, &three, &[&lease]);
    assert_eq!(out.code, 1);
    let line = porcelain(
        "!",
        three.as_str(),
        "refs/heads/main",
        "[rejected] (stale info)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn a_push_of_what_the_remote_already_has_succeeds_whatever_the_lease() {
    let mut w = world();
    let (repo, _origin, url, one, two, _alt) = leased(&mut w);
    let lease = format!("--force-with-lease=refs/heads/main:{one}");
    let out = porcelain_push(&w, &repo, &two, &[&lease]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        format!(
            "{}{}Done\n",
            to(&url),
            porcelain("=", two.as_str(), "refs/heads/main", "[up to date]")
        )
    );
}

#[test]
fn an_empty_lease_lets_a_branch_the_remote_does_not_have_be_made() {
    let mut w = world();
    let (repo, origin, url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    let out = porcelain_push(&w, &repo, &one, &["--force-with-lease=refs/heads/main:"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        format!(
            "{}{}Done\n",
            to(&url),
            porcelain("*", one.as_str(), "refs/heads/main", "[new branch]")
        )
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(one));
}

#[test]
fn an_empty_lease_on_a_branch_the_remote_has_is_stale_info() {
    let mut w = world();
    let (repo, origin, _url, _one, two, alt) = leased(&mut w);
    let out = porcelain_push(&w, &repo, &alt, &["--force-with-lease=refs/heads/main:"]);
    assert_eq!(out.code, 1);
    let line = porcelain(
        "!",
        alt.as_str(),
        "refs/heads/main",
        "[rejected] (stale info)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn a_rewrite_without_a_lease_is_rejected_non_fast_forward() {
    let mut w = world();
    let (repo, origin, _url, _one, two, alt) = leased(&mut w);
    let out = porcelain_push(&w, &repo, &alt, &[]);
    assert_eq!(out.code, 1);
    let line = porcelain(
        "!",
        alt.as_str(),
        "refs/heads/main",
        "[rejected] (non-fast-forward)",
    );
    assert!(out.stdout.contains(&line), "{}", out.stdout);
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

// ---- push --delete ----------------------------------------------------------------------------------------

#[test]
fn push_delete_removes_the_branch_on_the_remote_and_prints_the_deleted_line() {
    let mut w = world();
    let (repo, origin, url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    w.ok(repo.dir(), &["push", "origin", "b"]);
    let out = w.git(repo.dir(), &["push", "origin", "--delete", "b"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert_eq!(out.stderr, format!("{} - [deleted]         b\n", to(&url)));
    assert_eq!(w.tip(&origin, "refs/heads/b"), None);
    assert!(w.tip(&origin, "refs/heads/main").is_some());
}

#[test]
fn push_delete_removes_the_remote_tracking_ref_here() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    w.ok(repo.dir(), &["push", "origin", "b"]);
    assert!(w.has_rev(repo.dir(), "origin/b"));
    w.ok(repo.dir(), &["push", "origin", "--delete", "b"]);
    assert!(!w.has_rev(repo.dir(), "origin/b"));
    assert!(w.has_rev(repo.dir(), "origin/main"));
}

#[test]
fn push_delete_of_a_branch_the_remote_does_not_have_is_exit_1_remote_ref_does_not_exist() {
    let mut w = world();
    let (repo, _origin, url) = w.linked();
    w.ok(repo.dir(), &["push", "origin", "main"]);
    let out = w.git(repo.dir(), &["push", "origin", "--delete", "b"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert_eq!(
        out.stderr,
        format!(
            "error: unable to delete 'b': remote ref does not exist\nerror: failed to push some refs to '{url}'\n"
        )
    );
}

#[test]
fn a_porcelain_delete_is_a_minus_with_an_empty_source_and_deleted() {
    let mut w = world();
    let (repo, origin, url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    w.ok(repo.dir(), &["push", "origin", "b"]);
    let out = w.git(
        repo.dir(),
        &["push", "--porcelain", "origin", "--delete", "b"],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        format!("{}-\t:refs/heads/b\t[deleted]\nDone\n", to(&url))
    );
    assert_eq!(w.tip(&origin, "refs/heads/b"), None);
}

#[test]
fn ls_remote_exit_code_of_a_branch_that_was_deleted_on_the_remote_is_exit_2() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.branch("b");
    w.ok(repo.dir(), &["push", "origin", "main"]);
    w.ok(repo.dir(), &["push", "origin", "b"]);
    assert_eq!(
        w.git(
            repo.dir(),
            &["ls-remote", "--exit-code", "--heads", "origin", "b"]
        )
        .code,
        0
    );
    w.ok(repo.dir(), &["push", "origin", "--delete", "b"]);
    assert_eq!(
        w.git(
            repo.dir(),
            &["ls-remote", "--exit-code", "--heads", "origin", "b"]
        )
        .code,
        2
    );
}

// ---- git/worktree.rs::push_commit_with (rule P) ------------------------------------------------------------

fn pushed(w: &World, repo: &Repo, sha: &Oid) -> Push {
    push_commit_with(&w.runner, repo.dir(), "main", sha.as_str()).unwrap()
}

#[test]
fn push_commit_of_a_new_branch_is_pushed_new_and_makes_one_git_call() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    let before = w.subcommands().len();
    assert_eq!(pushed(&w, &repo, &one), Push::Pushed { new: true });
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(one));
    assert_eq!(w.subcommands()[before..], ["push"]);
}

#[test]
fn push_commit_of_what_the_remote_has_is_pushed_not_new() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.push();
    let one = w.rev(repo.dir(), "main");
    assert_eq!(pushed(&w, &repo, &one), Push::Pushed { new: false });
}

#[test]
fn push_commit_of_a_fast_forward_is_pushed_new() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    repo.push();
    let two = w.next(&repo, "two\n");
    assert_eq!(pushed(&w, &repo, &two), Push::Pushed { new: true });
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn push_commit_publishes_the_sha_it_is_given_not_the_tip_of_the_branch() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    assert_eq!(pushed(&w, &repo, &one), Push::Pushed { new: true });
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(one));
}

#[test]
fn push_commit_of_an_older_commit_the_remote_contains_is_contained_and_pushes_nothing() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    repo.push();
    let two = w.rev(repo.dir(), "main");
    assert_eq!(
        pushed(&w, &repo, &one),
        Push::Contained {
            remote: two.to_string()
        }
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

/// What GitHub's ref-lock race between two concurrent pushes prints (contract §7.4 rule P): a `!` line with no `(non-fast-forward)` reason.
fn lock_race() -> Reply {
    let line = porcelain(
        "!",
        &"3".repeat(40),
        "refs/heads/main",
        "[remote rejected] (cannot lock ref 'refs/heads/main': is at 1111111111111111111111111111111111111111 but expected 2222222222222222222222222222222222222222)",
    );
    Reply::exit(
        1,
        &format!("To origin\n{line}Done\n"),
        "error: failed to push some refs to 'origin'\n",
    )
}

fn is_push(call: &Call) -> bool {
    call.args.iter().any(|a| a == "push")
}

fn is_ls_remote(call: &Call) -> bool {
    call.args.iter().any(|a| a == "ls-remote")
}

#[test]
fn push_commit_after_a_remote_rejected_lock_race_the_remote_already_contains_is_contained() {
    let mut w = world();
    let (repo, origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    repo.push();
    let two = w.rev(repo.dir(), "main");
    let _ = w.runner.clone().once("git", is_push, lock_race());
    assert_eq!(
        pushed(&w, &repo, &one),
        Push::Contained {
            remote: two.to_string()
        }
    );
    assert_eq!(w.tip(&origin, "refs/heads/main"), Some(two));
}

#[test]
fn push_commit_after_a_remote_rejected_lock_race_the_remote_does_not_contain_is_git_failed_naming_the_reason()
 {
    let mut w = world();
    let d = w.diverged();
    w.learn(&d.origin, &d.a, &d.b2);
    let _ = w.runner.clone().once("git", is_push, lock_race());
    let err = push_commit_with(&w.runner, d.a.dir(), "main", d.a2.as_str()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert!(
        err.message().contains("cannot lock ref"),
        "{}",
        err.message()
    );
    assert!(
        err.message().starts_with("git push exited 1: "),
        "{}",
        err.message()
    );
    assert!(
        !err.details()["stderr"].as_str().unwrap_or("").is_empty(),
        "{:?}",
        err.details()
    );
}

#[test]
fn push_commit_whose_lock_race_check_cannot_reach_the_remote_reports_the_push_error() {
    let mut w = world();
    let d = w.diverged();
    w.learn(&d.origin, &d.a, &d.b2);
    let _ = w.runner.clone().once("git", is_push, lock_race());
    let _ = w.runner.clone().once(
        "git",
        is_ls_remote,
        Reply::exit(
            128,
            "",
            "fatal: unable to access 'origin': Connection closed\n",
        ),
    );
    let err = push_commit_with(&w.runner, d.a.dir(), "main", d.a2.as_str()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert!(
        err.message().contains("cannot lock ref"),
        "{}",
        err.message()
    );
}

#[test]
fn push_commit_failing_without_a_rejection_line_names_the_first_stderr_line() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    let _ = w.runner.clone().once(
        "git",
        is_push,
        Reply::exit(
            128,
            "",
            "Connection closed by 140.82.112.4 port 22\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n",
        ),
    );
    let err = push_commit_with(&w.runner, repo.dir(), "main", one.as_str()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert_eq!(
        err.message(),
        "git push exited 128: Connection closed by 140.82.112.4 port 22."
    );
}

#[test]
fn push_commit_of_a_diverged_commit_whose_remote_tip_is_held_is_rejected_non_fast_forward() {
    let mut w = world();
    let d = w.diverged();
    w.learn(&d.origin, &d.a, &d.b2);
    let Push::Rejected { stderr } = pushed(&w, &d.a, &d.a2) else {
        panic!("expected a rejection")
    };
    assert!(stderr.contains("(non-fast-forward)"), "{stderr}");
    assert_eq!(w.tip(&d.origin, "refs/heads/main"), Some(d.b2));
}

#[test]
fn push_commit_whose_remote_tip_is_not_held_here_is_rejected_fetch_first_never_contained() {
    let mut w = world();
    let d = w.diverged();
    let Push::Rejected { stderr } = pushed(&w, &d.a, &d.a2) else {
        panic!("expected a rejection")
    };
    assert!(stderr.contains("(fetch first)"), "{stderr}");
}

#[test]
fn push_commit_after_a_fetch_turns_a_fetch_first_rejection_into_a_non_fast_forward_one() {
    let mut w = world();
    let d = w.diverged();
    w.ok(d.a.dir(), &["fetch", "origin", "main"]);
    let Push::Rejected { stderr } = pushed(&w, &d.a, &d.a2) else {
        panic!("expected a rejection")
    };
    assert!(stderr.contains("(non-fast-forward)"), "{stderr}");
}

#[test]
fn push_commit_with_no_origin_is_git_failed() {
    let mut w = world();
    let repo = w.repo();
    let one = w.rev(repo.dir(), "main");
    let err = push_commit_with(&w.runner, repo.dir(), "main", one.as_str()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
}

#[test]
fn push_commit_to_an_unresolvable_url_is_git_failed_with_the_stderr() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let one = w.rev(repo.dir(), "main");
    let err = push_commit_with(&w.runner, repo.dir(), "main", one.as_str()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    let stderr = err
        .details()
        .get("stderr")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(stderr.contains("Could not resolve host"), "{stderr}");
}

// ---- pr/rebase.rs::remote_contains_with ----------------------------------------------------------------------

const MAIN: &str = "refs/heads/main";

fn contains(w: &World, repo: &Repo, sha: &Oid) -> bool {
    remote_contains_with(&w.runner, repo.dir(), MAIN, sha.as_str()).unwrap()
}

#[test]
fn remote_contains_the_commit_that_is_its_tip() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.push();
    assert!(contains(&w, &repo, &w.rev(repo.dir(), "main")));
}

#[test]
fn remote_contains_an_ancestor_of_its_tip_that_is_held_here() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    w.next(&repo, "two\n");
    repo.push();
    assert!(contains(&w, &repo, &one));
}

#[test]
fn remote_does_not_contain_a_commit_ahead_of_its_tip() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.push();
    let two = w.next(&repo, "two\n");
    assert!(!contains(&w, &repo, &two));
}

#[test]
fn remote_does_not_contain_a_diverged_commit() {
    let mut w = world();
    let d = w.diverged();
    w.learn(&d.origin, &d.a, &d.b2);
    assert!(!contains(&w, &d.a, &d.a2));
}

#[test]
fn remote_does_not_contain_anything_when_its_tip_is_not_held_here() {
    let mut w = world();
    let d = w.diverged();
    assert!(!contains(&w, &d.a, &d.c1));
}

#[test]
fn remote_without_the_branch_contains_nothing() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    let one = w.rev(repo.dir(), "main");
    assert!(!contains(&w, &repo, &one));
}

#[test]
fn remote_contains_asks_ls_remote_then_cat_file_then_merge_base() {
    let mut w = world();
    let (repo, _origin, _url) = w.linked();
    repo.push();
    let one = w.rev(repo.dir(), "main");
    let before = w.subcommands().len();
    assert!(contains(&w, &repo, &one));
    assert_eq!(
        w.subcommands()[before..],
        ["ls-remote", "cat-file", "merge-base"]
    );
}

#[test]
fn remote_contains_of_an_unresolvable_url_is_git_failed_with_the_stderr() {
    let mut w = world();
    let repo = w.repo();
    repo.keyed("x");
    let one = w.rev(repo.dir(), "main");
    let err = remote_contains_with(&w.runner, repo.dir(), MAIN, one.as_str()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    let stderr = err
        .details()
        .get("stderr")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(stderr.contains("Could not resolve host"), "{stderr}");
}
