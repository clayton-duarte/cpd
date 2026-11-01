//! The read-only subcommands of `GitRoute`, through a `FakeRunner` (`FakeRunner::with_git`): global
//! options, repo resolution, strict parsing, and the exit-code fidelity points.
//!
//! The expected texts are literals recorded once by hand from real git in a scratch directory. The git at
//! hand was 2.54.0 (Apple Git-157), not 2.43; none of these texts changed between the two as far as I know
//! (unverified on 2.43), and `git --version` is the route's own `2.43.0`. No test runs git (or any
//! process), reads the wall clock or sleeps: every call is a `Command` handed to the `FakeRunner`, which
//! never starts it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{GitRoute, Head, Oid, Repo, Universe};
use crate::clock::ManualClock;
use crate::host::deadline::Failed;
use crate::host::runner::{FakeRunner, Reply, Runner};

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
    fn set_ref(&self, repo: &Repo, name: &str, at: &Oid) {
        self.u(|u| u.set_ref(repo.id(), name, at, "test"));
    }
    /// `git -C <dir> <args...>`, the form of the product.
    fn git(&self, dir: &Path, args: &[&str]) -> Out {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        run(&self.runner, &mut cmd)
    }
    /// `git <args...>` with `cwd` as the working directory and no `-C`.
    fn git_cwd(&self, cwd: &Path, args: &[&str]) -> Out {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(cwd);
        run(&self.runner, &mut cmd)
    }
    fn git_env(&self, dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Out {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args).envs(envs.iter().copied());
        run(&self.runner, &mut cmd)
    }
    /// A linked worktree `wt` of `repo` on the existing branch `branch`.
    fn linked(&self, repo: &Repo, name: &str, branch: &str) -> PathBuf {
        let path = self.root().join(name);
        self.u(|u| u.add_worktree(repo.id(), &path, Head::Branch(branch.to_owned())));
        path
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

const NOT_A_REPO: &str = "fatal: not a git repository (or any of the parent directories): .git\n";

// ---- global options and the call itself -------------------------------------------------

#[test]
fn git_version_without_a_subcommand_prints_the_minimum_version_the_doctor_checks() {
    let w = world();
    let mut cmd = Command::new("git");
    cmd.arg("--version");
    let out = run(&w.runner, &mut cmd);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "git version 2.43.0\n", "")
    );
}

#[test]
fn dash_c_names_the_directory_the_call_runs_in() {
    let mut w = world();
    let repo = w.repo();
    let head = w.git(repo.dir(), &["rev-parse", "--verify", "HEAD"]);
    assert_eq!(head.code, 0);
    assert_eq!(head.stdout.len(), 41);
}

#[test]
fn without_dash_c_the_calls_cwd_is_the_directory() {
    let mut w = world();
    let repo = w.repo();
    let by_c = w.git(repo.dir(), &["rev-parse", "--verify", "HEAD"]);
    let by_cwd = w.git_cwd(repo.dir(), &["rev-parse", "--verify", "HEAD"]);
    assert_eq!(by_cwd.code, 0);
    assert_eq!(by_cwd.stdout, by_c.stdout);
}

#[test]
fn a_repeated_dash_c_is_relative_to_the_previous_one() {
    let mut w = world();
    let repo = w.repo();
    let name = repo.dir().file_name().unwrap().to_str().unwrap().to_owned();
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(w.root())
        .arg("-C")
        .arg(&name)
        .args(["rev-parse", "--verify", "HEAD"]);
    let out = run(&w.runner, &mut cmd);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout.len(), 41);
}

#[test]
fn a_relative_dash_c_is_relative_to_the_calls_cwd() {
    let mut w = world();
    let repo = w.repo();
    let name = repo.dir().file_name().unwrap().to_str().unwrap().to_owned();
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(&name)
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(w.root());
    let out = run(&w.runner, &mut cmd);
    assert_eq!(out.code, 0, "{}", out.stderr);
}

#[test]
fn dash_c_of_a_directory_that_does_not_exist_is_exit_128() {
    let w = world();
    let missing = w.root().join("nodir");
    let out = w.git(&missing, &["rev-parse", "HEAD"]);
    // recorded: `git -C r/nodir rev-parse HEAD`
    assert_eq!((out.code, out.stdout.as_str()), (128, ""));
    assert_eq!(
        out.stderr,
        format!(
            "fatal: cannot change to '{}': No such file or directory\n",
            missing.display()
        )
    );
}

#[test]
fn a_directory_with_no_repo_above_it_is_exit_128_not_a_git_repository() {
    let mut w = world();
    let _ = w.repo();
    let out = w.git(&w.root(), &["rev-parse", "HEAD"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", NOT_A_REPO)
    );
}

#[test]
fn a_subdirectory_of_a_worktree_is_in_its_repo() {
    let mut w = world();
    let repo = w.repo();
    repo.write("src/deep/file.txt", "x\n");
    let out = w.git(
        &repo.dir().join("src/deep"),
        &["rev-parse", "--verify", "HEAD"],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
}

#[test]
fn the_global_options_of_the_product_are_accepted_and_record_nothing() {
    let mut w = world();
    let repo = w.repo();
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo.dir()).args([
        "-c",
        "core.editor=true",
        "-c",
        "sequence.editor=true",
        "--no-optional-locks",
        "status",
        "--porcelain",
    ]);
    let out = run(&w.runner, &mut cmd);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    assert!(w.route.unsupported().is_empty());
}

#[test]
fn dash_c_accepts_exactly_the_documented_keys() {
    let mut w = world();
    let repo = w.repo();
    for setting in [
        "core.editor=true",
        "sequence.editor=true",
        "core.quotePath=false",
        "core.filesRefLockTimeout=10000",
        "core.packedRefsTimeout=10000",
        "user.name=Ada",
        "user.email=ada@example.test",
    ] {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(repo.dir())
            .args(["-c", setting, "rev-parse", "--verify", "HEAD"]);
        let out = run(&w.runner, &mut cmd);
        assert_eq!(out.code, 0, "{setting}: {}", out.stderr);
    }
    assert!(w.route.unsupported().is_empty());
}

#[test]
fn dash_c_of_another_key_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo.dir())
        .args(["-c", "bogus.key=1", "rev-parse", "--verify", "HEAD"]);
    let out = run(&w.runner, &mut cmd);
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr.starts_with("fake-git: unsupported: "),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("bogus.key"), "{}", out.stderr);
    let recorded = w.route.unsupported();
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].contains("bogus.key"), "{recorded:?}");
}

#[test]
fn the_lock_timeout_environment_of_every_product_call_is_accepted() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git_env(
        repo.dir(),
        &["rev-parse", "--verify", "HEAD"],
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_CONFIG_COUNT", "2"),
            ("GIT_CONFIG_KEY_0", "core.filesRefLockTimeout"),
            ("GIT_CONFIG_VALUE_0", "10000"),
            ("GIT_CONFIG_KEY_1", "core.packedRefsTimeout"),
            ("GIT_CONFIG_VALUE_1", "10000"),
        ],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(w.route.unsupported().is_empty());
}

#[test]
fn git_config_count_adds_config_the_subcommands_read() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add("remote.origin.url", "https://github.com/acme/x.git");
    let envs = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "url./srv/o.git.insteadOf"),
        ("GIT_CONFIG_VALUE_0", "https://github.com/acme/x.git"),
    ];
    let out = w.git_env(repo.dir(), &["remote", "get-url", "origin"], &envs);
    assert_eq!((out.code, out.stdout.as_str()), (0, "/srv/o.git\n"));
}

#[test]
fn an_unknown_subcommand_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["nonsense"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert_eq!(out.stderr, "fake-git: unsupported: nonsense\n");
    assert_eq!(w.route.unsupported(), ["nonsense"]);
}

#[test]
fn an_unlisted_flag_is_unsupported_exit_1_and_recorded() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["status", "--bogus"]);
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert_eq!(out.stderr, "fake-git: unsupported: status --bogus\n");
    assert_eq!(w.route.unsupported(), ["status --bogus"]);
}

#[test]
fn an_unlisted_flag_of_each_read_subcommand_is_unsupported() {
    let mut w = world();
    let repo = w.repo();
    for (sub, flag) in [
        ("rev-parse", "--bogus"),
        ("symbolic-ref", "--bogus"),
        ("config", "--bogus"),
        ("remote", "--bogus"),
        ("cat-file", "--bogus"),
        ("merge-base", "--bogus"),
        ("rev-list", "--bogus"),
        ("ls-tree", "--bogus"),
        ("log", "--bogus"),
        ("diff", "--bogus"),
    ] {
        let out = w.git(repo.dir(), &[sub, flag]);
        assert_eq!(out.code, 1, "{sub}");
        assert_eq!(
            out.stderr,
            format!("fake-git: unsupported: {sub} {flag}\n"),
            "{sub}"
        );
    }
    assert_eq!(w.route.unsupported().len(), 10);
}

#[test]
fn a_supported_call_that_exits_1_is_not_recorded_as_unsupported() {
    let mut w = world();
    let repo = w.repo();
    let miss = w.git(repo.dir(), &["rev-parse", "--verify", "--quiet", "nope"]);
    assert_eq!(miss.code, 1);
    assert!(w.route.unsupported().is_empty());
}

#[test]
fn a_call_takes_no_virtual_time() {
    let mut w = world();
    let repo = w.repo();
    w.git(repo.dir(), &["status", "--porcelain"]);
    w.git(repo.dir(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(w.clock.elapsed(), Duration::ZERO);
    assert!(w.clock.naps().is_empty());
}

#[test]
fn a_scripted_reply_wins_over_the_route() {
    let mut w = world();
    let repo = w.repo();
    let runner = FakeRunner::new(w.clock.clone())
        .on(
            "git",
            |c| c.args.iter().any(|a| a == "status"),
            Reply::exit(128, "", "fatal: bad object worktrees/x"),
        )
        .with_git(&w.route);
    let mut scripted = Command::new("git");
    scripted.arg("-C").arg(repo.dir()).arg("status");
    let out = run(&runner, &mut scripted);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: bad object worktrees/x")
    );
    let mut routed = Command::new("git");
    routed
        .arg("-C")
        .arg(repo.dir())
        .args(["rev-parse", "--verify", "HEAD"]);
    assert_eq!(run(&runner, &mut routed).code, 0);
    assert!(runner.unscripted().is_empty());
}

#[test]
fn a_program_other_than_git_is_still_unscripted() {
    let w = world();
    let mut cmd = Command::new("tmux");
    cmd.arg("list-sessions");
    let answer = w.runner.output(&mut cmd, "tmux", Duration::from_secs(10));
    assert!(matches!(answer, Err(Failed::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound));
    assert_eq!(w.runner.unscripted().len(), 1);
}

// ---- rev-parse ----------------------------------------------------------------------------

#[test]
fn rev_parse_verify_end_of_options_head_prints_the_full_sha_and_a_newline() {
    let mut w = world();
    let repo = w.repo();
    let head = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let out = w.git(
        repo.dir(),
        &["rev-parse", "--verify", "--end-of-options", "HEAD"],
    );
    assert_eq!(
        (out.code, out.stdout, out.stderr.as_str()),
        (0, format!("{head}\n"), "")
    );
}

#[test]
fn rev_parse_verify_resolves_each_form_of_a_branch_the_product_uses() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let ahead = w.commit_of(&repo, Some(&main), &[("README.md", "two\n")], "two");
    w.set_ref(&repo, "refs/remotes/origin/main", &ahead);
    for (rev, want) in [
        ("refs/heads/main", &main),
        ("main", &main),
        ("HEAD", &main),
        ("refs/remotes/origin/main", &ahead),
        ("origin/main", &ahead),
    ] {
        let out = w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--end-of-options", rev],
        );
        assert_eq!((out.code, out.stdout), (0, format!("{want}\n")), "{rev}");
    }
}

#[test]
fn rev_parse_verify_quiet_of_a_missing_ref_is_exit_1_and_silent() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["rev-parse", "--verify", "--quiet", "nope"]);
    // recorded: `git rev-parse --verify --quiet nope`
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "")
    );
}

#[test]
fn rev_parse_verify_of_a_missing_ref_is_exit_128_needed_a_single_revision() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["rev-parse", "--verify", "nope"]);
    // recorded: `git rev-parse --verify nope`
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", "fatal: Needed a single revision\n")
    );
}

#[test]
fn rev_parse_verify_end_of_options_of_a_missing_ref_is_also_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(
        repo.dir(),
        &["rev-parse", "--verify", "--end-of-options", "nope"],
    );
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: Needed a single revision\n")
    );
}

#[test]
fn rev_parse_without_verify_of_a_missing_name_is_exit_128_ambiguous_argument() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["rev-parse", "nope"]);
    // recorded: `git rev-parse nope` (stdout echoes the argument; stderr has three more lines of advice)
    assert_eq!(out.code, 128);
    assert_eq!(out.stdout, "nope\n");
    assert_eq!(
        first_line(&out.stderr),
        "fatal: ambiguous argument 'nope': unknown revision or path not in the working tree."
    );
}

#[test]
fn rev_parse_verify_head_of_an_unborn_branch_is_exit_128() {
    let mut w = world();
    let repo = w.empty();
    let out = w.git(repo.dir(), &["rev-parse", "--verify", "HEAD"]);
    // recorded in a fresh `git init`
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: Needed a single revision\n")
    );
}

#[test]
fn rev_parse_verify_quiet_head_of_an_unborn_branch_is_exit_1() {
    let mut w = world();
    let repo = w.empty();
    let out = w.git(repo.dir(), &["rev-parse", "--verify", "--quiet", "HEAD"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "")
    );
}

#[test]
fn rev_parse_verify_quiet_prints_the_sha_of_a_ref_that_exists() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let out = w.git(
        repo.dir(),
        &["rev-parse", "--verify", "--quiet", "refs/heads/main"],
    );
    assert_eq!((out.code, out.stdout), (0, format!("{main}\n")));
}

#[test]
fn rev_parse_verify_quiet_of_rev_colon_path_is_the_blob_id_and_1_when_the_path_is_absent() {
    let mut w = world();
    let repo = w.repo();
    let blob = w.u(|u| u.blob(repo.id(), b"one\n"));
    let found = w.git(
        repo.dir(),
        &["rev-parse", "--verify", "--quiet", "HEAD:README.md"],
    );
    assert_eq!((found.code, found.stdout), (0, format!("{blob}\n")));
    // recorded: `git rev-parse --verify --quiet HEAD:nope.txt`
    let absent = w.git(
        repo.dir(),
        &["rev-parse", "--verify", "--quiet", "HEAD:nope.txt"],
    );
    assert_eq!(
        (absent.code, absent.stdout.as_str(), absent.stderr.as_str()),
        (1, "", "")
    );
}

#[test]
fn rev_parse_verify_sha_caret_tree_is_the_tree_id() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let tree = w.u(|u| u.commit_data(&main)).unwrap().tree;
    let out = w.git(
        repo.dir(),
        &["rev-parse", "--verify", &format!("{main}^{{tree}}")],
    );
    assert_eq!((out.code, out.stdout), (0, format!("{tree}\n")));
}

#[test]
fn rev_parse_verify_of_a_detached_head_is_that_commit() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let other = w.commit_of(&repo, Some(&main), &[("README.md", "two\n")], "two");
    w.u(|u| u.set_head(repo.dir(), Head::Detached(other.clone())));
    let out = w.git(repo.dir(), &["rev-parse", "--verify", "HEAD"]);
    assert_eq!((out.code, out.stdout), (0, format!("{other}\n")));
}

#[test]
fn rev_parse_head_in_a_linked_worktree_is_that_worktrees_head() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let feat = w.commit_of(&repo, Some(&main), &[("f.txt", "f\n")], "feat");
    w.set_ref(&repo, "refs/heads/feat", &feat);
    let wt = w.linked(&repo, "wt", "feat");
    let there = w.git(&wt, &["rev-parse", "--verify", "HEAD"]);
    let here = w.git(repo.dir(), &["rev-parse", "--verify", "HEAD"]);
    assert_eq!(there.stdout, format!("{feat}\n"));
    assert_eq!(here.stdout, format!("{main}\n"));
}

#[test]
fn rev_parse_short_head_is_the_first_seven_hex_characters() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let out = w.git(repo.dir(), &["rev-parse", "--short", "HEAD"]);
    assert_eq!(
        (out.code, out.stdout),
        (0, format!("{}\n", &main.as_str()[..7]))
    );
}

#[test]
fn rev_parse_git_path_prints_the_absolute_path_in_the_git_dir_of_the_main_worktree() {
    let mut w = world();
    let repo = w.repo();
    for name in [
        "index",
        "rebase-merge",
        "rebase-apply",
        "rebase-merge/orig-head",
        "rebase-apply/orig-head",
    ] {
        let out = w.git(
            repo.dir(),
            &["rev-parse", "--path-format=absolute", "--git-path", name],
        );
        assert_eq!(out.code, 0, "{name}");
        assert_eq!(
            out.stdout,
            format!("{}\n", repo.dir().join(".git").join(name).display()),
            "{name}"
        );
    }
}

#[test]
fn rev_parse_git_path_in_a_linked_worktree_is_in_its_admin_dir() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.linked(&repo, "wt", "main");
    for name in ["index", "rebase-merge", "rebase-merge/orig-head"] {
        let out = w.git(
            &wt,
            &["rev-parse", "--path-format=absolute", "--git-path", name],
        );
        assert_eq!(
            out.stdout,
            format!(
                "{}\n",
                repo.dir().join(".git/worktrees/wt").join(name).display()
            ),
            "{name}"
        );
    }
}

#[test]
fn rev_parse_git_path_index_is_the_file_the_universe_rewrites() {
    let mut w = world();
    let repo = w.repo();
    repo.write("a.txt", "a\n");
    repo.add("a.txt");
    let out = w.git(
        repo.dir(),
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let path = PathBuf::from(out.stdout.trim_end());
    assert!(path.is_file(), "{}", path.display());
}

#[test]
fn rev_parse_git_common_dir_is_dot_git_at_the_root_and_the_absolute_git_dir_in_a_linked_worktree() {
    let mut w = world();
    let repo = w.repo();
    let wt = w.linked(&repo, "wt", "main");
    // recorded: `git rev-parse --git-common-dir` in the root of a repo, and in a linked worktree of it
    let at_root = w.git(repo.dir(), &["rev-parse", "--git-common-dir"]);
    assert_eq!((at_root.code, at_root.stdout.as_str()), (0, ".git\n"));
    let linked = w.git(&wt, &["rev-parse", "--git-common-dir"]);
    assert_eq!(
        (linked.code, linked.stdout),
        (0, format!("{}\n", repo.dir().join(".git").display()))
    );
}

#[test]
fn rev_parse_git_common_dir_outside_a_repo_is_exit_128_with_the_first_stderr_line_the_product_reads()
 {
    let w = world();
    let out = w.git(&w.root(), &["rev-parse", "--git-common-dir"]);
    assert_eq!((out.code, out.stderr.as_str()), (128, NOT_A_REPO));
}

// ---- symbolic-ref --------------------------------------------------------------------------

#[test]
fn symbolic_ref_short_quiet_head_prints_the_branch() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["symbolic-ref", "--short", "-q", "HEAD"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "main\n", "")
    );
}

#[test]
fn symbolic_ref_quiet_head_prints_the_full_ref() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["symbolic-ref", "-q", "HEAD"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "refs/heads/main\n"));
}

#[test]
fn symbolic_ref_short_quiet_head_of_an_unborn_branch_still_names_it() {
    let mut w = world();
    let repo = w.empty();
    let out = w.git(repo.dir(), &["symbolic-ref", "--short", "-q", "HEAD"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "main\n"));
}

#[test]
fn symbolic_ref_quiet_head_when_detached_is_exit_1_and_silent() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    w.u(|u| u.set_head(repo.dir(), Head::Detached(main)));
    // recorded: `git symbolic-ref -q HEAD` and `--short -q` on a detached HEAD
    for args in [
        &["symbolic-ref", "-q", "HEAD"][..],
        &["symbolic-ref", "--short", "-q", "HEAD"][..],
    ] {
        let out = w.git(repo.dir(), args);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (1, "", ""),
            "{args:?}"
        );
    }
}

#[test]
fn symbolic_ref_head_without_quiet_when_detached_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    w.u(|u| u.set_head(repo.dir(), Head::Detached(main)));
    let out = w.git(repo.dir(), &["symbolic-ref", "--short", "HEAD"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: ref HEAD is not a symbolic ref\n")
    );
}

#[test]
fn symbolic_ref_short_of_origin_head_strips_refs_remotes() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    w.set_ref(&repo, "refs/remotes/origin/main", &main);
    w.u(|u| {
        u.set_symbolic_ref(
            repo.id(),
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        )
    });
    for args in [
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ][..],
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"][..],
    ] {
        let out = w.git(repo.dir(), args);
        assert_eq!(
            (out.code, out.stdout.as_str()),
            (0, "origin/main\n"),
            "{args:?}"
        );
    }
}

#[test]
fn symbolic_ref_quiet_of_an_unset_origin_head_is_exit_1_and_without_quiet_exit_128() {
    let mut w = world();
    let repo = w.repo();
    // recorded: `git symbolic-ref --quiet --short refs/remotes/origin/HEAD` and without `--quiet`, where it is unset
    let quiet = w.git(
        repo.dir(),
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    );
    assert_eq!(
        (quiet.code, quiet.stdout.as_str(), quiet.stderr.as_str()),
        (1, "", "")
    );
    let loud = w.git(
        repo.dir(),
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    );
    assert_eq!(
        (loud.code, loud.stderr.as_str()),
        (
            128,
            "fatal: ref refs/remotes/origin/HEAD is not a symbolic ref\n"
        )
    );
}

// ---- config and remote ----------------------------------------------------------------------

#[test]
fn config_get_prints_the_value() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add("remote.origin.url", "https://github.com/acme/x.git");
    let out = w.git(repo.dir(), &["config", "--get", "remote.origin.url"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "https://github.com/acme/x.git\n")
    );
}

#[test]
fn config_get_of_a_key_with_several_values_prints_the_last() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    repo.config_add(
        "remote.origin.fetch",
        "+refs/heads/other:refs/remotes/origin/other",
    );
    // recorded: `git config --get remote.origin.fetch` with those two values
    let out = w.git(repo.dir(), &["config", "--get", "remote.origin.fetch"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "+refs/heads/other:refs/remotes/origin/other\n")
    );
}

#[test]
fn config_get_of_an_unset_key_is_exit_1_and_silent() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["config", "--get", "remote.origin.url"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "")
    );
}

#[test]
fn config_get_reads_the_url_as_configured_not_rewritten_by_instead_of() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add("remote.origin.url", "https://github.com/acme/x.git");
    repo.config_add(
        "url./tmp/bare.git.insteadOf",
        "https://github.com/acme/x.git",
    );
    let out = w.git(repo.dir(), &["config", "--get", "remote.origin.url"]);
    assert_eq!(out.stdout, "https://github.com/acme/x.git\n");
}

#[test]
fn config_get_sees_the_global_user_config() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["config", "--get", "user.name"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "cpd test\n"));
}

#[test]
fn config_get_regexp_of_instead_of_prints_the_lowercased_key_a_space_and_the_value() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add(
        "url./tmp/bare.git.insteadOf",
        "https://github.com/acme/x.git",
    );
    // recorded: `git config --get-regexp '^url\..*\.insteadof$'` (the key is printed in lowercase)
    let out = w.git(
        repo.dir(),
        &["config", "--get-regexp", r"^url\..*\.insteadof$"],
    );
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            "url./tmp/bare.git.insteadof https://github.com/acme/x.git\n"
        )
    );
}

#[test]
fn config_get_regexp_of_branch_keys_lists_each_match_on_its_own_line() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add("branch.main.remote", "origin");
    repo.config_add("branch.main.merge", "refs/heads/main");
    repo.config_add("core.other", "x");
    let out = w.git(repo.dir(), &["config", "--get-regexp", r"^branch\."]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            "branch.main.remote origin\nbranch.main.merge refs/heads/main\n"
        )
    );
}

#[test]
fn config_get_regexp_without_a_match_is_exit_1_and_silent() {
    let mut w = world();
    let repo = w.repo();
    // recorded: `git config --get-regexp '^branch\.'` with no such key
    let out = w.git(repo.dir(), &["config", "--get-regexp", r"^branch\."]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (1, "", "")
    );
}

#[test]
fn remote_with_none_prints_nothing_and_lists_names_one_per_line_sorted() {
    let mut w = world();
    let repo = w.repo();
    let none = w.git(repo.dir(), &["remote"]);
    assert_eq!((none.code, none.stdout.as_str()), (0, ""));
    repo.config_add("remote.upstream.url", "https://github.com/up/x.git");
    repo.config_add("remote.origin.url", "https://github.com/acme/x.git");
    let two = w.git(repo.dir(), &["remote"]);
    assert_eq!((two.code, two.stdout.as_str()), (0, "origin\nupstream\n"));
}

#[test]
fn remote_get_url_applies_instead_of() {
    let mut w = world();
    let repo = w.repo();
    repo.config_add("remote.origin.url", "https://github.com/acme/x.git");
    repo.config_add(
        "url./tmp/bare.git.insteadOf",
        "https://github.com/acme/x.git",
    );
    // recorded: `git remote get-url origin` (unlike `config --get remote.origin.url`, it applies insteadOf)
    let out = w.git(repo.dir(), &["remote", "get-url", "origin"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "/tmp/bare.git\n"));
}

#[test]
fn remote_get_url_of_a_missing_remote_is_exit_2() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["remote", "get-url", "origin"]);
    // recorded: `git remote get-url origin` in a repo with no remote
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (2, "", "error: No such remote 'origin'\n")
    );
}

// ---- cat-file, merge-base ---------------------------------------------------------------------

#[test]
fn cat_file_e_of_a_commit_the_repo_holds_is_exit_0_and_silent() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    for spec in [
        format!("{main}^{{commit}}"),
        main.to_string(),
        "HEAD".to_owned(),
    ] {
        let out = w.git(repo.dir(), &["cat-file", "-e", &spec]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (0, "", ""),
            "{spec}"
        );
    }
}

#[test]
fn cat_file_e_of_a_commit_only_another_repo_holds_is_not_known_locally() {
    let mut w = world();
    let repo = w.repo();
    let other = w.repo();
    let foreign = w.commit_of(&other, None, &[("z", "z\n")], "elsewhere");
    // recorded: `git cat-file -e <missing sha>` is exit 1 and silent; with `^{commit}` it is exit 128
    let bare = w.git(repo.dir(), &["cat-file", "-e", foreign.as_str()]);
    assert_eq!(
        (bare.code, bare.stdout.as_str(), bare.stderr.as_str()),
        (1, "", "")
    );
    let spec = format!("{foreign}^{{commit}}");
    let peeled = w.git(repo.dir(), &["cat-file", "-e", &spec]);
    assert_eq!(
        (peeled.code, peeled.stderr),
        (128, format!("fatal: Not a valid object name {spec}\n"))
    );
}

#[test]
fn cat_file_e_of_a_name_that_exists_nowhere_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["cat-file", "-e", "nope"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: Not a valid object name nope\n")
    );
}

#[test]
fn merge_base_is_ancestor_is_exit_0_for_an_ancestor_and_for_itself_and_1_for_a_descendant() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let child = w.commit_of(&repo, Some(&main), &[("README.md", "two\n")], "two");
    w.set_ref(&repo, "refs/heads/feat", &child);
    let yes = w.git(
        repo.dir(),
        &["merge-base", "--is-ancestor", main.as_str(), child.as_str()],
    );
    assert_eq!(
        (yes.code, yes.stdout.as_str(), yes.stderr.as_str()),
        (0, "", "")
    );
    let same = w.git(repo.dir(), &["merge-base", "--is-ancestor", "HEAD", "HEAD"]);
    assert_eq!(same.code, 0);
    let no = w.git(
        repo.dir(),
        &["merge-base", "--is-ancestor", child.as_str(), main.as_str()],
    );
    assert_eq!(
        (no.code, no.stdout.as_str(), no.stderr.as_str()),
        (1, "", "")
    );
    assert!(w.route.unsupported().is_empty());
}

#[test]
fn merge_base_is_ancestor_takes_branch_names() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let child = w.commit_of(&repo, Some(&main), &[("README.md", "two\n")], "two");
    w.set_ref(&repo, "refs/heads/feat", &child);
    assert_eq!(
        w.git(repo.dir(), &["merge-base", "--is-ancestor", "main", "feat"])
            .code,
        0
    );
    assert_eq!(
        w.git(repo.dir(), &["merge-base", "--is-ancestor", "feat", "main"])
            .code,
        1
    );
}

#[test]
fn merge_base_is_ancestor_of_an_unknown_object_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let other = w.repo();
    let foreign = w.commit_of(&other, None, &[("z", "z\n")], "elsewhere");
    // recorded: `git merge-base --is-ancestor HEAD <missing sha>` and with a name that is nothing
    let held_elsewhere = w.git(
        repo.dir(),
        &["merge-base", "--is-ancestor", "HEAD", foreign.as_str()],
    );
    assert_eq!(held_elsewhere.code, 128);
    assert_eq!(
        held_elsewhere.stderr,
        format!("fatal: Not a valid commit name {foreign}\n")
    );
    let nothing = w.git(repo.dir(), &["merge-base", "--is-ancestor", "nope", "HEAD"]);
    assert_eq!(
        (nothing.code, nothing.stderr.as_str()),
        (128, "fatal: Not a valid object name nope\n")
    );
}

// ---- rev-list -----------------------------------------------------------------------------------

#[test]
fn rev_list_count_of_base_dot_dot_head_is_an_integer() {
    let mut w = world();
    let repo = w.repo();
    let base = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    w.set_ref(&repo, "refs/heads/base", &base);
    let none = w.git(repo.dir(), &["rev-list", "--count", "base..HEAD"]);
    assert_eq!((none.code, none.stdout.as_str()), (0, "0\n"));
    repo.write("a.txt", "a\n");
    repo.commit("two");
    repo.write("b.txt", "b\n");
    repo.commit("three");
    let two = w.git(repo.dir(), &["rev-list", "--count", "base..HEAD"]);
    assert_eq!(two.stdout, "2\n");
    let all = w.git(repo.dir(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(all.stdout, "3\n");
}

#[test]
fn rev_list_count_of_a_symmetric_difference_counts_each_sides_own_commits() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let on_main = w.commit_of(&repo, Some(&main), &[("README.md", "m\n")], "on main");
    let on_feat = w.commit_of(&repo, Some(&main), &[("README.md", "f\n")], "on feat");
    let feat2 = w.commit_of(&repo, Some(&on_feat), &[("README.md", "f2\n")], "on feat 2");
    w.set_ref(&repo, "refs/heads/main", &on_main);
    w.set_ref(&repo, "refs/heads/feat", &feat2);
    let out = w.git(repo.dir(), &["rev-list", "--count", "main...feat"]);
    assert_eq!(out.stdout, "3\n");
}

#[test]
fn rev_list_lists_shas_newest_first_one_per_line() {
    let mut w = world();
    let repo = w.repo();
    let first = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    repo.write("a.txt", "a\n");
    let second = repo.commit("two");
    let out = w.git(repo.dir(), &["rev-list", "HEAD"]);
    assert_eq!((out.code, out.stdout), (0, format!("{second}\n{first}\n")));
}

#[test]
fn rev_list_no_merges_head_not_remotes_origin_lists_the_unpushed_commits() {
    let mut w = world();
    let repo = w.repo();
    let base = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    w.set_ref(&repo, "refs/remotes/origin/main", &base);
    let clean = w.git(
        repo.dir(),
        &[
            "rev-list",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    );
    assert_eq!((clean.code, clean.stdout.as_str()), (0, ""));
    repo.write("a.txt", "a\n");
    let local = repo.commit("local");
    let out = w.git(
        repo.dir(),
        &[
            "rev-list",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    );
    assert_eq!((out.code, out.stdout), (0, format!("{local}\n")));
}

#[test]
fn rev_list_not_remotes_origin_and_an_oid_leaves_out_what_that_oid_reaches_too() {
    let mut w = world();
    let repo = w.repo();
    let base = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    w.set_ref(&repo, "refs/remotes/origin/main", &base);
    repo.write("a.txt", "a\n");
    let local = repo.commit("local");
    let out = w.git(
        repo.dir(),
        &[
            "rev-list",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
            local.as_str(),
        ],
    );
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
}

#[test]
fn rev_list_no_merges_skips_a_merge_commit() {
    let mut w = world();
    let repo = w.repo();
    let base = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let left = w.commit_of(
        &repo,
        Some(&base),
        &[("README.md", "one\n"), ("l", "l\n")],
        "left",
    );
    let right = w.commit_of(
        &repo,
        Some(&base),
        &[("README.md", "one\n"), ("r", "r\n")],
        "right",
    );
    let merge = w.u(|u| {
        let blobs: Vec<Oid> = ["one\n", "l\n", "r\n"]
            .iter()
            .map(|t| u.blob(repo.id(), t.as_bytes()))
            .collect();
        let tree = u.tree(
            repo.id(),
            &[("README.md", &blobs[0]), ("l", &blobs[1]), ("r", &blobs[2])],
        );
        u.commit(repo.id(), &tree, &[left.clone(), right.clone()], "merge")
    });
    w.set_ref(&repo, "refs/heads/main", &merge);
    let out = w.git(
        repo.dir(),
        &[
            "rev-list",
            "--no-merges",
            "--count",
            &format!("{base}..HEAD"),
        ],
    );
    assert_eq!(out.stdout, "2\n");
    let with = w.git(
        repo.dir(),
        &["rev-list", "--count", &format!("{base}..HEAD")],
    );
    assert_eq!(with.stdout, "3\n");
}

#[test]
fn rev_list_of_an_unknown_revision_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    // recorded: `git rev-list nope` and `git rev-list --count nope..HEAD`
    let plain = w.git(repo.dir(), &["rev-list", "nope"]);
    assert_eq!(plain.code, 128);
    assert_eq!(
        first_line(&plain.stderr),
        "fatal: ambiguous argument 'nope': unknown revision or path not in the working tree."
    );
    let ranged = w.git(repo.dir(), &["rev-list", "--count", "nope..HEAD"]);
    assert_eq!(ranged.code, 128);
    assert_eq!(
        first_line(&ranged.stderr),
        "fatal: ambiguous argument 'nope..HEAD': unknown revision or path not in the working tree."
    );
}

// ---- ls-tree, log -g ------------------------------------------------------------------------------

#[test]
fn ls_tree_r_name_only_z_prints_every_path_nul_terminated_in_order() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let feat = w.commit_of(
        &repo,
        Some(&main),
        &[
            ("README.md", "one\n"),
            ("f.txt", "f\n"),
            ("dir/sub/g.txt", "g\n"),
        ],
        "feat",
    );
    w.set_ref(&repo, "refs/heads/feat", &feat);
    // recorded: `git ls-tree -r --name-only -z feat` on that tree
    let out = w.git(repo.dir(), &["ls-tree", "-r", "--name-only", "-z", "feat"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "README.md\0dir/sub/g.txt\0f.txt\0")
    );
}

#[test]
fn ls_tree_of_a_rev_that_names_nothing_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["ls-tree", "-r", "--name-only", "-z", "nope"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (128, "", "fatal: Not a valid object name nope\n")
    );
}

#[test]
fn log_g_format_t_prints_the_trees_of_a_branchs_reflog_newest_first() {
    let mut w = world();
    let repo = w.empty();
    repo.write("a.txt", "a\n");
    let first = repo.commit("one");
    repo.write("a.txt", "b\n");
    let second = repo.commit("two");
    let tree = |c: &Oid| w.u(|u| u.commit_data(c)).unwrap().tree;
    let out = w.git(repo.dir(), &["log", "-g", "--format=%T", "refs/heads/main"]);
    assert_eq!(
        (out.code, out.stdout),
        (0, format!("{}\n{}\n", tree(&second), tree(&first)))
    );
}

#[test]
fn log_g_of_a_ref_that_does_not_exist_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    // recorded: `git log -g --format=%T refs/heads/nope`
    let out = w.git(repo.dir(), &["log", "-g", "--format=%T", "refs/heads/nope"]);
    assert_eq!(out.code, 128);
    assert_eq!(
        first_line(&out.stderr),
        "fatal: ambiguous argument 'refs/heads/nope': unknown revision or path not in the working tree."
    );
}

#[test]
fn log_format_s_prints_the_subjects_newest_first_and_dash_1_only_the_newest() {
    let mut w = world();
    let repo = w.empty();
    repo.write("a.txt", "a\n");
    repo.commit("one");
    repo.write("a.txt", "b\n");
    repo.commit("two");
    // git's own semantics: `git log --format=%s` is one subject per line, newest first; `-1` stops after the first.
    let all = w.git(repo.dir(), &["log", "--format=%s"]);
    assert_eq!((all.code, all.stdout.as_str()), (0, "two\none\n"));
    let newest = w.git(repo.dir(), &["log", "-1", "--format=%s"]);
    assert_eq!((newest.code, newest.stdout.as_str()), (0, "two\n"));
    assert!(w.route.unsupported().is_empty());
}

// ---- status --porcelain ------------------------------------------------------------------------------

#[test]
fn status_porcelain_of_a_clean_tree_is_empty() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["status", "--porcelain"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
}

#[test]
fn status_porcelain_lists_a_modified_file_a_staged_new_file_and_an_untracked_one() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    repo.write("new.txt", "n\n");
    repo.add("new.txt");
    repo.write("loose.txt", "l\n");
    // recorded: ` M README.md`, `A  new.txt`, `?? loose.txt`, tracked paths first
    let out = w.git(repo.dir(), &["status", "--porcelain"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, " M README.md\nA  new.txt\n?? loose.txt\n")
    );
}

#[test]
fn status_porcelain_collapses_an_untracked_directory_to_one_line() {
    let mut w = world();
    let repo = w.repo();
    repo.write("u/q", "q\n");
    repo.write("u/deep/r", "r\n");
    let out = w.git(repo.dir(), &["status", "--porcelain"]);
    assert_eq!(out.stdout, "?? u/\n");
}

#[test]
fn ls_files_others_lists_the_untracked_entries_nul_terminated_and_collapses_a_directory() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    repo.write("loose.txt", "l\n");
    repo.write("u/q", "q\n");
    repo.write("u/deep/r", "r\n");
    let args = ["ls-files", "--others", "--exclude-standard", "--directory"];
    let out = w.git(repo.dir(), &[args.as_slice(), &["-z"]].concat());
    assert_eq!((out.code, out.stdout.as_str()), (0, "loose.txt\0u/\0"));
    let out = w.git(repo.dir(), &args);
    assert_eq!((out.code, out.stdout.as_str()), (0, "loose.txt\nu/\n"));
}

#[test]
fn ls_files_without_others_is_unsupported() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["ls-files"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "fake-git: unsupported: ls-files (without --others)\n")
    );
}

#[test]
fn status_porcelain_marks_a_tracked_file_deleted_from_disk() {
    let mut w = world();
    let repo = w.repo();
    std::fs::remove_file(repo.dir().join("README.md")).unwrap();
    // recorded: ` D a` after `rm a`
    let out = w.git(repo.dir(), &["status", "--porcelain"]);
    assert_eq!(out.stdout, " D README.md\n");
}

#[test]
fn status_porcelain_marks_a_path_with_all_three_stages_unmerged() {
    let mut w = world();
    let repo = w.repo();
    repo.write("a.txt", "<<<<<<< ours\n");
    w.u(|u| {
        for (stage, text) in [(1, "base\n"), (2, "ours\n"), (3, "theirs\n")] {
            let blob = u.blob(repo.id(), text.as_bytes());
            u.stage(repo.dir(), "a.txt", &blob, stage);
        }
    });
    let out = w.git(repo.dir(), &["status", "--porcelain"]);
    assert_eq!(out.stdout, "UU a.txt\n");
}

#[test]
fn status_porcelain_untracked_files_no_leaves_out_the_untracked() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    repo.write("loose.txt", "l\n");
    repo.write("u/q", "q\n");
    let out = w.git(
        repo.dir(),
        &["status", "--porcelain", "--untracked-files=no"],
    );
    assert_eq!((out.code, out.stdout.as_str()), (0, " M README.md\n"));
}

#[test]
fn status_with_no_optional_locks_answers_the_same() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    let plain = w.git(repo.dir(), &["status", "--porcelain"]);
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo.dir())
        .args(["--no-optional-locks", "status", "--porcelain"]);
    let unlocked = run(&w.runner, &mut cmd);
    assert_eq!((unlocked.code, unlocked.stdout), (0, plain.stdout));
}

// ---- diff ----------------------------------------------------------------------------------------------

#[test]
fn diff_quiet_is_exit_0_when_the_working_tree_matches_the_index_and_1_when_it_differs() {
    let mut w = world();
    let repo = w.repo();
    let clean = w.git(repo.dir(), &["diff", "--quiet"]);
    assert_eq!(
        (clean.code, clean.stdout.as_str(), clean.stderr.as_str()),
        (0, "", "")
    );
    repo.write("README.md", "changed\n");
    let dirty = w.git(repo.dir(), &["diff", "--quiet"]);
    assert_eq!(
        (dirty.code, dirty.stdout.as_str(), dirty.stderr.as_str()),
        (1, "", "")
    );
    assert!(w.route.unsupported().is_empty());
}

#[test]
fn diff_quiet_does_not_see_a_change_that_is_already_staged() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    repo.add("README.md");
    let out = w.git(repo.dir(), &["diff", "--quiet"]);
    assert_eq!(out.code, 0);
}

#[test]
fn diff_quiet_with_no_optional_locks_is_the_form_of_the_lead_round() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo.dir())
        .args(["--no-optional-locks", "diff", "--quiet"]);
    assert_eq!(run(&w.runner, &mut cmd).code, 1);
}

#[test]
fn diff_name_only_diff_filter_u_lists_the_unmerged_paths_one_per_line() {
    let mut w = world();
    let repo = w.repo();
    w.u(|u| {
        for path in ["b.txt", "a.txt"] {
            for (stage, text) in [(1, "base\n"), (2, "ours\n"), (3, "theirs\n")] {
                let blob = u.blob(repo.id(), text.as_bytes());
                u.stage(repo.dir(), path, &blob, stage);
            }
        }
    });
    let out = w.git(repo.dir(), &["diff", "--name-only", "--diff-filter=U"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "a.txt\nb.txt\n"));
}

#[test]
fn diff_name_only_diff_filter_u_of_a_merged_tree_is_empty() {
    let mut w = world();
    let repo = w.repo();
    let out = w.git(repo.dir(), &["diff", "--name-only", "--diff-filter=U"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
}

#[test]
fn diff_name_only_z_head_lists_tracked_changes_staged_or_not_and_no_untracked() {
    let mut w = world();
    let repo = w.repo();
    repo.write("README.md", "changed\n");
    repo.write("new.txt", "n\n");
    repo.add("new.txt");
    repo.write("loose.txt", "l\n");
    // recorded: `git diff --name-only -z HEAD`
    let out = w.git(repo.dir(), &["diff", "--name-only", "-z", "HEAD"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "README.md\0new.txt\0"));
}

#[test]
fn diff_name_only_no_renames_z_triple_dot_lists_what_the_branch_changed_since_the_fork() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let on_main = w.commit_of(
        &repo,
        Some(&main),
        &[("README.md", "one\n"), ("m.txt", "m\n")],
        "on main",
    );
    let on_feat = w.commit_of(
        &repo,
        Some(&main),
        &[
            ("README.md", "one\n"),
            ("f.txt", "f\n"),
            ("dir/sub/g.txt", "g\n"),
        ],
        "on feat",
    );
    w.set_ref(&repo, "refs/heads/main", &on_main);
    w.set_ref(&repo, "refs/heads/feat", &on_feat);
    // recorded: `git diff --name-only --no-renames -z main...feat`; m.txt (main's own) is not in it
    let out = w.git(
        repo.dir(),
        &["diff", "--name-only", "--no-renames", "-z", "main...feat"],
    );
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "dir/sub/g.txt\0f.txt\0")
    );
}

#[test]
fn diff_name_only_without_z_separates_the_names_by_newlines() {
    let mut w = world();
    let repo = w.repo();
    let main = w.u(|u| u.ref_oid(repo.id(), "refs/heads/main")).unwrap();
    let feat = w.commit_of(
        &repo,
        Some(&main),
        &[
            ("README.md", "one\n"),
            ("f.txt", "f\n"),
            ("dir/sub/g.txt", "g\n"),
        ],
        "feat",
    );
    w.set_ref(&repo, "refs/heads/feat", &feat);
    let out = w.git(repo.dir(), &["diff", "--name-only", "main...feat"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "dir/sub/g.txt\nf.txt\n")
    );
}

#[test]
fn diff_of_a_revision_that_names_nothing_is_exit_128() {
    let mut w = world();
    let repo = w.repo();
    // recorded: `git diff --name-only nope...main`
    let out = w.git(repo.dir(), &["diff", "--name-only", "nope...main"]);
    assert_eq!(out.code, 128);
    assert_eq!(
        first_line(&out.stderr),
        "fatal: ambiguous argument 'nope...main': unknown revision or path not in the working tree."
    );
}

// ---- ls-remote ------------------------------------------------------------------------------------------

#[test]
fn ls_remote_heads_prints_the_sha_a_tab_and_the_ref() {
    let mut w = world();
    let repo = w.repo();
    let origin = repo.bare_origin(&w.root().join("origin.git"));
    repo.push();
    let main = w.u(|u| u.ref_oid(origin.id(), "refs/heads/main")).unwrap();
    // recorded: `git ls-remote --heads origin refs/heads/main`
    let out = w.git(
        repo.dir(),
        &["ls-remote", "--heads", "origin", "refs/heads/main"],
    );
    assert_eq!(
        (out.code, out.stdout),
        (0, format!("{main}\trefs/heads/main\n"))
    );
}

#[test]
fn ls_remote_exit_code_of_a_missing_branch_is_exit_2_and_of_a_present_one_exit_0() {
    let mut w = world();
    let repo = w.repo();
    let _origin = repo.bare_origin(&w.root().join("origin.git"));
    repo.push();
    // recorded: `git ls-remote --exit-code --heads origin nope`
    let missing = w.git(
        repo.dir(),
        &["ls-remote", "--exit-code", "--heads", "origin", "nope"],
    );
    assert_eq!(
        (
            missing.code,
            missing.stdout.as_str(),
            missing.stderr.as_str()
        ),
        (2, "", "")
    );
    let present = w.git(
        repo.dir(),
        &["ls-remote", "--exit-code", "--heads", "origin", "main"],
    );
    assert_eq!(present.code, 0);
    assert!(
        present.stdout.ends_with("\trefs/heads/main\n"),
        "{}",
        present.stdout
    );
}

#[test]
fn ls_remote_of_a_missing_ref_without_exit_code_is_exit_0_and_empty() {
    let mut w = world();
    let repo = w.repo();
    let _origin = repo.bare_origin(&w.root().join("origin.git"));
    repo.push();
    let out = w.git(repo.dir(), &["ls-remote", "origin", "refs/heads/nope"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
}

#[test]
fn ls_remote_goes_through_instead_of_to_the_repo_it_names() {
    let mut w = world();
    let repo = w.repo();
    let dir = w.root().join("origin.git");
    let origin = w.u(|u| u.init(&dir, true));
    repo.keyed("x");
    repo.config_add(
        &format!("url.{}.insteadOf", dir.display()),
        "https://github.com/acme/x.git",
    );
    repo.push();
    let main = w.u(|u| u.ref_oid(origin, "refs/heads/main")).unwrap();
    let out = w.git(
        repo.dir(),
        &["ls-remote", "--exit-code", "--heads", "origin", "main"],
    );
    assert_eq!(
        (out.code, out.stdout),
        (0, format!("{main}\trefs/heads/main\n"))
    );
}
