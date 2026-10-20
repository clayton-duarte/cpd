//! The `git` wrapper that refuses `git stash` (contract §6.5, hs-d310).

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use cpd_core::home::CpdHome;
use cpd_core::host::git_guard::{self, NO_GIT, REFUSAL};

use crate::common;
use common::{output, poll, spawn, write_executable};

/// How long one call of the wrapper may take: a few `sh` builtins and one `exec`, milliseconds. A call that is not
/// over in this is wrappers that `exec` each other for ever (hs-d341), not a slow machine: the test fails at once
/// instead of at the suite's 90 s.
const BOUND: Duration = Duration::from_secs(10);

struct Wrapper {
    _tmp: tempfile::TempDir,
    home: CpdHome,
}

impl Wrapper {
    fn new() -> Wrapper {
        let tmp = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(tmp.path().join("cpd-home").into()), None).unwrap();
        // The harness writes what the tests execute (`install` is tested below, never executed).
        fs::create_dir_all(home.bin_dir()).unwrap();
        write_executable(&home.git_wrapper(), git_guard::WRAPPER);
        Wrapper { _tmp: tmp, home }
    }

    fn bin(&self) -> PathBuf {
        self.home.bin_dir()
    }

    /// Runs `git <args>` the way a Session does, with `path` as `PATH`: (exit code, stdout, stderr). Panics when the
    /// call is not over in [`BOUND`].
    fn run(&self, path: &OsString, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let mut cmd = Command::new(self.home.git_wrapper());
        cmd.args(args).current_dir(cwd).env("PATH", path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = spawn(&mut cmd).unwrap();
        if poll(BOUND, || child.try_wait().unwrap()).is_none() {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("`git {args:?}` through {} is not over after {BOUND:?}: wrappers that exec each other, PATH {path:?}", self.home.git_wrapper().display());
        }
        // Already reaped: `wait_with_output` reads what is left in the pipes.
        let out = child.wait_with_output().unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    }

    fn session_path(&self) -> OsString {
        git_guard::path_with_bin(&self.home, Some(common::inherited_path().to_owned())).into()
    }
}

/// A repo with one commit, made by the real git (the one on the test's own `PATH`).
fn repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir).env_clear().env("PATH", common::inherited_path());
        let out = output(&mut cmd).unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    fs::write(dir.join("f"), "x").unwrap();
    git(&["add", "f"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "first"]);
}

#[test]
fn it_refuses_git_stash_wherever_the_global_options_put_it() {
    let w = Wrapper::new();
    let cwd = tempfile::tempdir().unwrap();
    let path = w.session_path();
    let dir = cwd.path().to_str().unwrap();
    for args in [
        vec!["stash"],
        vec!["stash", "list"],
        vec!["-C", dir, "stash", "list"],
        vec!["-c", "a=b", "stash"],
        vec!["--no-pager", "-C", dir, "-c", "a=b", "stash", "pop"],
        vec!["--git-dir=/x/.git", "stash"],
        vec!["--work-tree", "/x", "--namespace", "n", "stash"],
    ] {
        let (code, stdout, stderr) = w.run(&path, cwd.path(), &args);
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (1, "", format!("{REFUSAL}\n").as_str()), "{args:?}");
    }
}

#[test]
fn it_runs_other_commands_through_the_real_git() {
    let w = Wrapper::new();
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    repo(&repo_dir);
    let path = w.session_path();
    let (code, stdout, stderr) = w.run(&path, &repo_dir, &["status", "--short"]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    let (code, stdout, _) = w.run(&path, tmp.path(), &["-C", repo_dir.to_str().unwrap(), "log", "--format=%s"]);
    assert_eq!((code, stdout.as_str()), (0, "first\n"));
    // A `stash` that is not the subcommand is not refused: a branch, a message, an argument.
    let (code, stdout, _) = w.run(&path, &repo_dir, &["log", "--format=%s", "--grep=stash"]);
    assert_eq!((code, stdout.as_str()), (0, ""));
}

#[test]
fn it_finds_the_real_git_when_its_own_directory_is_on_path_twice() {
    let w = Wrapper::new();
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    fs::create_dir_all(&real).unwrap();
    write_executable(&real.join("git"), "#!/bin/sh\necho real git \"$@\"\n");
    let bin = w.bin();
    let path = env::join_paths([bin.as_path(), bin.as_path(), real.as_path(), bin.as_path()]).unwrap();
    // Also by a spelling of the directory that is not the one it was started by.
    let dotted = bin.join(".");
    let path_dotted = env::join_paths([dotted.as_path(), bin.as_path(), real.as_path()]).unwrap();
    for path in [path, path_dotted] {
        let (code, stdout, stderr) = w.run(&path, tmp.path(), &["-C", "d", "status"]);
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "real git -C d status\n", ""), "{path:?}");
    }
}

#[test]
fn it_ends_at_the_real_git_when_another_wrapper_is_on_path_too() {
    // Two `$CPD_HOME`s, a test env's and the User's: a Session's `PATH` has the User's `bin/` first, and a test of a
    // process it started puts the test's before it. Each wrapper finds the other as the first `git` once its own
    // directory is out.
    let (first, second) = (Wrapper::new(), Wrapper::new());
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    fs::create_dir_all(&real).unwrap();
    write_executable(&real.join("git"), "#!/bin/sh\necho real git \"$@\"\necho \"$PATH\"\n");
    for dirs in [[first.bin(), second.bin(), real.clone()], [second.bin(), first.bin(), real.clone()]] {
        let path = env::join_paths(dirs).unwrap();
        for w in [&first, &second] {
            let (code, stdout, stderr) = w.run(&path, tmp.path(), &["-C", "d", "status"]);
            // The real git gets the `PATH` as it was given: the wrapper stays on it for what git itself runs.
            let want = format!("real git -C d status\n{}\n", path.to_str().unwrap());
            assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, want.as_str(), ""), "{path:?}");
        }
    }
}

#[test]
fn it_says_so_and_exits_127_when_the_only_other_git_is_another_wrapper() {
    let (first, second) = (Wrapper::new(), Wrapper::new());
    let tmp = tempfile::tempdir().unwrap();
    let path = env::join_paths([first.bin(), second.bin()]).unwrap();
    for w in [&first, &second] {
        let (code, stdout, stderr) = w.run(&path, tmp.path(), &["status"]);
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (127, "", format!("{NO_GIT}\n").as_str()));
    }
}

#[test]
fn it_says_so_and_exits_127_when_there_is_no_other_git() {
    let w = Wrapper::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin = w.bin();
    let path = env::join_paths([bin.as_path(), bin.as_path()]).unwrap();
    let (code, stdout, stderr) = w.run(&path, tmp.path(), &["status"]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (127, "", format!("{NO_GIT}\n").as_str()));
}

#[test]
fn writing_it_twice_leaves_one_identical_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(tmp.path().join("cpd-home").into()), None).unwrap();
    let path = git_guard::install(&home).unwrap();
    assert_eq!(path, home.git_wrapper());
    let first = fs::read(&path).unwrap();
    assert_eq!(git_guard::install(&home).unwrap(), path);
    assert_eq!(fs::read(&path).unwrap(), first);
    assert_eq!(first, git_guard::WRAPPER.as_bytes());
    assert_eq!(fs::read_dir(home.bin_dir()).unwrap().count(), 1);
}

#[test]
fn the_harness_takes_every_wrapper_directory_out_of_a_path_and_nothing_else() {
    let (first, second) = (Wrapper::new(), Wrapper::new());
    let tmp = tempfile::tempdir().unwrap();
    let (real, tools, missing) = (tmp.path().join("real"), tmp.path().join("tools"), tmp.path().join("missing"));
    for dir in [&real, &tools] {
        fs::create_dir_all(dir).unwrap();
    }
    write_executable(&real.join("git"), "#!/bin/sh\n");
    write_executable(&tools.join("tmux"), "#!/bin/sh\n");
    // An older wrapper is one too: it starts with the signature and goes on differently.
    let older = tmp.path().join("older");
    fs::create_dir_all(&older).unwrap();
    write_executable(&older.join("git"), &format!("{}: refuses `git stash`.\nexec echo older\n", git_guard::SIGNATURE));
    let dirs = [first.bin(), real.clone(), second.bin(), older, tools.clone(), missing.clone(), first.bin()];
    let path = env::join_paths(dirs).unwrap();
    let want = env::join_paths([real, tools, missing]).unwrap();
    assert_eq!(common::without_wrapper_dirs(&path), want);
}
