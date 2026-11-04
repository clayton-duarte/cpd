//! `context distill`'s `git grep` of the docs check (contract §4.6 step 3): for each record of the batch and each
//! repo of the Context's Workflows, `git -C <repo> grep -l -w <id> -- docs '*.md'`, spawned through the
//! [`Runner`](crate::host::runner::Runner) (`distill::grep_hits_with`), never as a direct `Command`.
//!
//! Two kinds of test, none of which runs git or any process:
//! - unit tests on SCRIPTED replies of the `FakeRunner` (`docs_md_*`, `a_documented_target_*`, the exit codes):
//!   they pin the argv, the order of the calls, the `<repo>:<path>` form of a hit and what exit 1 and any other
//!   failure mean;
//! - tests of the ROUTE: `grep -l -w <word> -- docs '*.md'` of the fake git over a `Repo` of the Universe, whose
//!   texts and exit codes were recorded once by hand from real git in a scratch dir (git 2.54.0, Apple Git-157,
//!   not 2.43; unverified on 2.43). What real git did there: the pathspec `docs` is a directory (every file
//!   under it, whatever its name), `*.md` matches a name at any depth (`*` crosses `/`); the files come
//!   sorted by path; only tracked files are searched, and in their WORKING TREE state (an edit that is not
//!   staged is searched, a file that was deleted is not); `-w` is a whole-word match (`hs-d1` is not in `hs-d10`);
//!   exit 1 and nothing printed when nothing matches; in a directory that is no repo exit 128 and
//!   `fatal: not a git repository (or any of the parent directories): .git`.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::curation::Approval;
use super::distill::{Batch, grep_hits_with};
use super::render::Indexed;
use super::{By, Draft, Record};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::fakes::git::{GitRoute, Repo, Universe};
use crate::host::deadline::Failed;
use crate::host::runner::{FakeRunner, Reply, Runner};
use crate::ids::RecordId;

fn id(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

fn record(seq: i64, text_id: &str, text: &str, draft: Draft) -> Indexed {
    (
        seq,
        Record {
            id: id(text_id),
            at: "2026-10-01T10:00:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: text.into(),
            draft,
        },
    )
}

fn decision(seq: i64, text_id: &str) -> Indexed {
    record(
        seq,
        text_id,
        "Use it.",
        Draft::Decision { supersedes: vec![] },
    )
}

/// The argv of one grep call, after the program: what the product asks.
fn grep_args(repo: &str, word: &str) -> Vec<String> {
    ["-C", repo, "grep", "-l", "-w", word, "--", "docs", "*.md"]
        .iter()
        .map(|a| (*a).to_owned())
        .collect()
}

/// A fake that answers the grep of `word` in `repo` with `reply`.
fn answer(runner: FakeRunner, repo: &str, word: &str, reply: Reply) -> FakeRunner {
    let args = grep_args(repo, word);
    runner.on("git", move |call| call.args == args, reply)
}

fn runner() -> FakeRunner {
    FakeRunner::new(ManualClock::new())
}

fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

fn hits_of(map: &HashMap<RecordId, Vec<String>>, text_id: &str) -> Vec<String> {
    map.get(&id(text_id))
        .unwrap_or_else(|| panic!("no entry for {text_id}: {map:?}"))
        .clone()
}

// ---- scripted replies -------------------------------------------------------------------------------------

#[test]
fn docs_md_lists_the_grep_hits_of_the_contexts_repos() {
    let (d1, d10) = (decision(1, "hs-d1"), decision(2, "hs-d10"));
    let r = answer(runner(), "/r/a", "hs-d1", Reply::ok("docs/design.md\n"));
    let r = answer(r, "/r/a", "hs-d10", Reply::ok("docs/other.md\n"));
    let r = answer(r, "/r/b", "hs-d1", Reply::ok("README.md\nsub/deep.md\n"));
    let r = answer(r, "/r/b", "hs-d10", Reply::exit(1, "", ""));
    let hits = grep_hits_with(&r, &repos(&["/r/a", "/r/b"]), &[&d1, &d10]).unwrap();
    assert_eq!(
        hits_of(&hits, "hs-d1"),
        ["/r/a:docs/design.md", "/r/b:README.md", "/r/b:sub/deep.md"]
    );
    assert_eq!(hits_of(&hits, "hs-d10"), ["/r/a:docs/other.md"]);
    assert_eq!(hits.len(), 2);
}

#[test]
fn the_grep_asks_git_for_each_record_in_each_repo_in_that_order_and_nothing_else() {
    let (d1, d2) = (decision(1, "hs-d1"), decision(2, "hs-d2"));
    let r = runner().on("git", |_| true, Reply::exit(1, "", ""));
    grep_hits_with(&r, &repos(&["/r/a", "/r/b"]), &[&d1, &d2]).unwrap();
    let calls = r.calls();
    let asked: Vec<(&str, &Vec<String>)> = calls
        .iter()
        .map(|c| (c.program.as_str(), &c.args))
        .collect();
    assert_eq!(
        asked,
        [
            ("git", &grep_args("/r/a", "hs-d1")),
            ("git", &grep_args("/r/b", "hs-d1")),
            ("git", &grep_args("/r/a", "hs-d2")),
            ("git", &grep_args("/r/b", "hs-d2")),
        ]
    );
    assert!(r.unscripted().is_empty());
}

#[test]
fn a_git_grep_call_names_its_tool_and_has_a_deadline() {
    let d1 = decision(1, "hs-d1");
    let r = runner().on("git", |_| true, Reply::exit(1, "", ""));
    grep_hits_with(&r, &repos(&["/r/a"]), &[&d1]).unwrap();
    let call = &r.calls()[0];
    assert_eq!(call.tool, "git");
    assert!(
        call.deadline > std::time::Duration::ZERO,
        "the Runner is given a deadline: {call:?}"
    );
}

#[test]
fn a_grep_that_exits_1_is_no_hit_and_the_record_still_has_an_empty_list() {
    let d1 = decision(1, "hs-d1");
    let r = runner().on("git", |_| true, Reply::exit(1, "", ""));
    let hits = grep_hits_with(&r, &repos(&["/r/a", "/r/b"]), &[&d1]).unwrap();
    assert_eq!(hits_of(&hits, "hs-d1"), Vec::<String>::new());
}

#[test]
fn a_context_with_no_repo_has_no_hit_and_asks_git_nothing() {
    let d1 = decision(1, "hs-d1");
    let r = runner();
    let hits = grep_hits_with(&r, &[], &[&d1]).unwrap();
    assert_eq!(hits_of(&hits, "hs-d1"), Vec::<String>::new());
    assert!(r.calls().is_empty());
}

#[test]
fn a_grep_that_fails_is_git_failed_with_the_repo_and_the_stderr() {
    let d1 = decision(1, "hs-d1");
    let r = runner().on(
        "git",
        |_| true,
        Reply::exit(
            128,
            "",
            "fatal: not a git repository (or any of the parent directories): .git\n",
        ),
    );
    let e = grep_hits_with(&r, &repos(&["/r/a"]), &[&d1]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::GitFailed);
    assert_eq!(
        e.message(),
        "Cannot search /r/a for the record ids: fatal: not a git repository (or any of the parent directories): .git."
    );
    assert_eq!(
        e.details().get("repo").and_then(|v| v.as_str()),
        Some("/r/a")
    );
}

#[test]
fn a_grep_that_exits_2_is_a_failure_too_not_a_miss() {
    let d1 = decision(1, "hs-d1");
    let r = runner().on("git", |_| true, Reply::exit(2, "", "usage: git grep\n"));
    let e = grep_hits_with(&r, &repos(&["/r/a"]), &[&d1]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::GitFailed);
}

#[test]
fn a_git_that_cannot_start_is_git_failed_with_the_repo() {
    let d1 = decision(1, "hs-d1");
    let r = runner().on("git", |_| true, Reply::io(ErrorKind::NotFound));
    let e = grep_hits_with(&r, &repos(&["/r/a"]), &[&d1]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::GitFailed);
    assert_eq!(
        e.details().get("repo").and_then(|v| v.as_str()),
        Some("/r/a")
    );
}

#[test]
fn a_grep_that_fails_ends_the_search_at_that_repo() {
    let (d1, d2) = (decision(1, "hs-d1"), decision(2, "hs-d2"));
    let r = answer(
        runner(),
        "/r/a",
        "hs-d1",
        Reply::exit(128, "", "fatal: boom\n"),
    );
    let r = r.on("git", |_| true, Reply::exit(1, "", ""));
    let e = grep_hits_with(&r, &repos(&["/r/a", "/r/b"]), &[&d1, &d2]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::GitFailed);
    assert_eq!(r.calls().len(), 1, "no later call: {:?}", r.calls());
}

#[test]
fn a_hit_is_the_repo_as_given_a_colon_and_the_path_git_printed() {
    let d1 = decision(1, "hs-d1");
    let repo = "/Users/x/projects/cpd.a";
    let r = answer(runner(), repo, "hs-d1", Reply::ok("docs/design.md\n"));
    let hits = grep_hits_with(&r, &repos(&[repo]), &[&d1]).unwrap();
    assert_eq!(
        hits_of(&hits, "hs-d1"),
        ["/Users/x/projects/cpd.a:docs/design.md"]
    );
}

/// The model's line for `id`, retiring it as documented at `target`.
fn documented(text_id: &str, topic: &str, short: &str, target: &str) -> String {
    format!(
        r#"{{"id":"{text_id}","topic":"{topic}","short":"{short}","retire":{{"reason":"documented","target":"{target}"}}}}"#
    )
}

#[test]
fn a_documented_target_that_is_not_a_grep_hit_rejects_only_that_line() {
    let (d1, d2) = (decision(1, "hs-d1"), decision(2, "hs-d2"));
    let n3 = record(3, "hs-n3", "Locks are flock.", Draft::Note);
    let (design, wal) = ("/r/a:docs/design.md", "/r/a:docs/wal.md");
    let r = answer(runner(), "/r/a", "hs-d1", Reply::ok("docs/design.md\n"));
    let r = answer(r, "/r/a", "hs-d2", Reply::ok("docs/wal.md\n"));
    let r = answer(r, "/r/a", "hs-n3", Reply::exit(1, "", ""));
    let records = [&d1, &d2, &n3];
    let hits = grep_hits_with(&r, &repos(&["/r/a"]), &records).unwrap();
    let batch = Batch {
        records: &records,
        in_force: records
            .iter()
            .map(|(seq, rec)| (rec.id.clone(), *seq))
            .collect(),
        hits,
    };
    // hs-d1 names a path that is no hit; hs-d2 names a real hit of its own; hs-n3 names a hit of hs-d1.
    let text = [
        documented("hs-d1", "storage", "sqlite", "/r/a:docs/invented.md"),
        documented("hs-d2", "storage", "sqlite WAL", wal),
        documented("hs-n3", "locking", "flock", design),
    ]
    .join("\n");
    let checked = batch.check(&text);
    let rejected: Vec<(Option<String>, Option<usize>)> = checked
        .rejected
        .iter()
        .map(|p| (p.id.as_ref().map(ToString::to_string), p.line))
        .collect();
    assert_eq!(
        rejected,
        [
            (Some("hs-d1".to_owned()), Some(1)),
            (Some("hs-n3".to_owned()), Some(3))
        ]
    );
    assert!(
        checked
            .rejected
            .iter()
            .all(|p| p.what.contains("is not one of the record's grep hits")),
        "{:?}",
        checked.rejected
    );
    assert_eq!(
        checked
            .curations
            .iter()
            .map(|c| c.record.to_string())
            .collect::<Vec<_>>(),
        ["hs-d2"]
    );
    assert_eq!(
        checked
            .proposals
            .iter()
            .map(|p| (p.record.to_string(), p.target.clone()))
            .collect::<Vec<_>>(),
        [("hs-d2".to_owned(), wal.to_owned())]
    );
}

#[test]
fn a_documented_target_that_is_a_grep_hit_of_the_record_in_a_second_repo_is_accepted() {
    let d1 = decision(1, "hs-d1");
    let r = answer(runner(), "/r/a", "hs-d1", Reply::exit(1, "", ""));
    let r = answer(r, "/r/b", "hs-d1", Reply::ok("README.md\n"));
    let records = [&d1];
    let hits = grep_hits_with(&r, &repos(&["/r/a", "/r/b"]), &records).unwrap();
    let batch = Batch {
        records: &records,
        in_force: HashMap::from([(id("hs-d1"), 1)]),
        hits,
    };
    let checked = batch.check(&documented("hs-d1", "storage", "sqlite", "/r/b:README.md"));
    assert_eq!(
        (checked.rejected.len(), checked.proposals.len()),
        (0, 1),
        "{:?}",
        checked.rejected
    );
}

fn line(text_id: &str, reason: &str, target: &str) -> String {
    format!(
        r#"{{"id":"{text_id}","topic":"t-{text_id}","short":"s","retire":{{"reason":"{reason}","target":"{target}"}}}}"#
    )
}

/// A batch of `hs-d1..hs-d4` (seq 1..4) whose only grep hit is `hs-d4`'s `/r/a:docs/d4.md`.
fn mixed_checked() -> super::distill::Checked {
    let records = [
        decision(1, "hs-d1"),
        decision(2, "hs-d2"),
        decision(3, "hs-d3"),
        decision(4, "hs-d4"),
    ];
    let refs: Vec<&Indexed> = records.iter().collect();
    let batch = Batch {
        records: &refs,
        in_force: refs.iter().map(|(seq, r)| (r.id.clone(), *seq)).collect(),
        hits: HashMap::from([(id("hs-d4"), vec!["/r/a:docs/d4.md".to_owned()])]),
    };
    let text = [
        line("hs-d1", "superseded", "hs-d2"),
        line("hs-d3", "duplicate", "hs-d2"),
        line("hs-d4", "documented", "/r/a:docs/d4.md"),
    ]
    .join("\n");
    let checked = batch.check(&text);
    assert!(checked.rejected.is_empty(), "{:?}", checked.rejected);
    checked
}

#[test]
fn a_documented_proposal_carries_the_git_grep_approval() {
    let checked = mixed_checked();
    let documented = checked
        .proposals
        .iter()
        .find(|p| p.record == id("hs-d4"))
        .unwrap();
    assert_eq!(documented.approval, Approval::GitGrep);
}

#[test]
fn a_duplicate_or_superseded_proposal_starts_pending() {
    let checked = mixed_checked();
    let approvals: Vec<(String, Approval)> = checked
        .proposals
        .iter()
        .map(|p| (p.record.to_string(), p.approval))
        .collect();
    assert_eq!(
        approvals,
        [
            ("hs-d1".to_owned(), Approval::Pending),
            ("hs-d3".to_owned(), Approval::Pending),
            ("hs-d4".to_owned(), Approval::GitGrep),
        ]
    );
}

#[test]
fn the_grep_is_spawned_through_the_runner_not_as_a_direct_command() {
    // The tripwire of the rule "no direct spawn of git": the file that greps names no `Command::new("git")`.
    let source = include_str!("distill.rs");
    assert!(
        !source.contains("Command::new(\"git\")"),
        "distill.rs still spawns git directly"
    );
}

// ---- the route --------------------------------------------------------------------------------------------

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// A Universe, the `GitRoute` over it and a `FakeRunner` that routes `git` to it, with a temp dir for the repos.
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

fn p(path: &Path) -> &str {
    path.to_str().expect("a temp path is UTF-8")
}

impl World {
    /// The temp root, canonical (what real git prints for a path under it).
    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    /// A repo on `main` whose one commit holds `files`.
    fn repo(&mut self, files: &[(&str, &str)]) -> Repo {
        self.repos += 1;
        let repo = Repo::init(
            &self.universe,
            &self.root().join(format!("repo{}", self.repos)),
        );
        for (path, text) in files {
            repo.write(path, text);
        }
        repo.commit("base");
        repo
    }
    /// `git -C <dir> <args...>`, the form of the product.
    fn git(&self, dir: &Path, args: &[&str]) -> Out {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        match self.runner.output(&mut cmd, "git", Duration::from_secs(60)) {
            Ok(o) => Out {
                code: std::os::unix::process::ExitStatusExt::into_raw(o.status) >> 8,
                stdout: String::from_utf8(o.stdout).unwrap(),
                stderr: String::from_utf8(o.stderr).unwrap(),
            },
            Err(Failed::Io(e)) => panic!("git could not start: {e}"),
            Err(Failed::Timeout(e)) => panic!("git timed out: {e:?}"),
        }
    }
}

fn grep(w: &World, dir: &Path, word: &str) -> Out {
    w.git(dir, &["grep", "-l", "-w", word, "--", "docs", "*.md"])
}

/// A repo whose one commit holds the files of the research: the pathspec `docs` and `*.md` at any depth.
fn docs_repo(w: &mut World) -> Repo {
    w.repo(&[
        ("README.md", "hs-d1\n"),
        ("notes.txt", "hs-d1 is not markdown\n"),
        ("docs/design.md", "see hs-d1\n"),
        ("docs/other.md", "hs-d10 only\n"),
        ("docs/data.txt", "hs-d1\n"),
        ("docs/sub/z.md", "hs-d1\n"),
        ("sub/deep.md", "hs-d1\n"),
        ("clean.md", "clean\n"),
    ])
}

#[test]
fn git_grep_lists_what_is_under_docs_and_the_markdown_files_at_any_depth_sorted() {
    let mut w = world();
    let repo = docs_repo(&mut w);
    let out = grep(&w, repo.dir(), "hs-d1");
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (
            0,
            "README.md\ndocs/data.txt\ndocs/design.md\ndocs/sub/z.md\nsub/deep.md\n"
        ),
        "{}",
        out.stderr
    );
}

#[test]
fn git_grep_w_matches_whole_words_only() {
    let mut w = world();
    let repo = docs_repo(&mut w);
    let out = grep(&w, repo.dir(), "hs-d10");
    assert_eq!(
        (out.code, out.stdout.as_str()),
        (0, "docs/other.md\n"),
        "{}",
        out.stderr
    );
}

#[test]
fn git_grep_exits_1_and_prints_nothing_when_no_file_has_the_word() {
    let mut w = world();
    let repo = docs_repo(&mut w);
    let out = grep(&w, repo.dir(), "hs-d2");
    assert_eq!(
        (out.code, out.stdout, out.stderr),
        (1, String::new(), String::new())
    );
}

#[test]
fn git_grep_searches_tracked_files_as_they_are_in_the_working_tree() {
    let mut w = world();
    let repo = docs_repo(&mut w);
    // Not committed, not staged: an edit of a tracked file is searched, an untracked file is not, a deleted one is not.
    repo.write("clean.md", "clean\nhs-d1 now\n");
    repo.write("untracked.md", "hs-d1\n");
    std::fs::remove_file(repo.dir().join("README.md")).unwrap();
    let out = grep(&w, repo.dir(), "hs-d1");
    assert_eq!(
        out.stdout, "clean.md\ndocs/data.txt\ndocs/design.md\ndocs/sub/z.md\nsub/deep.md\n",
        "{}",
        out.stderr
    );
}

#[test]
fn git_grep_in_a_directory_that_is_no_repo_is_128() {
    let w = world();
    let plain = w.root().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let out = grep(&w, &plain, "hs-d1");
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (
            128,
            "fatal: not a git repository (or any of the parent directories): .git\n"
        )
    );
}

#[test]
fn git_grep_is_a_call_the_route_supports() {
    let mut w = world();
    let repo = docs_repo(&mut w);
    grep(&w, repo.dir(), "hs-d1");
    assert_eq!(w.route.unsupported(), Vec::<String>::new());
}

#[test]
fn the_hits_through_the_route_are_the_repo_as_given_and_the_paths_of_git_grep() {
    let mut w = world();
    let (a, b) = (
        docs_repo(&mut w),
        w.repo(&[("README.md", "Decided in hs-d1.\n")]),
    );
    let (d1, d10) = (decision(1, "hs-d1"), decision(2, "hs-d10"));
    let names = vec![p(a.dir()).to_owned(), p(b.dir()).to_owned()];
    let hits = grep_hits_with(&w.runner, &names, &[&d1, &d10]).unwrap();
    let prefix = |dir: &Path, paths: &[&str]| -> Vec<String> {
        paths
            .iter()
            .map(|p| format!("{}:{p}", dir.display()))
            .collect()
    };
    let mut want_d1 = prefix(
        a.dir(),
        &[
            "README.md",
            "docs/data.txt",
            "docs/design.md",
            "docs/sub/z.md",
            "sub/deep.md",
        ],
    );
    want_d1.extend(prefix(b.dir(), &["README.md"]));
    assert_eq!(hits_of(&hits, "hs-d1"), want_d1);
    assert_eq!(
        hits_of(&hits, "hs-d10"),
        prefix(a.dir(), &["docs/other.md"])
    );
    assert_eq!(w.route.unsupported(), Vec::<String>::new());
}

#[test]
fn a_repo_that_is_gone_is_git_failed_through_the_route() {
    let w = world();
    let gone = w.root().join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    let d1 = decision(1, "hs-d1");
    let e = grep_hits_with(&w.runner, &[p(&gone).to_owned()], &[&d1]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::GitFailed);
    assert!(
        e.message().contains("fatal: not a git repository"),
        "{}",
        e.message()
    );
}
