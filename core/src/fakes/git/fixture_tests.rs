//! The test-only subcommands of the fake git: the ones the product never runs and the tests of `cli/tests`
//! use to build their repos (`init add commit clone switch checkout reset merge tag mv rm show for-each-ref
//! diff-index diff-files update-index`, the writing forms of `config`, `remote` and `branch`, and the flags
//! `-q`, `-b`, `-B`, `-X ours`, `--no-ff`, `--no-edit`), served by the same table as the product's, the literal
//! lines of a `.gitignore`, the collapse of an untracked directory in `status`, and the `Repo` handle
//! (`write`, `commit`, `branch`, `push`, `bare_origin`, `keyed`) mixed with them, which `fx.git(dir, args)` of
//! the test kit will use.
//!
//! Every texts and exit code asserted is what real git printed for the same call, recorded once by hand in a
//! scratch directory. The git at hand was 2.54.0 (Apple Git-157), not 2.43; none of these texts changed
//! between the two as far as I know (unverified on 2.43). Where the text is long or version bound (a diffstat,
//! a `hint:`), a test reads the exit code, the first line or the state instead.
//!
//! What is not asserted, on purpose: the rename detection of `status` after `mv` (real git prints
//! `R  a -> b`; the fake does not model renames, so the tests read the tree after a commit).
//!
//! The repos are built through the route itself (`git init`, `git add`, `git commit`), the way a test of
//! `cli/tests` does, on the `main` branch (`init.defaultBranch` of the test kit's global config). No test runs
//! git (or any process), reads the wall clock or sleeps: every call is a `Command` handed to the `FakeRunner`,
//! which never starts it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::Oid;
use super::scenario::{Out, World, p, short, world};
use crate::host::deadline::Failed;
use crate::host::runner::Runner;

/// A world whose global config is the test kit's `.gitconfig`: `user.*` and `init.defaultBranch main`.
fn fx() -> World {
    let w = world();
    w.u(|u| u.set_global("init.defaultBranch", "main"));
    w
}

/// `git <args>` with `dir` as the cwd and no `-C`: how `env.git(dir, args)` of the tests runs.
fn cwd(w: &World, dir: &Path, args: &[&str]) -> Out {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    match w.runner.output(&mut cmd, "git", Duration::from_secs(60)) {
        Ok(o) => Out {
            code: std::os::unix::process::ExitStatusExt::into_raw(o.status) >> 8,
            stdout: String::from_utf8(o.stdout).unwrap(),
            stderr: String::from_utf8(o.stderr).unwrap(),
        },
        Err(Failed::Io(e)) => panic!("git could not start: {e}"),
        Err(Failed::Timeout(e)) => panic!("git timed out: {e:?}"),
    }
}

/// [`cwd`] that must exit 0; returns stdout.
fn cwd_ok(w: &World, dir: &Path, args: &[&str]) -> String {
    let out = cwd(w, dir, args);
    assert_eq!(out.code, 0, "git {args:?} in {dir:?} said: {}", out.stderr);
    out.stdout
}

/// The first line of a text, `""` when it is empty.
fn first(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// A repo on `main` with one commit, `base`, holding `files`, made by `git init`, `git add -A` and `git commit`.
fn made(w: &mut World, files: &[(&str, &str)]) -> PathBuf {
    let dir = w.fresh("repo");
    w.ok(&w.root(), &["init", "-q", "-b", "main", p(&dir)]);
    for (path, text) in files {
        w.write(&dir, path, text);
    }
    w.ok(&dir, &["add", "-A"]);
    w.ok(&dir, &["commit", "-q", "-m", "base"]);
    dir
}

/// A repo on `main` whose `base` holds `README.md` = `one\n`.
fn one(w: &mut World) -> PathBuf {
    made(w, &[("README.md", "one\n")])
}

/// Writes `path`, stages everything and commits it as `message`.
fn commit_file(w: &World, dir: &Path, path: &str, text: &str, message: &str) {
    w.write(dir, path, text);
    w.ok(dir, &["add", "-A"]);
    w.ok(dir, &["commit", "-q", "-m", message]);
}

fn status(w: &World, dir: &Path) -> String {
    w.ok(dir, &["status", "--porcelain"])
}

/// What `git rev-parse --verify <rev>` prints in `dir`, trimmed.
fn sha(w: &World, dir: &Path, rev: &str) -> String {
    w.ok(dir, &["rev-parse", "--verify", rev]).trim().to_owned()
}

fn branches(w: &World, dir: &Path) -> Vec<String> {
    w.ok(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

/// A repo on `main` (`README.md` = `one\n`) and a branch `feat` that adds `feat.txt`, while `main` adds
/// `main.txt`; `HEAD` is on `main`.
fn forked(w: &mut World) -> PathBuf {
    let dir = one(w);
    w.ok(&dir, &["checkout", "-q", "-b", "feat"]);
    commit_file(w, &dir, "feat.txt", "feat\n", "feat");
    w.ok(&dir, &["checkout", "-q", "main"]);
    commit_file(w, &dir, "main.txt", "main\n", "mainside");
    dir
}

/// A repo on `main` and a branch `feat` that edits `README.md`, where `main` edits it too (a content conflict).
fn clashing(w: &mut World) -> PathBuf {
    let dir = one(w);
    w.ok(&dir, &["checkout", "-q", "-b", "feat"]);
    commit_file(w, &dir, "README.md", "feat\n", "feat");
    w.ok(&dir, &["checkout", "-q", "main"]);
    commit_file(w, &dir, "README.md", "main\n", "mainside");
    dir
}

// ---- init ---------------------------------------------------------------------------------------------

#[test]
fn init_makes_a_repo_on_the_default_branch_and_says_where() {
    let mut w = fx();
    let dir = w.fresh("new");
    let out = w.git(&w.root(), &["init", p(&dir)]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            format!(
                "Initialized empty Git repository in {}/.git/\n",
                dir.display()
            )
            .as_str()
        ),
        "{}",
        out.stderr
    );
    assert_eq!(w.ok(&dir, &["symbolic-ref", "--short", "HEAD"]), "main\n");
}

#[test]
fn init_makes_a_repo_with_no_commit_yet() {
    let mut w = fx();
    let dir = w.fresh("new");
    w.ok(&w.root(), &["init", "-q", p(&dir)]);
    assert_eq!(
        w.git(&dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
            .code,
        1
    );
    assert_eq!(status(&w, &dir), "");
}

#[test]
fn init_dash_q_prints_nothing() {
    let mut w = fx();
    let dir = w.fresh("new");
    let out = w.git(&w.root(), &["init", "-q", p(&dir)]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
}

#[test]
fn init_dash_b_and_initial_branch_name_the_unborn_branch() {
    let mut w = fx();
    let (a, b) = (w.fresh("a"), w.fresh("b"));
    w.ok(&w.root(), &["init", "-q", "-b", "trunk", p(&a)]);
    w.ok(&w.root(), &["init", "-q", "--initial-branch=dev", p(&b)]);
    assert_eq!(w.ok(&a, &["symbolic-ref", "HEAD"]), "refs/heads/trunk\n");
    assert_eq!(w.ok(&b, &["symbolic-ref", "--short", "HEAD"]), "dev\n");
}

#[test]
fn init_without_a_directory_makes_the_repo_in_the_cwd() {
    let mut w = fx();
    let dir = w.fresh("here");
    std::fs::create_dir_all(&dir).unwrap();
    let out = cwd(&w, &dir, &["init", "-q", "-b", "main"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        cwd_ok(&w, &dir, &["symbolic-ref", "--short", "HEAD"]),
        "main\n"
    );
}

#[test]
fn init_of_an_existing_repo_reinitialises_it_and_keeps_its_commits() {
    let mut w = fx();
    let dir = one(&mut w);
    let head = sha(&w, &dir, "HEAD");
    let out = w.git(&dir, &["init"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            format!(
                "Reinitialized existing Git repository in {}/.git/\n",
                dir.display()
            )
            .as_str()
        ),
        "{}",
        out.stderr
    );
    assert_eq!(sha(&w, &dir, "HEAD"), head);
}

#[test]
fn init_bare_makes_a_bare_repo_that_has_no_work_tree() {
    let mut w = fx();
    let dir = w.fresh("origin.git");
    let out = w.git(&w.root(), &["init", "--bare", "-b", "main", p(&dir)]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            format!("Initialized empty Git repository in {}/\n", dir.display()).as_str()
        ),
        "{}",
        out.stderr
    );
    assert_eq!(w.ok(&dir, &["symbolic-ref", "HEAD"]), "refs/heads/main\n");
    let status = w.git(&dir, &["status", "--porcelain"]);
    assert_eq!(
        (status.code, status.stderr.as_str()),
        (128, "fatal: this operation must be run in a work tree\n")
    );
}

// ---- add, status, .gitignore ------------------------------------------------------------------------------

#[test]
fn add_of_a_file_stages_it_as_added() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "new.txt", "new\n");
    assert_eq!(status(&w, &dir), "?? new.txt\n");
    w.ok(&dir, &["add", "new.txt"]);
    assert_eq!(status(&w, &dir), "A  new.txt\n");
}

#[test]
fn add_of_an_edited_file_stages_the_edit() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "README.md", "two\n");
    assert_eq!(status(&w, &dir), " M README.md\n");
    w.ok(&dir, &["add", "README.md"]);
    assert_eq!(status(&w, &dir), "M  README.md\n");
}

#[test]
fn add_dash_a_stages_edits_deletions_and_new_files() {
    let mut w = fx();
    let dir = made(
        &mut w,
        &[("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")],
    );
    w.write(&dir, "a.txt", "a2\n");
    std::fs::remove_file(dir.join("b.txt")).unwrap();
    w.write(&dir, "d/e.txt", "e\n");
    w.ok(&dir, &["add", "-A"]);
    assert_eq!(status(&w, &dir), "M  a.txt\nD  b.txt\nA  d/e.txt\n");
}

#[test]
fn add_dot_stages_what_is_under_the_directory() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "x.txt", "x\n");
    w.write(&dir, "sub/y.txt", "y\n");
    w.ok(&dir, &["add", "."]);
    assert_eq!(status(&w, &dir), "A  sub/y.txt\nA  x.txt\n");
}

#[test]
fn add_in_the_cwd_form_stages_the_file() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "x.txt", "x\n");
    cwd_ok(&w, &dir, &["add", "x.txt"]);
    assert_eq!(cwd_ok(&w, &dir, &["status", "--porcelain"]), "A  x.txt\n");
}

#[test]
fn add_of_a_path_that_does_not_exist_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["add", "nothing"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: pathspec 'nothing' did not match any files\n")
    );
}

#[test]
fn status_collapses_an_untracked_directory_to_one_line() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "dir/inner/f", "1\n");
    w.write(&dir, "dir/g", "2\n");
    w.write(&dir, "other/h", "3\n");
    w.write(&dir, "top", "4\n");
    assert_eq!(status(&w, &dir), "?? dir/\n?? other/\n?? top\n");
}

#[test]
fn status_lists_the_untracked_files_of_a_directory_that_has_tracked_ones() {
    let mut w = fx();
    let dir = made(&mut w, &[("tracked/t", "t\n")]);
    w.write(&dir, "tracked/u", "u\n");
    w.write(&dir, "tracked/sub/v", "v\n");
    assert_eq!(status(&w, &dir), "?? tracked/sub/\n?? tracked/u\n");
}

#[test]
fn status_with_untracked_files_no_hides_the_untracked() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "dir/f", "1\n");
    w.write(&dir, "README.md", "two\n");
    assert_eq!(
        w.ok(&dir, &["status", "--porcelain", "--untracked-files=no"]),
        " M README.md\n"
    );
}

#[test]
fn gitignore_literal_lines_hide_files_from_status_and_from_add_dash_a() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, ".gitignore", "# a comment\n\nbuild\nsecret.txt\n");
    w.write(&dir, "build/out", "o\n");
    w.write(&dir, "secret.txt", "s\n");
    w.write(&dir, "keep.txt", "k\n");
    assert_eq!(status(&w, &dir), "?? .gitignore\n?? keep.txt\n");
    w.ok(&dir, &["add", "-A"]);
    assert_eq!(status(&w, &dir), "A  .gitignore\nA  keep.txt\n");
}

#[test]
fn a_gitignore_line_with_a_trailing_slash_ignores_the_directory() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, ".gitignore", "cache/\n");
    w.write(&dir, "cache/f", "c\n");
    w.ok(&dir, &["add", "-A"]);
    assert_eq!(status(&w, &dir), "A  .gitignore\n");
}

#[test]
fn an_ignored_file_that_is_already_tracked_still_shows_its_changes() {
    let mut w = fx();
    let dir = made(&mut w, &[("tracked.txt", "1\n")]);
    w.write(&dir, ".gitignore", "tracked.txt\n");
    w.write(&dir, "tracked.txt", "2\n");
    assert_eq!(status(&w, &dir), " M tracked.txt\n?? .gitignore\n");
}

#[test]
fn add_of_an_ignored_path_exits_1_and_names_the_rule() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, ".gitignore", "secret.txt\n");
    w.write(&dir, "secret.txt", "s\n");
    let out = w.git(&dir, &["add", "secret.txt"]);
    assert_eq!(
        (out.code, first(&out.stderr)),
        (
            1,
            "The following paths are ignored by one of your .gitignore files:"
        )
    );
    assert_eq!(status(&w, &dir), "?? .gitignore\n");
}

#[test]
fn a_gitignore_glob_is_unsupported_and_recorded_not_guessed() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, ".gitignore", "*.log\n");
    w.write(&dir, "x.log", "l\n");
    let out = w.git(&dir, &["status", "--porcelain"]);
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.starts_with("fake-git: unsupported: "),
        "{}",
        out.stderr
    );
    assert_eq!(
        w.route.unsupported().len(),
        1,
        "{:?}",
        w.route.unsupported()
    );
}

// ---- commit -------------------------------------------------------------------------------------------

#[test]
fn the_first_commit_says_root_commit_and_what_it_holds() {
    let mut w = fx();
    let dir = w.fresh("new");
    w.ok(&w.root(), &["init", "-q", "-b", "main", p(&dir)]);
    w.write(&dir, "README.md", "one\n");
    w.ok(&dir, &["add", "README.md"]);
    let out = w.git(&dir, &["commit", "-m", "first"]);
    let head = short(sha(&w, &dir, "HEAD"));
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, format!("[main (root-commit) {head}] first\n 1 file changed, 1 insertion(+)\n create mode 100644 README.md\n").as_str()),
        "{}",
        out.stderr
    );
}

#[test]
fn a_later_commit_says_the_branch_and_the_short_sha() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "a.txt", "a\n");
    w.ok(&dir, &["add", "-A"]);
    let out = w.git(&dir, &["commit", "-m", "second"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        first(&out.stdout),
        format!("[main {}] second", short(sha(&w, &dir, "HEAD")))
    );
}

#[test]
fn commit_dash_q_prints_nothing_and_moves_the_branch() {
    let mut w = fx();
    let dir = one(&mut w);
    let before = sha(&w, &dir, "refs/heads/main");
    w.write(&dir, "a.txt", "a\n");
    w.ok(&dir, &["add", "-A"]);
    let out = w.git(&dir, &["commit", "-q", "-m", "second"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_ne!(sha(&w, &dir, "refs/heads/main"), before);
    assert_eq!(status(&w, &dir), "");
}

#[test]
fn commit_makes_a_commit_of_the_staged_tree_over_head() {
    let mut w = fx();
    let dir = one(&mut w);
    let before = sha(&w, &dir, "HEAD");
    w.write(&dir, "a.txt", "a\n");
    w.write(&dir, "b.txt", "b\n");
    w.ok(&dir, &["add", "a.txt"]);
    w.ok(&dir, &["commit", "-q", "-m", "only a"]);
    let commit = w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD")));
    assert_eq!(
        (commit.parents, commit.message.as_str()),
        (vec![Oid::new(&before)], "only a")
    );
    assert_eq!(w.paths_at(&dir, "HEAD"), ["README.md", "a.txt"]);
    assert_eq!(status(&w, &dir), "?? b.txt\n");
}

#[test]
fn commit_with_nothing_to_commit_exits_1() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["commit", "-m", "again"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (1, "On branch main\nnothing to commit, working tree clean\n")
    );
}

#[test]
fn commit_allow_empty_makes_a_commit_with_the_tree_of_its_parent() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["commit", "-q", "--allow-empty", "-m", "hooks"]);
    assert_eq!(sha(&w, &dir, "HEAD^{tree}"), sha(&w, &dir, "HEAD~1^{tree}"));
    let commit = w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD")));
    assert_eq!(
        (commit.parents.len(), commit.message.as_str()),
        (1, "hooks")
    );
    assert_eq!(w.ok(&dir, &["rev-list", "--count", "HEAD"]), "2\n");
}

#[test]
fn commit_allow_empty_works_on_an_unborn_branch() {
    let mut w = fx();
    let dir = w.fresh("new");
    w.ok(&w.root(), &["init", "-q", "-b", "main", p(&dir)]);
    w.ok(&dir, &["commit", "-q", "-m", "init", "--allow-empty"]);
    assert_eq!(w.ok(&dir, &["rev-list", "--count", "HEAD"]), "1\n");
}

#[test]
fn commit_takes_the_author_from_the_config_and_dash_c_overrides_it() {
    let mut w = fx();
    let dir = one(&mut w);
    let base = w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD")));
    assert_eq!(base.author, "cpd test <test@cpd.invalid>");
    w.write(&dir, "a.txt", "a\n");
    w.ok(&dir, &["add", "-A"]);
    w.ok(
        &dir,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "docs",
        ],
    );
    assert_eq!(
        w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD"))).author,
        "t <t@t.invalid>"
    );
}

#[test]
fn commit_takes_the_author_a_repo_config_sets() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["config", "user.name", "someone"]);
    w.ok(&dir, &["config", "user.email", "someone@cpd.invalid"]);
    commit_file(&w, &dir, "a.txt", "a\n", "mine");
    assert_eq!(
        w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD"))).author,
        "someone <someone@cpd.invalid>"
    );
}

#[test]
fn commit_in_the_cwd_form_commits_the_repo_of_the_cwd() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "a.txt", "a\n");
    cwd_ok(&w, &dir, &["add", "-A"]);
    cwd_ok(&w, &dir, &["commit", "-q", "-m", "second"]);
    assert_eq!(w.paths_at(&dir, "HEAD"), ["README.md", "a.txt"]);
}

#[test]
fn commit_on_a_detached_head_moves_only_head() {
    let mut w = fx();
    let dir = one(&mut w);
    let main = sha(&w, &dir, "main");
    w.ok(&dir, &["checkout", "-q", "--detach"]);
    commit_file(&w, &dir, "a.txt", "a\n", "loose");
    assert_eq!(sha(&w, &dir, "main"), main);
    assert_ne!(sha(&w, &dir, "HEAD"), main);
    assert_eq!(w.git(&dir, &["symbolic-ref", "-q", "HEAD"]).code, 1);
}

#[test]
fn a_commit_is_in_the_reflog_of_its_branch_with_its_tree() {
    let mut w = fx();
    let dir = one(&mut w);
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    let trees = w.ok(&dir, &["log", "-g", "--format=%T", "refs/heads/main"]);
    let (head_tree, first_tree) = (sha(&w, &dir, "HEAD^{tree}"), sha(&w, &dir, "HEAD~1^{tree}"));
    assert_eq!(trees, format!("{head_tree}\n{first_tree}\n"));
}

// ---- show, for-each-ref -----------------------------------------------------------------------------------

#[test]
fn show_of_a_colon_path_prints_the_staged_file() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "README.md", "staged by the Lead\n");
    w.ok(&dir, &["add", "README.md"]);
    w.write(&dir, "README.md", "edited after\n");
    assert_eq!(w.ok(&dir, &["show", ":README.md"]), "staged by the Lead\n");
}

#[test]
fn show_of_a_rev_and_path_prints_the_file_of_that_commit() {
    let mut w = fx();
    let dir = one(&mut w);
    commit_file(&w, &dir, "README.md", "two\n", "second");
    assert_eq!(w.ok(&dir, &["show", "HEAD:README.md"]), "two\n");
    assert_eq!(w.ok(&dir, &["show", "HEAD~1:README.md"]), "one\n");
}

#[test]
fn show_of_a_path_that_is_not_in_the_commit_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["show", "HEAD:nothing"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: path 'nothing' does not exist in 'HEAD'\n")
    );
}

#[test]
fn rev_parse_knows_head_tilde_n() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    commit_file(&w, &dir, "b.txt", "b\n", "third");
    assert_eq!(sha(&w, &dir, "HEAD~2"), first);
    assert_eq!(sha(&w, &dir, "HEAD~0"), sha(&w, &dir, "HEAD"));
    let out = w.git(&dir, &["rev-parse", "--verify", "HEAD~3"]);
    assert_eq!(out.code, 128);
}

#[test]
fn for_each_ref_with_a_format_lists_the_names_sorted() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["branch", "side"]);
    w.ok(&dir, &["branch", "att"]);
    assert_eq!(
        w.ok(&dir, &["for-each-ref", "--format=%(refname:short)"]),
        "att\nmain\nside\n"
    );
}

#[test]
fn for_each_ref_of_one_ref_with_the_object_name_format_prints_its_sha() {
    let mut w = fx();
    let dir = one(&mut w);
    let main = sha(&w, &dir, "main");
    assert_eq!(
        w.ok(
            &dir,
            &["for-each-ref", "--format=%(objectname)", "refs/heads/main"]
        ),
        format!("{main}\n")
    );
    let none = w.git(
        &dir,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/heads/nothing",
        ],
    );
    assert_eq!((none.code, none.stdout), (0, String::new()));
}

#[test]
fn for_each_ref_without_a_format_prints_sha_type_tab_and_ref() {
    let mut w = fx();
    let dir = one(&mut w);
    let main = sha(&w, &dir, "main");
    assert_eq!(
        w.ok(&dir, &["for-each-ref"]),
        format!("{main} commit\trefs/heads/main\n")
    );
}

#[test]
fn for_each_ref_of_a_prefix_lists_what_is_under_it() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["branch", "cpd/pay-1"]);
    w.ok(&dir, &["branch", "cpd/pay-2"]);
    w.ok(&dir, &["tag", "v1"]);
    assert_eq!(
        w.ok(
            &dir,
            &["for-each-ref", "--format=%(refname)", "refs/heads/cpd/"]
        ),
        "refs/heads/cpd/pay-1\nrefs/heads/cpd/pay-2\n"
    );
    assert_eq!(
        w.ok(&dir, &["for-each-ref", "--format=%(refname)", "refs/tags"]),
        "refs/tags/v1\n"
    );
}

// ---- branch (writing and listing forms), tag --------------------------------------------------------------

#[test]
fn branch_makes_a_branch_at_head_without_moving_head() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["branch", "att"]);
    assert_eq!(sha(&w, &dir, "att"), sha(&w, &dir, "main"));
    assert_eq!(w.ok(&dir, &["branch", "--show-current"]), "main\n");
}

#[test]
fn branch_with_a_start_point_makes_it_there() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    w.ok(&dir, &["branch", "old", &first]);
    assert_eq!(sha(&w, &dir, "old"), first);
}

#[test]
fn branch_of_a_name_that_exists_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["branch", "att"]);
    let out = w.git(&dir, &["branch", "att"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: a branch named 'att' already exists\n")
    );
}

#[test]
fn branch_from_a_start_point_that_is_no_object_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["branch", "x", "nothing"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: not a valid object name: 'nothing'\n")
    );
}

#[test]
fn branch_list_marks_the_current_branch_and_sorts() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["branch", "att"]);
    assert_eq!(w.ok(&dir, &["branch", "--list"]), "  att\n* main\n");
}

#[test]
fn branch_list_with_a_pattern_keeps_the_branches_it_matches() {
    let mut w = fx();
    let dir = one(&mut w);
    for name in ["cpd/pay-1", "cpd/pay-2", "att"] {
        w.ok(&dir, &["branch", name]);
    }
    assert_eq!(
        w.ok(&dir, &["branch", "--list", "cpd/pay-1*"]),
        "  cpd/pay-1\n"
    );
    assert_eq!(
        w.ok(&dir, &["branch", "--list", "cpd/*"]),
        "  cpd/pay-1\n  cpd/pay-2\n"
    );
    assert_eq!(w.ok(&dir, &["branch", "--list", "zz"]), "");
}

#[test]
fn branch_show_current_is_empty_on_a_detached_head() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["checkout", "-q", "--detach"]);
    let out = w.git(&dir, &["branch", "--show-current"]);
    assert_eq!((out.code, out.stdout), (0, String::new()));
}

#[test]
fn branch_format_prints_the_short_name_and_the_object_name() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["branch", "att"]);
    let main = sha(&w, &dir, "main");
    assert_eq!(
        w.ok(&dir, &["branch", "--format=%(refname:short) %(objectname)"]),
        format!("att {main}\nmain {main}\n")
    );
}

#[test]
fn branch_of_an_unborn_head_has_no_commit_to_start_from() {
    let mut w = fx();
    let dir = w.fresh("new");
    w.ok(&w.root(), &["init", "-q", "-b", "main", p(&dir)]);
    let out = w.git(&dir, &["branch", "x"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: not a valid object name: 'main'\n")
    );
}

#[test]
fn a_lightweight_tag_names_the_commit_it_was_made_at() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    w.ok(&dir, &["tag", "v1"]);
    w.ok(&dir, &["tag", "v0", &first]);
    assert_eq!(
        (sha(&w, &dir, "v1"), sha(&w, &dir, "v0")),
        (sha(&w, &dir, "HEAD"), first)
    );
}

#[test]
fn tag_list_lists_the_tags_sorted_and_filters_by_a_pattern() {
    let mut w = fx();
    let dir = one(&mut w);
    for tag in ["v2", "v1", "w1"] {
        w.ok(&dir, &["tag", tag]);
    }
    assert_eq!(w.ok(&dir, &["tag", "--list"]), "v1\nv2\nw1\n");
    assert_eq!(w.ok(&dir, &["tag", "--list", "v1"]), "v1\n");
    assert_eq!(w.ok(&dir, &["tag", "--list", "v*"]), "v1\nv2\n");
    assert_eq!(w.ok(&dir, &["tag", "--list", "v0.99.9"]), "");
}

#[test]
fn tag_dash_d_deletes_the_tag_and_says_so() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["tag", "v1"]);
    let head = short(sha(&w, &dir, "HEAD"));
    let out = w.git(&dir, &["tag", "-d", "v1"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, format!("Deleted tag 'v1' (was {head})\n").as_str()),
        "{}",
        out.stderr
    );
    assert_eq!(w.ok(&dir, &["tag", "--list"]), "");
    assert_eq!(w.git(&dir, &["tag", "-d", "v1"]).code, 1);
}

#[test]
fn tag_of_a_name_that_exists_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["tag", "v1"]);
    let out = w.git(&dir, &["tag", "v1"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: tag 'v1' already exists\n")
    );
}

#[test]
fn an_annotated_tag_peels_to_its_commit() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    w.ok(&dir, &["tag", "-a", "v1", &first, "-m", "v1"]);
    assert_eq!(sha(&w, &dir, "v1^{commit}"), first);
    assert_ne!(
        sha(&w, &dir, "v1"),
        first,
        "the tag object is not the commit"
    );
}

// ---- switch, checkout ---------------------------------------------------------------------------------

#[test]
fn switch_moves_head_to_the_branch_and_rewrites_the_working_tree() {
    let mut w = fx();
    let dir = forked(&mut w);
    let out = w.git(&dir, &["switch", "-q", "feat"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(w.ok(&dir, &["branch", "--show-current"]), "feat\n");
    assert_eq!(
        (w.read(&dir, "feat.txt"), w.read(&dir, "main.txt")),
        (Some("feat\n".to_owned()), None)
    );
    w.ok(&dir, &["switch", "-q", "main"]);
    assert_eq!(
        (w.read(&dir, "feat.txt"), w.read(&dir, "main.txt")),
        (None, Some("main\n".to_owned()))
    );
}

#[test]
fn switch_says_what_it_did_without_dash_q() {
    let mut w = fx();
    let dir = forked(&mut w);
    let out = w.git(&dir, &["switch", "feat"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "Switched to branch 'feat'\n")
    );
}

#[test]
fn switch_dash_c_makes_the_branch_at_head_or_at_a_start_point_and_moves_to_it() {
    let mut w = fx();
    let dir = forked(&mut w);
    let first = sha(&w, &dir, "feat^{commit}");
    let out = w.git(&dir, &["switch", "-c", "new"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "Switched to a new branch 'new'\n")
    );
    assert_eq!(sha(&w, &dir, "new"), sha(&w, &dir, "main"));
    w.ok(&dir, &["switch", "-q", "-c", "from-feat", "feat"]);
    assert_eq!(
        (sha(&w, &dir, "from-feat"), first),
        (sha(&w, &dir, "feat"), sha(&w, &dir, "feat"))
    );
    assert_eq!(w.ok(&dir, &["branch", "--show-current"]), "from-feat\n");
    assert_eq!(w.read(&dir, "feat.txt").as_deref(), Some("feat\n"));
}

#[test]
fn switch_to_an_unknown_branch_is_fatal_and_stays_put() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["switch", "nope"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: invalid reference: nope\n")
    );
    assert_eq!(w.ok(&dir, &["branch", "--show-current"]), "main\n");
}

#[test]
fn switch_detach_leaves_the_branch_and_checks_out_its_commit() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["switch", "-q", "--detach", "feat"]);
    assert_eq!(w.git(&dir, &["symbolic-ref", "-q", "HEAD"]).code, 1);
    assert_eq!(sha(&w, &dir, "HEAD"), sha(&w, &dir, "feat"));
    assert_eq!(w.read(&dir, "feat.txt").as_deref(), Some("feat\n"));
}

#[test]
fn switch_capital_c_resets_the_branch_to_the_start_point() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["switch", "-q", "-C", "main", "feat"]);
    assert_eq!(sha(&w, &dir, "main"), sha(&w, &dir, "feat"));
    assert_eq!(w.ok(&dir, &["branch", "--show-current"]), "main\n");
    assert_eq!(
        (
            w.read(&dir, "feat.txt").as_deref(),
            w.read(&dir, "main.txt")
        ),
        (Some("feat\n"), None)
    );
}

#[test]
fn checkout_dash_b_makes_the_branch_at_head_and_moves_to_it() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["checkout", "-b", "side"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "Switched to a new branch 'side'\n")
    );
    assert_eq!(w.ok(&dir, &["branch", "--show-current"]), "side\n");
    assert_eq!(sha(&w, &dir, "side"), sha(&w, &dir, "main"));
}

#[test]
fn checkout_dash_b_with_a_start_point_starts_there() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["checkout", "-q", "-b", "other", "feat"]);
    assert_eq!(sha(&w, &dir, "other"), sha(&w, &dir, "feat"));
    assert_eq!(w.read(&dir, "feat.txt").as_deref(), Some("feat\n"));
}

#[test]
fn checkout_of_a_branch_rewrites_the_working_tree() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["checkout", "-q", "feat"]);
    assert_eq!(
        (
            w.read(&dir, "feat.txt").as_deref(),
            w.read(&dir, "main.txt")
        ),
        (Some("feat\n"), None)
    );
    assert_eq!(status(&w, &dir), "");
}

#[test]
fn checkout_detach_without_a_rev_detaches_at_head() {
    let mut w = fx();
    let dir = one(&mut w);
    let head = sha(&w, &dir, "HEAD");
    w.ok(&dir, &["checkout", "-q", "--detach"]);
    assert_eq!(w.git(&dir, &["symbolic-ref", "-q", "HEAD"]).code, 1);
    assert_eq!(sha(&w, &dir, "HEAD"), head);
    let out = w.git(&dir, &["checkout", "--detach", "main"]);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr),
        (0, "", format!("HEAD is now at {} base\n", short(&head)))
    );
}

#[test]
fn checkout_of_an_unknown_name_exits_1() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["checkout", "-q", "nope"]);
    assert_eq!(
        (out.code, first(&out.stderr)),
        (
            1,
            "error: pathspec 'nope' did not match any file(s) known to git"
        )
    );
}

#[test]
fn checkout_of_a_branch_only_the_remote_has_makes_a_tracking_branch() {
    let mut w = fx();
    let (origin, clone) = clone_with_a_remote_branch(&mut w);
    let _ = origin;
    w.ok(&clone, &["checkout", "-q", "cpd/pay"]);
    assert_eq!(w.ok(&clone, &["branch", "--show-current"]), "cpd/pay\n");
    assert_eq!(
        w.ok(&clone, &["config", "--get", "branch.cpd/pay.remote"]),
        "origin\n"
    );
    assert_eq!(
        w.ok(&clone, &["config", "--get", "branch.cpd/pay.merge"]),
        "refs/heads/cpd/pay\n"
    );
    assert_eq!(w.read(&clone, "pay.txt").as_deref(), Some("pay\n"));
}

#[test]
fn checkout_force_capital_b_resets_the_branch_to_the_remote_tracking_branch() {
    let mut w = fx();
    let (_, clone) = clone_with_a_remote_branch(&mut w);
    w.write(&clone, "README.md", "local edit\n");
    w.ok(
        &clone,
        &["checkout", "--force", "-B", "main", "origin/main"],
    );
    assert_eq!(sha(&w, &clone, "main"), sha(&w, &clone, "origin/main"));
    assert_eq!(w.read(&clone, "README.md").as_deref(), Some("one\n"));
    assert_eq!(w.ok(&clone, &["branch", "--show-current"]), "main\n");
}

/// A bare origin on `main` (`README.md`) and `cpd/pay` (`pay.txt`), and a clone of it (made by `git clone`).
fn clone_with_a_remote_branch(w: &mut World) -> (PathBuf, PathBuf) {
    let origin = w.fresh("origin.git");
    w.ok(
        &w.root(),
        &["init", "-q", "--bare", "-b", "main", p(&origin)],
    );
    let work = one(w);
    w.ok(&work, &["remote", "add", "origin", p(&origin)]);
    w.ok(&work, &["checkout", "-q", "-b", "cpd/pay"]);
    commit_file(w, &work, "pay.txt", "pay\n", "pay");
    w.ok(&work, &["push", "-q", "origin", "main", "cpd/pay"]);
    let clone = w.fresh("clone");
    w.ok(&w.root(), &["clone", "-q", p(&origin), p(&clone)]);
    (origin, clone)
}

// ---- clone --------------------------------------------------------------------------------------------

#[test]
fn clone_of_a_bare_origin_checks_out_its_head_branch_with_the_files_on_disk() {
    let mut w = fx();
    let (origin, clone) = clone_with_a_remote_branch(&mut w);
    assert_eq!(w.ok(&clone, &["branch", "--show-current"]), "main\n");
    assert_eq!(w.read(&clone, "README.md").as_deref(), Some("one\n"));
    assert_eq!(
        w.read(&clone, "pay.txt"),
        None,
        "only the checked out branch is on disk"
    );
    assert_eq!(status(&w, &clone), "");
    assert_eq!(
        w.ok(&clone, &["config", "--get", "remote.origin.url"]),
        format!("{}\n", origin.display())
    );
    assert_eq!(
        w.ok(&clone, &["remote", "get-url", "origin"]),
        format!("{}\n", origin.display())
    );
}

#[test]
fn clone_makes_the_remote_tracking_branches_and_the_origin_head() {
    let mut w = fx();
    let (origin, clone) = clone_with_a_remote_branch(&mut w);
    assert_eq!(
        w.ok(
            &clone,
            &["for-each-ref", "--format=%(refname)", "refs/remotes"]
        ),
        "refs/remotes/origin/HEAD\nrefs/remotes/origin/cpd/pay\nrefs/remotes/origin/main\n"
    );
    assert_eq!(
        w.ok(
            &clone,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]
        ),
        "origin/main\n"
    );
    assert_eq!(
        sha(&w, &clone, "origin/main"),
        sha(&w, &origin, "refs/heads/main")
    );
    assert_eq!(
        w.ok(&clone, &["config", "--get", "remote.origin.fetch"]),
        "+refs/heads/*:refs/remotes/origin/*\n"
    );
}

#[test]
fn clone_of_a_bare_origin_lists_the_remote_branches_with_branch_dash_r() {
    let mut w = fx();
    let (_, clone) = clone_with_a_remote_branch(&mut w);
    assert_eq!(
        w.ok(&clone, &["branch", "-r"]),
        "  origin/HEAD -> origin/main\n  origin/cpd/pay\n  origin/main\n"
    );
    assert_eq!(
        w.ok(&clone, &["branch", "-r", "--list", "origin/cpd/*"]),
        "  origin/cpd/pay\n"
    );
    assert_eq!(branches(&w, &clone), ["main"]);
}

#[test]
fn clone_shares_the_objects_so_the_clone_can_read_the_history() {
    let mut w = fx();
    let (origin, clone) = clone_with_a_remote_branch(&mut w);
    let tip = sha(&w, &origin, "refs/heads/cpd/pay");
    assert_eq!(
        w.git(&clone, &["cat-file", "-e", &format!("{tip}^{{commit}}")])
            .code,
        0
    );
    assert_eq!(
        w.ok(&clone, &["rev-list", "--count", "origin/cpd/pay"]),
        "2\n"
    );
}

#[test]
fn clone_without_dash_q_says_cloning_into_the_destination() {
    let mut w = fx();
    let origin = w.fresh("origin.git");
    w.ok(
        &w.root(),
        &["init", "-q", "--bare", "-b", "main", p(&origin)],
    );
    let dest = w.fresh("clone");
    let out = w.git(&w.root(), &["clone", p(&origin), p(&dest)]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        first(&out.stderr),
        format!("Cloning into '{}'...", dest.display())
    );
}

#[test]
fn clone_of_an_empty_bare_origin_is_an_unborn_branch_named_as_the_origin_head() {
    let mut w = fx();
    let origin = w.fresh("origin.git");
    w.ok(
        &w.root(),
        &["init", "-q", "--bare", "-b", "trunk", p(&origin)],
    );
    let dest = w.fresh("clone");
    w.ok(&w.root(), &["clone", "-q", p(&origin), p(&dest)]);
    assert_eq!(w.ok(&dest, &["symbolic-ref", "HEAD"]), "refs/heads/trunk\n");
    assert_eq!(
        w.git(&dest, &["rev-parse", "--verify", "--quiet", "HEAD"])
            .code,
        1
    );
    assert_eq!(w.ok(&dest, &["remote"]), "origin\n");
}

#[test]
fn clone_of_a_non_bare_repo_by_its_path_works_too() {
    let mut w = fx();
    let src = one(&mut w);
    let dest = w.fresh("clone");
    w.ok(&w.root(), &["clone", "-q", p(&src), p(&dest)]);
    assert_eq!(w.read(&dest, "README.md").as_deref(), Some("one\n"));
    assert_eq!(sha(&w, &dest, "HEAD"), sha(&w, &src, "HEAD"));
}

#[test]
fn clone_of_a_path_that_is_no_repo_is_fatal() {
    let mut w = fx();
    let nothing = w.fresh("nothing.git");
    let dest = w.fresh("clone");
    let out = w.git(&w.root(), &["clone", p(&nothing), p(&dest)]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (
            128,
            format!("fatal: repository '{}' does not exist\n", nothing.display()).as_str()
        )
    );
}

#[test]
fn clone_into_a_directory_that_has_a_repo_is_fatal() {
    let mut w = fx();
    let (origin, clone) = clone_with_a_remote_branch(&mut w);
    let out = w.git(&w.root(), &["clone", "-q", p(&origin), p(&clone)]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (
            128,
            format!(
                "fatal: destination path '{}' already exists and is not an empty directory.\n",
                clone.display()
            )
            .as_str()
        )
    );
}

// ---- reset --------------------------------------------------------------------------------------------

#[test]
fn reset_hard_head_tilde_1_moves_the_branch_and_the_files_back() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    commit_file(&w, &dir, "README.md", "two\n", "third");
    w.ok(&dir, &["reset", "-q", "--hard", "HEAD~1"]);
    assert_eq!(w.ok(&dir, &["rev-list", "--count", "HEAD"]), "2\n");
    assert_eq!(
        (
            w.read(&dir, "a.txt").as_deref(),
            w.read(&dir, "README.md").as_deref()
        ),
        (Some("a\n"), Some("one\n"))
    );
    w.ok(&dir, &["reset", "-q", "--hard", "HEAD~1"]);
    assert_eq!(sha(&w, &dir, "main"), first);
    assert_eq!(w.read(&dir, "a.txt"), None);
    assert_eq!(status(&w, &dir), "");
}

#[test]
fn reset_hard_to_a_sha_moves_the_branch_there() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    w.ok(&dir, &["reset", "-q", "--hard", &first]);
    assert_eq!(sha(&w, &dir, "main"), first);
    assert_eq!(w.read(&dir, "a.txt"), None);
}

#[test]
fn reset_hard_says_where_head_is_now_without_dash_q() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    let out = w.git(&dir, &["reset", "--hard", &first]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            format!("HEAD is now at {} base\n", short(&first)).as_str()
        ),
        "{}",
        out.stderr
    );
}

#[test]
fn reset_hard_alone_throws_away_staged_and_working_changes_and_keeps_untracked_files() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "README.md", "edited\n");
    w.write(&dir, "staged.txt", "s\n");
    w.ok(&dir, &["add", "staged.txt"]);
    w.write(&dir, "untracked.txt", "u\n");
    w.ok(&dir, &["reset", "-q", "--hard"]);
    assert_eq!(w.read(&dir, "README.md").as_deref(), Some("one\n"));
    assert_eq!(w.read(&dir, "staged.txt"), None);
    assert_eq!(status(&w, &dir), "?? untracked.txt\n");
}

#[test]
fn reset_hard_on_a_detached_head_moves_only_head() {
    let mut w = fx();
    let dir = one(&mut w);
    let first = sha(&w, &dir, "HEAD");
    commit_file(&w, &dir, "a.txt", "a\n", "second");
    let second = sha(&w, &dir, "HEAD");
    w.ok(&dir, &["checkout", "-q", "--detach"]);
    w.ok(&dir, &["reset", "-q", "--hard", &first]);
    assert_eq!(
        (sha(&w, &dir, "HEAD"), sha(&w, &dir, "main")),
        (first, second)
    );
}

#[test]
fn clean_fd_removes_untracked_files_and_directories_and_keeps_ignored_and_tracked_files() {
    let mut w = fx();
    let dir = made(&mut w, &[("f.txt", "f\n"), (".gitignore", "ignored.log\n")]);
    w.write(&dir, "new.rs", "n\n");
    w.write(&dir, "newdir/x.txt", "x\n");
    w.write(&dir, "ignored.log", "i\n");
    for flags in [&["-fd"][..], &["-df"], &["-f", "-d"]] {
        w.write(&dir, "new.rs", "n\n");
        w.write(&dir, "newdir/x.txt", "x\n");
        let mut args = vec!["clean"];
        args.extend_from_slice(flags);
        assert_eq!(w.ok(&dir, &args), "Removing new.rs\nRemoving newdir/\n");
        assert_eq!(
            (
                w.read(&dir, "new.rs"),
                w.read(&dir, "newdir/x.txt"),
                w.read(&dir, "ignored.log").as_deref(),
                w.read(&dir, "f.txt").as_deref()
            ),
            (None, None, Some("i\n"), Some("f\n"))
        );
        assert_eq!(status(&w, &dir), "");
    }
}

#[test]
fn clean_f_without_d_keeps_an_untracked_directory() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "new.rs", "n\n");
    w.write(&dir, "newdir/x.txt", "x\n");
    assert_eq!(w.ok(&dir, &["clean", "-f"]), "Removing new.rs\n");
    assert_eq!(w.read(&dir, "newdir/x.txt").as_deref(), Some("x\n"));
}

#[test]
fn clean_without_f_is_refused() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "new.rs", "n\n");
    let out = w.git(&dir, &["clean", "-d"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (
            128,
            "fatal: clean.requireForce defaults to true and neither -i, -n, nor -f given; refusing to clean\n"
        )
    );
    assert_eq!(w.read(&dir, "new.rs").as_deref(), Some("n\n"));
}

// ---- merge --------------------------------------------------------------------------------------------

#[test]
fn merge_dash_dash_ff_only_moves_the_branch_to_a_commit_that_descends_from_it() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["checkout", "-q", "-b", "side"]);
    commit_file(&w, &dir, "a.txt", "a\n", "side");
    w.ok(&dir, &["checkout", "-q", "main"]);
    w.ok(&dir, &["merge", "-q", "--ff-only", "side"]);
    assert_eq!(sha(&w, &dir, "main"), sha(&w, &dir, "side"));
    assert_eq!(w.read(&dir, "a.txt").as_deref(), Some("a\n"));
}

#[test]
fn merge_dash_dash_ff_only_of_a_diverged_branch_is_fatal() {
    let mut w = fx();
    let dir = forked(&mut w);
    let before = sha(&w, &dir, "main");
    let out = w.git(&dir, &["merge", "--ff-only", "feat"]);
    assert_eq!(out.code, 128);
    assert_eq!(
        out.stderr.lines().last(),
        Some("fatal: Not possible to fast-forward, aborting.")
    );
    assert_eq!(sha(&w, &dir, "main"), before);
}

#[test]
fn merge_no_ff_with_a_message_makes_a_merge_commit_of_both_tips() {
    let mut w = fx();
    let dir = forked(&mut w);
    let (main, feat) = (sha(&w, &dir, "main"), sha(&w, &dir, "feat"));
    let out = w.git(&dir, &["merge", "--no-ff", "-m", "merge feat", "feat"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(first(&out.stdout), "Merge made by the 'ort' strategy.");
    let merge = w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD")));
    assert_eq!(
        (merge.parents, merge.message.as_str()),
        (vec![Oid::new(&main), Oid::new(&feat)], "merge feat")
    );
    assert_eq!(
        w.paths_at(&dir, "HEAD"),
        ["README.md", "feat.txt", "main.txt"]
    );
    assert_eq!(
        (
            w.read(&dir, "feat.txt").as_deref(),
            w.read(&dir, "main.txt").as_deref()
        ),
        (Some("feat\n"), Some("main\n"))
    );
}

#[test]
fn merge_no_ff_of_a_branch_that_is_ahead_still_makes_a_merge_commit() {
    let mut w = fx();
    let dir = one(&mut w);
    let base = sha(&w, &dir, "HEAD");
    w.ok(&dir, &["checkout", "-q", "-b", "side"]);
    commit_file(&w, &dir, "a.txt", "a\n", "side");
    w.ok(&dir, &["checkout", "-q", "main"]);
    w.ok(
        &dir,
        &["merge", "-q", "--no-ff", "-m", "merge the lead", "side"],
    );
    let merge = w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD")));
    assert_eq!(
        merge.parents,
        vec![Oid::new(&base), Oid::new(&sha(&w, &dir, "side"))]
    );
}

#[test]
fn merge_no_edit_names_both_branches_in_the_message() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["switch", "-q", "-c", "side"]);
    w.ok(&dir, &["merge", "-q", "--no-edit", "feat"]);
    assert_eq!(
        w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD"))).message,
        "Merge branch 'feat' into side"
    );
}

#[test]
fn merge_of_a_branch_that_is_already_in_says_already_up_to_date() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["merge", "-q", "--no-edit", "feat"]);
    let before = sha(&w, &dir, "HEAD");
    let out = w.git(&dir, &["merge", "feat"]);
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "Already up to date.\n")
    );
    assert_eq!(sha(&w, &dir, "HEAD"), before);
}

#[test]
fn merge_of_an_unknown_name_exits_1() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["merge", "nope"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "merge: nope - not something we can merge\n")
    );
}

#[test]
fn merge_of_a_content_conflict_exits_1_and_leaves_the_markers_and_the_stages() {
    let mut w = fx();
    let dir = clashing(&mut w);
    let out = w.git(&dir, &["merge", "--no-edit", "feat"]);
    assert_eq!(out.code, 1, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "Auto-merging README.md\nCONFLICT (content): Merge conflict in README.md\nAutomatic merge failed; fix conflicts and then commit the result.\n"
    );
    assert_eq!(status(&w, &dir), "UU README.md\n");
    assert_eq!(
        w.read(&dir, "README.md").as_deref(),
        Some("<<<<<<< HEAD\nmain\n=======\nfeat\n>>>>>>> feat\n")
    );
}

#[test]
fn merge_of_an_add_add_conflict_is_aa() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["checkout", "-q", "-b", "cA"]);
    commit_file(&w, &dir, "c.txt", "A\n", "A");
    w.ok(&dir, &["checkout", "-q", "main"]);
    commit_file(&w, &dir, "c.txt", "B\n", "B");
    let out = w.git(&dir, &["merge", "--no-edit", "cA"]);
    assert_eq!(
        (out.code, out.stdout.lines().nth(1)),
        (1, Some("CONFLICT (add/add): Merge conflict in c.txt")),
        "{}",
        out.stderr
    );
    assert_eq!(status(&w, &dir), "AA c.txt\n");
}

#[test]
fn merge_dash_x_ours_takes_our_side_of_a_conflict_and_commits() {
    let mut w = fx();
    let dir = clashing(&mut w);
    let (main, feat) = (sha(&w, &dir, "main"), sha(&w, &dir, "feat"));
    w.ok(&dir, &["merge", "-q", "--no-edit", "-X", "ours", "feat"]);
    assert_eq!(w.read(&dir, "README.md").as_deref(), Some("main\n"));
    assert_eq!(status(&w, &dir), "");
    assert_eq!(
        w.commit_of(&Oid::new(&sha(&w, &dir, "HEAD"))).parents,
        vec![Oid::new(&main), Oid::new(&feat)]
    );
}

#[test]
fn merge_dash_x_ours_keeps_the_changes_that_do_not_conflict() {
    let mut w = fx();
    let dir = clashing(&mut w);
    w.ok(&dir, &["checkout", "-q", "feat"]);
    commit_file(&w, &dir, "extra.txt", "extra\n", "extra");
    w.ok(&dir, &["checkout", "-q", "main"]);
    w.ok(&dir, &["merge", "-q", "--no-edit", "-X", "ours", "feat"]);
    assert_eq!(
        (
            w.read(&dir, "README.md").as_deref(),
            w.read(&dir, "extra.txt").as_deref()
        ),
        (Some("main\n"), Some("extra\n"))
    );
}

#[test]
fn merge_of_a_remote_tracking_branch_merges_what_a_fetch_brought() {
    let mut w = fx();
    let (origin, clone) = clone_with_a_remote_branch(&mut w);
    let other = w.fresh("other");
    w.ok(&w.root(), &["clone", "-q", p(&origin), p(&other)]);
    commit_file(&w, &other, "o.txt", "o\n", "other");
    w.ok(&other, &["push", "-q", "origin", "main"]);
    commit_file(&w, &clone, "c.txt", "c\n", "mine");
    w.ok(&clone, &["fetch", "-q", "origin", "main"]);
    w.ok(
        &clone,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge remote-tracking branch 'origin/main'",
            "origin/main",
        ],
    );
    assert_eq!(w.paths_at(&clone, "HEAD"), ["README.md", "c.txt", "o.txt"]);
}

// ---- mv, rm -------------------------------------------------------------------------------------------

#[test]
fn mv_moves_the_file_on_disk_and_in_the_index() {
    let mut w = fx();
    let dir = made(&mut w, &[("verify.sh", "v\n"), ("keep.txt", "k\n")]);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    w.ok(&dir, &["mv", "verify.sh", "src/not-protected.sh"]);
    assert_eq!(
        (
            w.read(&dir, "verify.sh"),
            w.read(&dir, "src/not-protected.sh").as_deref()
        ),
        (None, Some("v\n"))
    );
    w.ok(&dir, &["commit", "-q", "-m", "rename away"]);
    assert_eq!(
        w.paths_at(&dir, "HEAD"),
        ["keep.txt", "src/not-protected.sh"]
    );
    assert_eq!(status(&w, &dir), "");
}

#[test]
fn mv_of_a_file_that_is_not_there_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["mv", "nope", "x"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: bad source, source=nope, destination=x\n")
    );
}

#[test]
fn rm_dash_q_removes_the_file_and_stages_the_deletion() {
    let mut w = fx();
    let dir = made(&mut w, &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let out = w.git(&dir, &["rm", "-q", "a.txt"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(w.read(&dir, "a.txt"), None);
    assert_eq!(status(&w, &dir), "D  a.txt\n");
}

#[test]
fn rm_says_which_file_it_removed() {
    let mut w = fx();
    let dir = made(&mut w, &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    assert_eq!(w.ok(&dir, &["rm", "a.txt"]), "rm 'a.txt'\n");
}

#[test]
fn rm_of_a_path_git_does_not_know_is_fatal() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["rm", "-q", "nope"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (128, "fatal: pathspec 'nope' did not match any files\n")
    );
}

#[test]
fn rm_of_a_file_with_changes_staged_in_the_index_exits_1_and_keeps_it() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "README.md", "two\n");
    w.ok(&dir, &["add", "README.md"]);
    let out = w.git(&dir, &["rm", "-q", "README.md"]);
    assert_eq!(
        (out.code, first(&out.stderr)),
        (
            1,
            "error: the following file has changes staged in the index:"
        )
    );
    assert_eq!(w.read(&dir, "README.md").as_deref(), Some("two\n"));
}

// ---- diff-index, diff-files, update-index ------------------------------------------------------------------

#[test]
fn diff_index_quiet_head_is_0_on_a_clean_tree_and_1_when_the_tree_differs_from_head() {
    let mut w = fx();
    let dir = one(&mut w);
    assert_eq!(
        w.git(&dir, &["diff-index", "--quiet", "HEAD", "--"]).code,
        0
    );
    w.write(&dir, "README.md", "two\n");
    assert_eq!(
        w.git(&dir, &["diff-index", "--quiet", "HEAD", "--"]).code,
        1
    );
    w.ok(&dir, &["add", "README.md"]);
    assert_eq!(
        w.git(&dir, &["diff-index", "--quiet", "HEAD", "--"]).code,
        1,
        "a staged change differs from HEAD"
    );
    w.ok(&dir, &["commit", "-q", "-m", "two"]);
    assert_eq!(
        w.git(&dir, &["diff-index", "--quiet", "HEAD", "--"]).code,
        0
    );
}

#[test]
fn diff_files_quiet_is_1_only_while_the_working_tree_differs_from_the_index() {
    let mut w = fx();
    let dir = one(&mut w);
    assert_eq!(w.git(&dir, &["diff-files", "--quiet"]).code, 0);
    w.write(&dir, "README.md", "two\n");
    assert_eq!(w.git(&dir, &["diff-files", "--quiet"]).code, 1);
    w.ok(&dir, &["add", "README.md"]);
    assert_eq!(w.git(&dir, &["diff-files", "--quiet"]).code, 0);
}

#[test]
fn diff_files_quiet_does_not_see_an_untracked_file_and_sees_a_deleted_one() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "new.txt", "n\n");
    assert_eq!(w.git(&dir, &["diff-files", "--quiet"]).code, 0);
    std::fs::remove_file(dir.join("README.md")).unwrap();
    assert_eq!(w.git(&dir, &["diff-files", "--quiet"]).code, 1);
}

#[test]
fn update_index_dash_q_refresh_exits_0_and_changes_nothing_a_status_sees() {
    let mut w = fx();
    let dir = one(&mut w);
    w.write(&dir, "README.md", "two\n");
    let before = status(&w, &dir);
    let out = w.git(&dir, &["update-index", "-q", "--refresh"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(status(&w, &dir), before);
}

// ---- config, remote (writing forms) ----------------------------------------------------------------------

#[test]
fn config_with_a_key_and_a_value_sets_it_for_the_repo() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["config", "key.name", "value"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(w.ok(&dir, &["config", "--get", "key.name"]), "value\n");
}

#[test]
fn config_sets_a_key_again_by_replacing_its_value() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["config", "key.name", "a"]);
    w.ok(&dir, &["config", "key.name", "b"]);
    assert_eq!(w.ok(&dir, &["config", "--get", "key.name"]), "b\n");
    assert_eq!(
        w.ok(&dir, &["config", "--get-regexp", "^key\\.name$"]),
        "key.name b\n"
    );
}

#[test]
fn config_of_one_repo_is_not_seen_by_another() {
    let mut w = fx();
    let (a, b) = (one(&mut w), one(&mut w));
    w.ok(&a, &["config", "key.name", "value"]);
    assert_eq!(w.git(&b, &["config", "--get", "key.name"]).code, 1);
}

#[test]
fn config_global_get_reads_the_global_config_of_the_test_kit() {
    let mut w = fx();
    let dir = one(&mut w);
    assert_eq!(
        w.ok(&dir, &["config", "--global", "--get", "user.name"]),
        "cpd test\n"
    );
    assert_eq!(
        w.git(&dir, &["config", "--global", "--get", "nothing.here"])
            .code,
        1
    );
}

#[test]
fn config_get_of_a_key_that_is_not_set_exits_1() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["config", "--get", "nothing.here"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (1, String::new(), String::new())
    );
}

#[test]
fn remote_add_makes_the_remote_with_the_default_fetch_refspec() {
    let mut w = fx();
    let dir = one(&mut w);
    let out = w.git(&dir, &["remote", "add", "origin", "https://example.com/x"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(w.ok(&dir, &["remote"]), "origin\n");
    assert_eq!(
        w.ok(&dir, &["remote", "get-url", "origin"]),
        "https://example.com/x\n"
    );
    assert_eq!(
        w.ok(&dir, &["config", "--get", "remote.origin.url"]),
        "https://example.com/x\n"
    );
    assert_eq!(
        w.ok(&dir, &["config", "--get", "remote.origin.fetch"]),
        "+refs/heads/*:refs/remotes/origin/*\n"
    );
}

#[test]
fn remote_add_of_a_name_that_exists_exits_3() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["remote", "add", "origin", "https://example.com/x"]);
    let out = w.git(&dir, &["remote", "add", "origin", "https://z"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (3, "error: remote origin already exists.\n")
    );
    assert_eq!(
        w.ok(&dir, &["remote", "get-url", "origin"]),
        "https://example.com/x\n"
    );
}

#[test]
fn remote_set_url_changes_the_url() {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(&dir, &["remote", "add", "origin", "https://example.com/x"]);
    w.ok(
        &dir,
        &["remote", "set-url", "origin", "https://example.com/y"],
    );
    assert_eq!(
        w.ok(&dir, &["config", "--get", "remote.origin.url"]),
        "https://example.com/y\n"
    );
}

#[test]
fn remote_remove_drops_the_remote_its_config_and_its_tracking_branches() {
    let mut w = fx();
    let (_, clone) = clone_with_a_remote_branch(&mut w);
    let out = w.git(&clone, &["remote", "remove", "origin"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(w.ok(&clone, &["remote"]), "");
    assert_eq!(
        w.git(&clone, &["config", "--get", "remote.origin.url"])
            .code,
        1
    );
    assert_eq!(
        w.ok(
            &clone,
            &["for-each-ref", "--format=%(refname)", "refs/remotes"]
        ),
        ""
    );
}

#[test]
fn remote_remove_of_a_remote_that_is_not_there_exits_2() {
    let mut w = fx();
    let dir = one(&mut w);
    for sub in ["remove", "rm"] {
        let out = w.git(&dir, &["remote", sub, "origin"]);
        assert_eq!(
            (out.code, out.stderr.as_str()),
            (2, "error: No such remote: 'origin'\n")
        );
    }
}

#[test]
fn config_url_insteadof_makes_a_github_url_reach_a_local_bare_repo() {
    let mut w = fx();
    let dir = one(&mut w);
    let bare = w.fresh("origin.git");
    w.ok(&w.root(), &["init", "-q", "--bare", "-b", "main", p(&bare)]);
    let url = "https://github.com/acme/clone.git";
    w.ok(&dir, &["remote", "add", "origin", url]);
    w.ok(
        &dir,
        &["config", &format!("url.{}.insteadOf", bare.display()), url],
    );
    assert_eq!(
        w.ok(&dir, &["remote", "get-url", "origin"]),
        format!("{}\n", bare.display())
    );
    assert_eq!(
        w.ok(&dir, &["config", "--get", "remote.origin.url"]),
        format!("{url}\n"),
        "config shows the url as configured"
    );
    w.ok(&dir, &["push", "-q", "origin", "main"]);
    assert_eq!(sha(&w, &bare, "refs/heads/main"), sha(&w, &dir, "main"));
    assert_eq!(
        w.ok(&dir, &["config", "--get-regexp", r"^url\..*\.insteadof$"]),
        format!("url.{}.insteadof {url}\n", bare.display())
    );
}

#[test]
fn remote_remove_of_an_unreachable_origin_after_the_insteadof_is_looked_up_leaves_a_repo_with_no_remote()
 {
    let mut w = fx();
    let dir = one(&mut w);
    w.ok(
        &dir,
        &["remote", "add", "origin", "https://github.com/acme/x.git"],
    );
    assert_eq!(
        w.git(&dir, &["config", "--get-regexp", r"^url\..*\.insteadof$"])
            .code,
        1
    );
    w.ok(&dir, &["remote", "remove", "origin"]);
    assert_eq!(w.ok(&dir, &["remote"]), "");
}

// ---- push and fetch with -q (the tests' forms) ----------------------------------------------------------------

/// A repo with a remote `origin` that is an empty bare repo, made by `init --bare` and `remote add`.
fn with_a_bare_origin(w: &mut World) -> (PathBuf, PathBuf) {
    let dir = one(w);
    let origin = w.fresh("origin.git");
    w.ok(
        &w.root(),
        &["init", "-q", "--bare", "-b", "main", p(&origin)],
    );
    w.ok(&dir, &["remote", "add", "origin", p(&origin)]);
    (dir, origin)
}

#[test]
fn push_dash_q_prints_nothing_and_updates_the_remote_and_the_tracking_branch() {
    let mut w = fx();
    let (dir, origin) = with_a_bare_origin(&mut w);
    let out = w.git(&dir, &["push", "-q", "origin", "main"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(sha(&w, &origin, "refs/heads/main"), sha(&w, &dir, "main"));
    assert_eq!(
        sha(&w, &dir, "refs/remotes/origin/main"),
        sha(&w, &dir, "main")
    );
}

#[test]
fn push_dash_q_dash_u_sets_the_upstream_of_the_branch() {
    let mut w = fx();
    let (dir, _) = with_a_bare_origin(&mut w);
    w.ok(&dir, &["checkout", "-q", "-b", "cpd/pay"]);
    w.ok(&dir, &["push", "-q", "-u", "origin", "cpd/pay"]);
    assert_eq!(
        w.ok(&dir, &["config", "--get", "branch.cpd/pay.remote"]),
        "origin\n"
    );
    assert_eq!(
        w.ok(&dir, &["config", "--get", "branch.cpd/pay.merge"]),
        "refs/heads/cpd/pay\n"
    );
    assert_eq!(
        w.git(&dir, &["config", "--get", "branch.main.remote"]).code,
        1
    );
}

#[test]
fn push_dash_q_of_head_to_a_full_ref_and_of_two_branches() {
    let mut w = fx();
    let (dir, origin) = with_a_bare_origin(&mut w);
    w.ok(&dir, &["branch", "side"]);
    w.ok(
        &dir,
        &["push", "-q", "origin", "HEAD:refs/heads/cpd/pay/pay"],
    );
    w.ok(&dir, &["push", "-q", "origin", "main", "side"]);
    assert_eq!(
        w.ok(&origin, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/cpd/pay/pay\nrefs/heads/main\nrefs/heads/side\n"
    );
}

#[test]
fn push_dash_q_delete_removes_the_branch_of_the_remote() {
    let mut w = fx();
    let (dir, origin) = with_a_bare_origin(&mut w);
    w.ok(&dir, &["push", "-q", "origin", "main"]);
    w.ok(&dir, &["branch", "cpd/pay/a1"]);
    w.ok(&dir, &["push", "-q", "origin", "cpd/pay/a1"]);
    let out = w.git(&dir, &["push", "-q", "origin", "--delete", "cpd/pay/a1"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(
        w.ok(&origin, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main\n"
    );
}

#[test]
fn fetch_dash_q_origin_brings_every_branch_and_prints_nothing() {
    let mut w = fx();
    let (dir, origin) = with_a_bare_origin(&mut w);
    w.ok(&dir, &["push", "-q", "origin", "main"]);
    let other = w.fresh("other");
    w.ok(&w.root(), &["clone", "-q", p(&origin), p(&other)]);
    w.ok(&other, &["checkout", "-q", "-b", "extra"]);
    commit_file(&w, &other, "x.txt", "x\n", "extra");
    w.ok(&other, &["push", "-q", "origin", "extra"]);
    let out = w.git(&dir, &["fetch", "-q", "origin"]);
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (0, String::new(), String::new())
    );
    assert_eq!(sha(&w, &dir, "origin/extra"), sha(&w, &other, "extra"));
}

// ---- the Repo handle with the subcommands -----------------------------------------------------------------

#[test]
fn git_add_and_commit_make_the_tree_the_repo_handle_makes() {
    let mut w = fx();
    let handle = w.repo(&[("README.md", "one\n"), ("docs/a.md", "a\n")]);
    let by_git = made(&mut w, &[("README.md", "one\n"), ("docs/a.md", "a\n")]);
    assert_eq!(
        sha(&w, handle.dir(), "HEAD^{tree}"),
        sha(&w, &by_git, "HEAD^{tree}")
    );
}

#[test]
fn a_repo_handle_commit_is_visible_to_git_status_and_switch() {
    let mut w = fx();
    let repo = w.repo(&[("README.md", "one\n")]);
    repo.branch("att");
    repo.write("a.txt", "a\n");
    repo.commit("second");
    w.ok(repo.dir(), &["switch", "-q", "att"]);
    assert_eq!(w.read(repo.dir(), "a.txt"), None);
    w.ok(repo.dir(), &["switch", "-q", "main"]);
    assert_eq!(w.read(repo.dir(), "a.txt").as_deref(), Some("a\n"));
    assert_eq!(status(&w, repo.dir()), "");
}

#[test]
fn a_file_the_repo_handle_wrote_is_staged_by_git_add_and_committed_by_git_commit() {
    let mut w = fx();
    let repo = w.repo(&[("README.md", "one\n")]);
    repo.write("b.txt", "b\n");
    assert_eq!(status(&w, repo.dir()), "?? b.txt\n");
    w.ok(repo.dir(), &["add", "b.txt"]);
    w.ok(repo.dir(), &["commit", "-q", "-m", "b"]);
    assert_eq!(w.paths_at(repo.dir(), "HEAD"), ["README.md", "b.txt"]);
}

#[test]
fn a_clone_of_the_bare_origin_of_a_pushed_repo_has_its_files() {
    let mut w = fx();
    let repo = w.repo(&[("README.md", "one\n")]);
    let bare = w.fresh("origin.git");
    repo.bare_origin(&bare);
    repo.push();
    let clone = w.fresh("clone");
    w.ok(&w.root(), &["clone", "-q", p(&bare), p(&clone)]);
    assert_eq!(w.read(&clone, "README.md").as_deref(), Some("one\n"));
    assert_eq!(sha(&w, &clone, "HEAD"), sha(&w, repo.dir(), "HEAD"));
}

#[test]
fn a_keyed_repo_gets_its_origin_back_to_none_by_remote_remove() {
    let mut w = fx();
    let repo = w.repo(&[("README.md", "one\n")]);
    repo.keyed("pay");
    assert_eq!(
        w.ok(repo.dir(), &["config", "--get", "remote.origin.url"]),
        "https://github.com/acme/pay.git\n"
    );
    assert_eq!(
        w.ok(repo.dir(), &["rev-parse", "--verify", "origin/main"]),
        format!("{}\n", sha(&w, repo.dir(), "main"))
    );
    w.ok(repo.dir(), &["remote", "remove", "origin"]);
    assert_eq!(w.ok(repo.dir(), &["remote"]), "");
    assert_eq!(
        w.git(
            repo.dir(),
            &["rev-parse", "--verify", "--quiet", "origin/main"]
        )
        .code,
        1
    );
}

#[test]
fn a_keyed_repo_reaches_a_local_bare_origin_through_an_insteadof_the_test_writes() {
    let mut w = fx();
    let repo = w.repo(&[("README.md", "one\n")]);
    repo.keyed("pay");
    let bare = w.fresh("origin.git");
    w.ok(&w.root(), &["init", "-q", "--bare", "-b", "main", p(&bare)]);
    w.ok(
        repo.dir(),
        &[
            "config",
            &format!("url.{}.insteadOf", bare.display()),
            "https://github.com/acme/pay.git",
        ],
    );
    w.ok(repo.dir(), &["push", "-q", "origin", "main"]);
    assert_eq!(
        sha(&w, &bare, "refs/heads/main"),
        sha(&w, repo.dir(), "main")
    );
}

#[test]
fn the_fixture_subcommands_leave_nothing_unsupported_behind() {
    let mut w = fx();
    let dir = forked(&mut w);
    w.ok(&dir, &["switch", "-q", "feat"]);
    w.ok(&dir, &["tag", "v1"]);
    w.ok(&dir, &["mv", "feat.txt", "moved.txt"]);
    w.ok(&dir, &["rm", "-q", "README.md"]);
    w.ok(&dir, &["reset", "-q", "--hard"]);
    w.ok(&dir, &["branch", "--list"]);
    w.route.assert_supported();
    assert_eq!(w.route.unsupported(), Vec::<String>::new());
}
