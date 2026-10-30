//! `git::tamper::written_paths` (contract §7.3, "The written files") on the fake git of an `Fx`.

use crate::common;

use std::path::{Path, PathBuf};

use common::fx::Fx;
use cpd_core::error::ErrorCode;
use cpd_core::git::tamper::written_paths;
use cpd_core::host::runner::{Call, Reply};

/// A repo whose `main` has `README.md`, plus the branches `lead` and `att` at `main`.
fn repo() -> (Fx, PathBuf) {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    fx.git(&repo, &["branch", "lead"]);
    fx.git(&repo, &["branch", "att"]);
    (fx, repo)
}

/// Commits every `(path, text)` on `branch`, then returns to `main`.
fn on(fx: &Fx, repo: &Path, branch: &str, files: &[(&str, &str)]) {
    fx.git(repo, &["switch", "-q", branch]);
    for (path, text) in files {
        fx.commit_file(repo, path, text, &format!("write {path}"));
    }
    fx.git(repo, &["switch", "-q", "main"]);
}

fn merge(fx: &Fx, repo: &Path, into: &str, from: &str) {
    fx.git(repo, &["switch", "-q", into]);
    fx.git(repo, &["merge", "-q", "--no-ff", "-m", "merge", from]);
    fx.git(repo, &["switch", "-q", "main"]);
}

fn written(fx: &Fx, repo: &Path, refs: &[&str]) -> Vec<String> {
    fx.scoped(|| written_paths(repo, "main", "att", refs).unwrap())
}

fn strings(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn is_diff(call: &Call) -> bool {
    call.args.iter().any(|a| a == "diff")
}

#[test]
fn with_no_ref_it_is_the_whole_range_diff() {
    let (fx, repo) = repo();
    on(&fx, &repo, "att", &[("a.rs", "a\n"), ("b.rs", "b\n")]);
    assert_eq!(written(&fx, &repo, &[]), strings(&["a.rs", "b.rs"]));
}

#[test]
fn a_path_equal_to_the_lead_tip_is_left_out() {
    let (fx, repo) = repo();
    on(&fx, &repo, "lead", &[("lead.rs", "from the lead\n")]);
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.git(&repo, &["merge", "-q", "--no-ff", "-m", "merge the lead", "lead"]);
    fx.git(&repo, &["switch", "-q", "main"]);
    on(&fx, &repo, "att", &[("own.rs", "mine\n")]);
    assert_eq!(written(&fx, &repo, &["lead"]), strings(&["own.rs"]));
}

#[test]
fn a_path_equal_to_any_one_of_several_refs_is_left_out() {
    let (fx, repo) = repo();
    fx.git(&repo, &["branch", "second"]);
    on(&fx, &repo, "lead", &[("lead.rs", "from the lead\n")]);
    on(&fx, &repo, "second", &[("second.rs", "from the second\n")]);
    merge(&fx, &repo, "att", "second");
    on(&fx, &repo, "att", &[("own.rs", "mine\n")]);
    assert_eq!(written(&fx, &repo, &["lead", "second"]), strings(&["own.rs"]));
}

#[test]
fn a_file_deleted_in_the_attempt_is_listed_unless_a_ref_deleted_it_too() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    fx.commit_file(&repo, "d.txt", "d\n", "add d");
    fx.commit_file(&repo, "e.txt", "e\n", "add e");
    fx.git(&repo, &["branch", "lead"]);
    fx.git(&repo, &["branch", "att"]);
    fx.git(&repo, &["switch", "-q", "lead"]);
    fx.git(&repo, &["rm", "-q", "e.txt"]);
    fx.git(&repo, &["commit", "-q", "-m", "lead removes e"]);
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.git(&repo, &["rm", "-q", "d.txt", "e.txt"]);
    fx.git(&repo, &["commit", "-q", "-m", "att removes both"]);
    fx.git(&repo, &["switch", "-q", "main"]);
    assert_eq!(written(&fx, &repo, &[]), strings(&["d.txt", "e.txt"]));
    assert_eq!(written(&fx, &repo, &["lead"]), strings(&["d.txt"]));
}

#[test]
fn a_rename_lists_both_paths() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    fx.commit_file(&repo, "old.txt", "some content\n", "add old");
    fx.git(&repo, &["branch", "lead"]);
    fx.git(&repo, &["branch", "att"]);
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.git(&repo, &["mv", "old.txt", "new.txt"]);
    fx.git(&repo, &["commit", "-q", "-m", "rename"]);
    fx.git(&repo, &["switch", "-q", "main"]);
    assert_eq!(written(&fx, &repo, &["lead"]), strings(&["new.txt", "old.txt"]));
}

#[test]
fn the_list_is_in_the_diffs_bytewise_order() {
    let (fx, repo) = repo();
    // Bytewise: "C" < "Z" < "a.txt" < "a/x" ('.' is 0x2e, '/' is 0x2f) < "b".
    on(&fx, &repo, "att", &[("b", "1\n"), ("a/x", "2\n"), ("Z", "3\n"), ("a.txt", "4\n"), ("C", "5\n")]);
    let expected = strings(&["C", "Z", "a.txt", "a/x", "b"]);
    assert_eq!(written(&fx, &repo, &[]), expected);
    assert_eq!(written(&fx, &repo, &["lead"]), expected);
}

#[test]
fn it_runs_one_diff_per_ref_and_no_call_per_path() {
    let (fx, repo) = repo();
    fx.git(&repo, &["branch", "second"]);
    let names: Vec<String> = (0..50).map(|i| format!("f{i:02}.txt")).collect();
    let files: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "x\n")).collect();
    on(&fx, &repo, "att", &files);
    let before = fx.calls("git").len();
    let found = written(&fx, &repo, &["lead", "second"]);
    assert_eq!(found.len(), 50);
    let calls: Vec<Call> = fx.calls("git").into_iter().skip(before).collect();
    assert_eq!(calls.iter().filter(|c| is_diff(c)).count(), 3, "{calls:?}");
    let names_a_path = |c: &Call| c.args.iter().any(|a| names.iter().any(|n| a.contains(n.as_str())));
    assert!(!calls.iter().any(|c| c.args.iter().any(|a| a == "rev-parse") && names_a_path(c)), "{calls:?}");
}

#[test]
fn a_failing_git_call_is_git_failed_and_returns_no_list() {
    let (fx, repo) = repo();
    on(&fx, &repo, "att", &[("a.rs", "a\n")]);
    let is_ref_diff = |call: &Call| is_diff(call) && !call.args.iter().any(|a| a.contains("..."));
    let runner = (*fx.fakes.runner).clone();
    let _ = runner.once("git", is_ref_diff, Reply::exit(128, "", "fatal: bad revision\n"));
    let err = fx.scoped(|| written_paths(&repo, "main", "att", &["lead"])).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
}
