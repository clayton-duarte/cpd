//! The `git` wrapper that refuses `git stash` (contract §6.5, hs-d310).

use std::env;
use std::fs;
use std::path::Path;

use cpd_core::home::CpdHome;
use cpd_core::host::git_guard::{self, NO_GIT, REFUSAL};

use crate::common;
use common::write_executable;

use common::GitWrapper as Wrapper;

/// A program that does nothing and succeeds, to stand where the real `git` would: git is git's to test, not ours.
fn true_program() -> &'static Path {
    ["/usr/bin/true", "/bin/true"].into_iter().map(Path::new).find(|p| p.exists()).expect("a `true` program")
}

/// The one test of the authored wrapper script, run by a real `sh` (OS behaviour of the script: policy [tests]'s
/// named stay, research tests-mock-mig-pr.md section 5): the whole `stash` argv table, the pass-through to the first
/// other `git` on `PATH` (a symlink to `true`: no real git runs), and the exit 127 of both `PATH` layouts with no
/// other `git`.
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

    // Anything that is not the `stash` subcommand goes on to the other `git` on `PATH`: here `true`, which prints nothing.
    let real = cwd.path().join("real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(true_program(), real.join("git")).unwrap();
    let path = env::join_paths([w.bin(), real]).unwrap();
    for args in [
        vec!["status", "--short"],
        vec!["-C", dir, "log", "--format=%s"],
        // A `stash` that is not the subcommand is not refused: a branch, a message, an argument.
        vec!["log", "--format=%s", "--grep=stash"],
    ] {
        let (code, stdout, stderr) = w.run(&path, cwd.path(), &args);
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""), "{args:?}");
    }

    // No other `git`: the only other directory is another wrapper's, or this wrapper's own again.
    let second = Wrapper::new();
    let (own, other) = (w.bin(), second.bin());
    for (wrapper, dirs) in [(&w, [other.as_path(), own.as_path()]), (&second, [own.as_path(), other.as_path()]), (&w, [own.as_path(), own.as_path()])] {
        let path = env::join_paths(dirs).unwrap();
        let (code, stdout, stderr) = wrapper.run(&path, cwd.path(), &["status"]);
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (127, "", format!("{NO_GIT}\n").as_str()), "{dirs:?}");
    }
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
    let tmp = tempfile::tempdir().unwrap();
    // Two wrapper directories, written as `install` writes them: no process, no `TestEnv`.
    let wrapper_bin = |name: &str| {
        let home = CpdHome::resolve(Some(tmp.path().join(name).into()), None).unwrap();
        git_guard::install(&home).unwrap();
        home.bin_dir()
    };
    let (first, second) = (wrapper_bin("home-a"), wrapper_bin("home-b"));
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
    let dirs = [first.clone(), real.clone(), second, older, tools.clone(), missing.clone(), first];
    let path = env::join_paths(dirs).unwrap();
    let want = env::join_paths([real, tools, missing]).unwrap();
    assert_eq!(common::without_wrapper_dirs(&path), want);
}
