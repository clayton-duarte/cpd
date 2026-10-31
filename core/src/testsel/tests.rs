//! `cpd test changed` and `cpd test run` (commands.md §26), in process on `Fakes`: the old exec tests of
//! `scripts/test-changed` (`cli/tests/tooling/test_changed_script.rs`) and `scripts/tests.sh`
//! (`cli/tests/tooling/tests_script.rs`) ported name for name, with the real git, cargo, nextest, tsc and node
//! replaced by canned replies of the `FakeRunner`. The files are real (a temp dir per test); no process runs.
//!
//! Selection: the changed files come from canned `git diff --name-only` and `git ls-files --others` text, the crates
//! from canned `cargo metadata` JSON; what the logic would run is read from the dry run's one line, and from the argv
//! of the calls on `FakeRunner::calls()`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::TempDir;

use super::{ChangedArgs, Kind, build, changed, kind_of, list, pi_name, run};
use crate::error::CpdError;
use crate::host::runner::{Call, FakeRunner, Reply};
use crate::ports::Fakes;

/// What a command did: its exit status (a refusal's own exit code), the text it wrote, and every process it asked for.
struct Out {
    status: u8,
    stdout: String,
    stderr: String,
    calls: Vec<Call>,
    unscripted: Vec<Call>,
}

impl Out {
    fn of(result: Result<u8, CpdError>, stdout: Vec<u8>, mut stderr: Vec<u8>, fakes: &Fakes) -> Out {
        let status = match result {
            Ok(status) => status,
            Err(err) => {
                stderr.extend_from_slice(err.message().as_bytes());
                err.exit().code()
            }
        };
        Out {
            status,
            stdout: String::from_utf8(stdout).expect("utf-8 on stdout"),
            stderr: String::from_utf8(stderr).expect("utf-8 on stderr"),
            calls: fakes.runner.calls(),
            unscripted: fakes.runner.unscripted(),
        }
    }

    /// Everything that was said, for a failure message.
    fn what(&self) -> String {
        format!("status {}, stdout {:?}, stderr {:?}, calls {:?}", self.status, self.stdout, self.stderr, self.argvs())
    }

    fn argvs(&self) -> Vec<Vec<String>> {
        self.calls.iter().map(argv).collect()
    }

    /// The calls of the program whose file name is `name`.
    fn calls_of(&self, name: &str) -> Vec<&Call> {
        self.calls.iter().filter(|c| c.program.rsplit('/').next() == Some(name)).collect()
    }

    /// The `cargo.sh` calls whose first argument is `first`.
    fn cargo(&self, first: &str) -> Vec<&Call> {
        self.calls_of("cargo.sh").into_iter().filter(|c| c.args.first().map(String::as_str) == Some(first)).collect()
    }

    /// The git call that has `word` as an argument.
    fn git(&self, word: &str) -> &Call {
        self.calls_of("git")
            .into_iter()
            .find(|c| c.args.iter().any(|a| a == word))
            .unwrap_or_else(|| panic!("no git {word}: {}", self.what()))
    }
}

fn argv(call: &Call) -> Vec<String> {
    std::iter::once(call.program.clone()).chain(call.args.iter().cloned()).collect()
}

fn words(call: &Call) -> Vec<&str> {
    call.args.iter().map(String::as_str).collect()
}

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// A rule's test: the call has `word` as one of its arguments.
fn has(word: impl Into<String>) -> impl Fn(&Call) -> bool + Send + Sync + 'static {
    let word = word.into();
    move |call| call.args.iter().any(|a| *a == word)
}

/// A rule's test: the call's first argument is `word` (`scripts/cargo.sh <word> …`).
fn first(word: &'static str) -> impl Fn(&Call) -> bool + Send + Sync + 'static {
    move |call| call.args.first().is_some_and(|a| a == word)
}

fn lines(names: &[String]) -> String {
    names.iter().map(|n| format!("{n}\n")).collect()
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().expect("a file has a directory")).expect("make the directory");
    fs::write(file, text).expect("write the file");
}

fn scratch_dir() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temp dir");
    // `cargo metadata` and `git rev-parse` print real paths (on macOS `/var` is `/private/var`).
    let root = dir.path().canonicalize().expect("the temp dir exists");
    (dir, root)
}

// ---- `cpd test changed` ----------------------------------------------------------------------------------------------

/// The merge base of the base and HEAD, as the canned `git merge-base` prints it.
const MERGE_BASE: &str = "0123456789abcdef0123456789abcdef01234567";

/// The base the dry runs are given: a commit.
const BASE: &str = "89abcdef0123456789abcdef0123456789abcdef";

/// The scratch workspace's files at the base commit: cpd's crate graph and layout (`core/` is `cpd-core`, `tui/` is
/// `cpd-tui` on core, `cli/` is `cpd` on both, its integration tests one binary per area, `cli/tests/<area>/` with a
/// module per file, and `cli/tests/common/` shared by them). `(path, text)`.
const FILES: &[(&str, &str)] = &[
    ("Cargo.toml", "[workspace]\nresolver = \"3\"\nmembers = [\"core\", \"tui\", \"cli\"]\n"),
    ("core/Cargo.toml", "[package]\nname = \"cpd-core\"\n"),
    ("core/src/lib.rs", "pub fn core() {}\n"),
    ("core/src/store/migrations/0001-initial.sql", "-- cpd-migration: additive\n"),
    ("tui/Cargo.toml", "[package]\nname = \"cpd-tui\"\n"),
    ("tui/src/lib.rs", "pub fn tui() {}\n"),
    ("cli/Cargo.toml", "[package]\nname = \"cpd\"\n"),
    ("cli/build.rs", "fn main() {}\n"),
    ("cli/src/main.rs", "fn main() {}\n"),
    ("cli/tests/common/mod.rs", "pub fn helper() {}\n"),
    ("cli/tests/first/main.rs", "mod alpha;\nmod beta;\n"),
    ("cli/tests/first/alpha.rs", "#[test]\nfn one() {}\n"),
    ("cli/tests/first/beta.rs", "#[test]\nfn two() {}\n"),
    ("cli/tests/first/data/sample.txt", "sample\n"),
    ("cli/tests/second/main.rs", "mod gamma;\n"),
    ("cli/tests/second/gamma.rs", "#[test]\nfn three() {}\n"),
    (".config/nextest.toml", "[profile.default]\nfail-fast = false\n"),
    ("docs/notes.md", "# Notes\n"),
];

/// What the dry run printed: the full suite, or `-p` crates, `--test` binaries and `-E` filters.
#[derive(Debug, PartialEq, Eq)]
struct Selection {
    workspace: bool,
    packages: BTreeSet<String>,
    binaries: BTreeSet<String>,
    filters: Vec<String>,
}

fn crates(names: &[&str]) -> Selection {
    Selection { workspace: false, packages: names.iter().map(|n| n.to_string()).collect(), binaries: BTreeSet::new(), filters: Vec::new() }
}

/// Only the test binary (area) `area` of the crate `cpd`.
fn binary(area: &str) -> Selection {
    Selection { binaries: [area.to_string()].into(), ..crates(&["cpd"]) }
}

/// Only the module `name` in the one test binary `area`.
fn module(area: &str, name: &str) -> Selection {
    Selection { filters: vec![format!("test(/^{name}::/)")], ..binary(area) }
}

fn full_suite() -> Selection {
    Selection { workspace: true, ..crates(&[]) }
}

/// A scratch git repo's worth of facts: the workspace of [`FILES`] on disk, and the files git would report changed.
struct Scratch {
    _dir: TempDir,
    root: PathBuf,
    /// What `git diff --name-only <merge base>` prints: committed, staged and unstaged changes.
    changed: Vec<String>,
    /// What `git ls-files --others --exclude-standard` prints.
    untracked: Vec<String>,
}

impl Scratch {
    fn new() -> Scratch {
        let (dir, root) = scratch_dir();
        for (path, text) in FILES {
            write(&root, path, text);
        }
        Scratch { _dir: dir, root, changed: Vec::new(), untracked: Vec::new() }
    }

    /// `path` is changed: tracked, and different from the base (committed, staged or unstaged).
    fn edit(&mut self, path: &str) {
        self.changed.push(path.to_owned());
    }

    /// `path` is new and untracked.
    fn untracked(&mut self, path: &str) {
        self.untracked.push(path.to_owned());
    }

    /// `cargo metadata --no-deps` as cargo prints it for the workspace: its members, their directories and their
    /// path dependencies, in the workspace's order.
    fn metadata(&self) -> String {
        let root = self.root.display().to_string();
        let id = |name: &str, dir: &str| format!("path+file://{root}/{dir}#{name}@0.1.0");
        let package = |name: &str, dir: &str, deps: &[(&str, &str)]| {
            json!({
                "name": name,
                "version": "0.1.0",
                "id": id(name, dir),
                "manifest_path": format!("{root}/{dir}/Cargo.toml"),
                "dependencies": deps.iter().map(|(dep, at)| json!({"name": dep, "kind": null, "path": format!("{root}/{at}")})).collect::<Vec<_>>(),
            })
        };
        json!({
            "packages": [
                package("cpd-core", "core", &[]),
                package("cpd-tui", "tui", &[("cpd-core", "core")]),
                package("cpd", "cli", &[("cpd-core", "core"), ("cpd-tui", "tui")]),
            ],
            "workspace_members": [id("cpd-core", "core"), id("cpd-tui", "tui"), id("cpd", "cli")],
            "workspace_root": root,
            "target_directory": format!("{root}/target"),
            "version": 1,
        })
        .to_string()
    }

    /// The fakes with git and cargo's metadata answering from this scratch; nextest answers `nextest`.
    fn fakes(&self, nextest: Reply) -> Fakes {
        let fakes = Fakes::new();
        let _ = (*fakes.runner)
            .clone()
            .on("git", has("rev-parse"), Reply::ok(&format!("{}\n", self.root.display())))
            .on("git", has("merge-base"), Reply::ok(&format!("{MERGE_BASE}\n")))
            .on("git", has("diff"), Reply::ok(&lines(&self.changed)))
            .on("git", has("ls-files"), Reply::ok(&lines(&self.untracked)))
            .on("cargo.sh", first("metadata"), Reply::ok(&self.metadata()))
            .on("cargo.sh", first("nextest"), nextest);
        fakes
    }

    /// `cpd test changed [--dry-run] [<base>]` from `cwd`.
    fn changed(&self, cwd: &Path, dry_run: bool, base: Option<&str>, nextest: Reply) -> Out {
        let fakes = self.fakes(nextest);
        let args = ChangedArgs { dry_run, base: base.map(str::to_owned) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = changed(&fakes.ports, cwd, &args, &mut out, &mut err);
        Out::of(result, out, err, &fakes)
    }

    /// `cpd test changed --dry-run <base>` from the root.
    fn dry_run(&self) -> Out {
        self.changed(&self.root, true, Some(BASE), Reply::ok(""))
    }

    /// The selection of a dry run that must print a command: exit 0 and exactly one line,
    /// `<...>scripts/cargo.sh nextest run` and then `--workspace`, `-p <crate>`, `--test <binary>` and `-E <filter>` only.
    fn selection(&self) -> Selection {
        let out = self.dry_run();
        let what = out.what();
        assert_eq!(out.status, 0, "{what}");
        let line = out.stdout.strip_suffix('\n').unwrap_or_else(|| panic!("stdout is not one line: {what}"));
        assert!(!line.contains('\n'), "stdout is more than one line: {what}");
        let words = shell_words(line);
        assert!(
            words.len() >= 3 && words[0].ends_with("scripts/cargo.sh") && words[1] == "nextest" && words[2] == "run",
            "not a `scripts/cargo.sh nextest run` command: {what}"
        );
        let mut selection = crates(&[]);
        let mut rest = words[3..].iter();
        while let Some(word) = rest.next() {
            match word.as_str() {
                "--workspace" => selection.workspace = true,
                "-p" => {
                    let name = rest.next().unwrap_or_else(|| panic!("-p without a crate: {what}"));
                    assert!(selection.packages.insert(name.clone()), "-p {name} twice: {what}");
                }
                "--test" => {
                    let name = rest.next().unwrap_or_else(|| panic!("--test without a binary: {what}"));
                    assert!(selection.binaries.insert(name.clone()), "--test {name} twice: {what}");
                }
                "-E" => selection.filters.push(rest.next().unwrap_or_else(|| panic!("-E without a filter: {what}")).clone()),
                other => panic!("unexpected argument {other:?}: {what}"),
            }
        }
        selection
    }

    /// The dry run of a change that reaches no Rust test: exit 0, nothing on stdout, and
    /// `test-changed: nothing to run` on stderr.
    fn assert_nothing_to_run(&self) {
        let out = self.dry_run();
        let what = out.what();
        assert_eq!((out.status, out.stdout.as_str()), (0, ""), "{what}");
        assert!(out.stderr.contains("test-changed: nothing to run"), "{what}");
    }
}

/// Splits a printed shell command into words: blanks separate them, single and double quotes
/// group, a backslash outside single quotes escapes the next character.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => words.extend(word.take()),
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    w.push(q);
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => w.extend(chars.next()),
                        _ => w.push(q),
                    }
                }
            }
            '\\' => word.get_or_insert_with(String::new).extend(chars.next()),
            _ => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    words
}

// Ported from cli/tests/tooling/test_changed_script.rs, name for name.

#[test]
fn a_core_change_runs_core_and_its_reverse_dependencies() {
    let mut s = Scratch::new();
    s.edit("core/src/lib.rs");
    assert_eq!(s.selection(), crates(&["cpd-core", "cpd-tui", "cpd"]));
}

#[test]
fn a_tui_change_runs_tui_and_cli() {
    let mut s = Scratch::new();
    s.edit("tui/src/lib.rs");
    assert_eq!(s.selection(), crates(&["cpd-tui", "cpd"]));
}

#[test]
fn a_cli_source_change_runs_cli_only() {
    let mut s = Scratch::new();
    s.edit("cli/src/main.rs");
    assert_eq!(s.selection(), crates(&["cpd"]));
}

#[test]
fn a_change_to_one_integration_test_module_runs_that_module_in_its_binary_only() {
    let mut s = Scratch::new();
    s.edit("cli/tests/first/alpha.rs");
    assert_eq!(s.selection(), module("first", "alpha"));
}

#[test]
fn a_change_to_an_area_s_main_or_other_file_runs_that_binary_only() {
    for file in ["cli/tests/first/main.rs", "cli/tests/first/data/sample.txt"] {
        let mut s = Scratch::new();
        s.edit(file);
        assert_eq!(s.selection(), binary("first"), "{file}");
    }
}

#[test]
fn a_new_module_file_of_an_area_runs_that_module_only() {
    let mut s = Scratch::new();
    s.untracked("cli/tests/second/delta.rs");
    assert_eq!(s.selection(), module("second", "delta"));
}

#[test]
fn changes_in_two_areas_run_each_with_its_own_filter() {
    let mut s = Scratch::new();
    s.edit("cli/tests/first/alpha.rs");
    s.edit("cli/tests/first/beta.rs");
    s.edit("cli/tests/second/gamma.rs");
    let want = Selection {
        binaries: ["first".to_string(), "second".to_string()].into(),
        filters: ["binary(first) & test(/^alpha::/)", "binary(first) & test(/^beta::/)", "binary(second) & test(/^gamma::/)"]
            .map(String::from)
            .into(),
        ..crates(&["cpd"])
    };
    assert_eq!(s.selection(), want);
}

#[test]
fn a_whole_binary_wins_over_a_module_of_the_same_area_and_keeps_the_other_area_s_module() {
    let mut s = Scratch::new();
    s.edit("cli/tests/first/main.rs");
    s.edit("cli/tests/first/alpha.rs");
    s.edit("cli/tests/second/gamma.rs");
    let want = Selection {
        binaries: ["first".to_string(), "second".to_string()].into(),
        filters: ["binary(first)", "binary(second) & test(/^gamma::/)"].map(String::from).into(),
        ..crates(&["cpd"])
    };
    assert_eq!(s.selection(), want);
}

#[test]
fn a_change_to_the_shared_test_files_runs_every_binary_of_the_cli_crate() {
    let mut s = Scratch::new();
    s.edit("cli/tests/common/mod.rs");
    assert_eq!(s.selection(), crates(&["cpd"]));
}

#[test]
fn a_module_change_beside_a_source_change_runs_the_whole_crate() {
    let mut s = Scratch::new();
    s.edit("cli/tests/first/alpha.rs");
    s.edit("cli/src/main.rs");
    assert_eq!(s.selection(), crates(&["cpd"]));
}

#[test]
fn uncommitted_changes_count_unstaged_staged_and_untracked() {
    // Unstaged and staged changes are both in `git diff <merge base>` (the working tree against the merge base);
    // untracked files are the ones `git ls-files --others --exclude-standard` lists.
    let mut s = Scratch::new();
    s.edit("core/src/lib.rs");
    assert_eq!(s.selection(), crates(&["cpd-core", "cpd-tui", "cpd"]), "unstaged");

    let mut s = Scratch::new();
    s.edit("cli/src/main.rs");
    assert_eq!(s.selection(), crates(&["cpd"]), "staged");

    let mut s = Scratch::new();
    s.untracked("tui/src/extra.rs");
    assert_eq!(s.selection(), crates(&["cpd-tui", "cpd"]), "untracked");
}

#[test]
fn each_root_level_input_falls_back_to_the_full_suite() {
    let inputs = [
        ("the root Cargo.toml", "Cargo.toml"),
        ("Cargo.lock", "Cargo.lock"),
        ("rust-toolchain.toml", "rust-toolchain.toml"),
        ("a build.rs", "cli/build.rs"),
        (".config/", ".config/nextest.toml"),
        ("a migration", "core/src/store/migrations/0002-next.sql"),
        ("scripts/cargo.sh", "scripts/cargo.sh"),
    ];
    for (input, file) in inputs {
        let mut s = Scratch::new();
        s.edit(file);
        assert_eq!(s.selection(), full_suite(), "{input}");
    }
}

#[test]
fn no_change_runs_nothing_and_says_so() {
    let s = Scratch::new();
    s.assert_nothing_to_run();
}

#[test]
fn a_change_outside_every_crate_runs_nothing() {
    let mut s = Scratch::new();
    s.edit("docs/notes.md");
    s.assert_nothing_to_run();
}

// New: what the in-process logic asks of git and cargo, which the exec tests could not see.

#[test]
fn the_changes_are_those_between_the_merge_base_of_the_base_and_head_and_the_working_tree_plus_untracked_files() {
    let mut s = Scratch::new();
    s.edit("cli/src/main.rs");
    let out = s.dry_run();
    let what = out.what();
    let merge_base = out.git("merge-base");
    assert!(merge_base.args.contains(&BASE.to_owned()) && merge_base.args.contains(&"HEAD".to_owned()), "{what}");
    let diff = out.git("diff");
    assert!(diff.args.contains(&"--name-only".to_owned()) && diff.args.contains(&MERGE_BASE.to_owned()), "{what}");
    assert!(!diff.args.contains(&"--cached".to_owned()), "the diff is of the working tree, not the index: {what}");
    let others = out.git("ls-files");
    assert!(others.args.contains(&"--others".to_owned()) && others.args.contains(&"--exclude-standard".to_owned()), "{what}");
    assert!(out.unscripted.is_empty(), "{what}");
}

#[test]
fn the_base_defaults_to_origin_main() {
    let mut s = Scratch::new();
    s.edit("cli/src/main.rs");
    let out = s.changed(&s.root, true, None, Reply::ok(""));
    assert_eq!(out.status, 0, "{}", out.what());
    assert!(out.git("merge-base").args.contains(&"origin/main".to_owned()), "{}", out.what());
}

#[test]
fn a_base_with_no_merge_base_with_head_is_exit_2_and_says_so() {
    let s = Scratch::new();
    let fakes = Fakes::new();
    let _ = (*fakes.runner)
        .clone()
        .on("git", has("rev-parse"), Reply::ok(&format!("{}\n", s.root.display())))
        .on("git", has("merge-base"), Reply::exit(1, "", ""));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ChangedArgs { dry_run: true, base: Some("nowhere".to_owned()) };
    let result = changed(&fakes.ports, &s.root, &args, &mut out, &mut err);
    let out = Out::of(result, out, err, &fakes);
    assert_eq!(out.status, 2, "{}", out.what());
    assert!(out.stderr.contains("test-changed: no merge base between nowhere and HEAD"), "{}", out.what());
    assert_eq!(out.stdout, "", "{}", out.what());
}

#[test]
fn a_dry_run_runs_no_test_and_asks_cargo_metadata_once_from_the_repo_root() {
    let mut s = Scratch::new();
    s.edit("core/src/lib.rs");
    let out = s.dry_run();
    assert!(out.cargo("nextest").is_empty(), "{}", out.what());
    let metadata = out.cargo("metadata");
    assert_eq!(metadata.len(), 1, "{}", out.what());
    assert_eq!(words(metadata[0]), ["metadata", "--format-version", "1", "--no-deps"], "{}", out.what());
    assert_eq!(metadata[0].cwd.as_deref(), Some(s.root.as_path()), "{}", out.what());
}

#[test]
fn the_full_suite_fallback_prints_exactly_the_workspace_command() {
    let mut s = Scratch::new();
    s.edit("Cargo.toml");
    let out = s.dry_run();
    assert_eq!((out.status, out.stdout.as_str()), (0, "scripts/cargo.sh nextest run --workspace\n"), "{}", out.what());
}

#[test]
fn without_a_dry_run_the_selected_command_runs_from_the_repo_root_and_its_status_is_nextest_s() {
    let mut s = Scratch::new();
    s.edit("cli/src/main.rs");
    let green = s.changed(&s.root, false, Some(BASE), Reply::exit(0, "ok\n", ""));
    assert_eq!(green.status, 0, "{}", green.what());
    let red = s.changed(&s.root, false, Some(BASE), Reply::exit(100, "", "1 test failed\n"));
    assert_eq!(red.status, 100, "{}", red.what());
    for out in [&green, &red] {
        let runs = out.cargo("nextest");
        assert_eq!(runs.len(), 1, "{}", out.what());
        assert!(runs[0].program.ends_with("scripts/cargo.sh"), "{}", out.what());
        assert_eq!(words(runs[0]), ["nextest", "run", "-p", "cpd"], "{}", out.what());
        assert_eq!(runs[0].cwd.as_deref(), Some(s.root.as_path()), "{}", out.what());
    }
}

#[test]
fn nothing_selected_runs_no_nextest_and_exits_0() {
    let mut s = Scratch::new();
    s.edit("docs/notes.md");
    let out = s.changed(&s.root, false, Some(BASE), Reply::exit(1, "", ""));
    assert_eq!(out.status, 0, "{}", out.what());
    assert!(out.stderr.contains("test-changed: nothing to run"), "{}", out.what());
    assert!(out.cargo("nextest").is_empty(), "{}", out.what());
}

#[test]
fn it_runs_from_the_repo_root_whatever_the_current_directory() {
    let mut s = Scratch::new();
    s.edit("core/src/lib.rs");
    let deep = s.root.join("cli/src");
    let out = s.changed(&deep, false, Some(BASE), Reply::ok(""));
    assert_eq!(out.status, 0, "{}", out.what());
    assert_eq!(out.git("rev-parse").cwd.as_deref(), Some(deep.as_path()), "{}", out.what());
    for call in out.cargo("metadata").into_iter().chain(out.cargo("nextest")) {
        assert_eq!(call.cwd.as_deref(), Some(s.root.as_path()), "{}", out.what());
    }
    assert_eq!(out.cargo("nextest").len(), 1, "{}", out.what());
}

// ---- `cpd test run` (and `build`, `list`) ----------------------------------------------------------------------------

const PI_TESTS: &str = r#"import { test } from "node:test";
import assert from "node:assert/strict";

test("passes", () => {});
test("fails", () => { assert.fail("red"); });
test("a.c (1+1) [x]", () => {});
test("abc", () => { assert.fail("the regex characters matched too much"); });
test("a.c (1+1) [x] more", () => { assert.fail("a prefix matched"); });
"#;

/// A second pi test file, with titles in `it(` calls and in each of the three quotes.
const OTHER_PI_TESTS: &str = "it('single', () => {});\nit(\"double\", () => {});\nit(`tick`, () => {});\n";

const PI: &str = "pi/test/sample.test.ts";
const OTHER_PI: &str = "pi/test/other.test.ts";

fn pi(title: &str) -> String {
    format!("{PI}::{title}")
}

/// `--test-name-pattern=` for `title`: anchored, and every regex character escaped.
fn pattern(title: &str) -> String {
    let mut text = String::from("--test-name-pattern=^");
    for c in title.chars() {
        if "[]\\.*+?^$(){}|".contains(c) {
            text.push('\\');
        }
        text.push(c);
    }
    text.push('$');
    text
}

/// A scratch copy of the repo's test files: two pi test files under a temp root, which the fakes name as git's top.
struct Sample {
    _dir: TempDir,
    root: PathBuf,
}

impl Sample {
    fn new() -> Sample {
        let (dir, root) = scratch_dir();
        write(&root, PI, PI_TESTS);
        write(&root, OTHER_PI, OTHER_PI_TESTS);
        Sample { _dir: dir, root }
    }

    /// The fakes: `scripted` adds its rules first (a first match answers); then git's top is the root, and nextest,
    /// node and pnpm succeed, `nextest list` finds no test.
    fn fakes(&self, scripted: impl FnOnce(FakeRunner) -> FakeRunner) -> Fakes {
        let fakes = Fakes::new();
        let _ = scripted((*fakes.runner).clone())
            .on("git", has("rev-parse"), Reply::ok(&format!("{}\n", self.root.display())))
            .on("cargo.sh", |c| c.args.starts_with(&owned(&["nextest", "list"])), Reply::ok(""))
            .on("cargo.sh", first("nextest"), Reply::ok(""))
            .on("node", has("--test"), Reply::ok(""))
            .on("pnpm", has("typecheck"), Reply::ok(""));
        fakes
    }

    /// `cpd test run <names>`, from a directory that is not the root: `<root>/pi`.
    fn run(&self, fakes: &Fakes, names: &[&str]) -> Out {
        let names = owned(names);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = run(&fakes.ports, &self.root.join("pi"), &names, &mut out, &mut err);
        Out::of(result, out, err, fakes)
    }

    fn list(&self, fakes: &Fakes, name: &str) -> Out {
        let mut err = Vec::new();
        let result = list(&fakes.ports, &self.root.join("pi"), name, &mut err);
        Out::of(result, Vec::new(), err, fakes)
    }

    fn build(&self, fakes: &Fakes) -> Out {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = build(&fakes.ports, &self.root, &mut out, &mut err);
        Out::of(result, out, err, fakes)
    }
}

/// The node calls: `node --test <patterns…> <file>`.
fn node_calls(out: &Out) -> Vec<Vec<&str>> {
    out.calls_of("node").into_iter().map(words).collect()
}

// Ported from cli/tests/tooling/tests_script.rs, name for name.

#[test]
fn a_rust_name_and_a_pi_name_each_run_and_pass() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r.on("node", has("--test"), Reply::ok("✔ passes (0.4ms)\n")));
    let rust = s.run(&fakes, &["green_one"]);
    assert_eq!(rust.status, 0, "{}", rust.what());
    let both = s.run(&fakes, &["green_one", &pi("passes")]);
    assert_eq!(both.status, 0, "{}", both.what());
    assert!(both.stdout.contains("passes"), "{}", both.what());
    let fakes = s.fakes(|r| r.on("cargo.sh", has("green_one"), Reply::ok("crate1::sample green_one\n")));
    assert_eq!(s.list(&fakes, "green_one").status, 0);
    assert_eq!(s.list(&fakes, &pi("passes")).status, 0);
}

#[test]
fn a_failing_test_makes_run_exit_non_zero() {
    let s = Sample::new();
    let fakes = s.fakes(|r| {
        r.on("node", has(pattern("fails")), Reply::exit(1, "✖ fails\n", ""))
            .on("cargo.sh", has("red_one"), Reply::exit(100, "", "red_one failed\n"))
    });
    let out = s.run(&fakes, &[&pi("fails")]);
    assert_ne!(out.status, 0, "{}", out.what());
    let mixed = s.run(&fakes, &[&pi("passes"), &pi("fails")]);
    assert_ne!(mixed.status, 0, "{}", mixed.what());
    let rust = s.run(&fakes, &["red_one"]);
    assert_ne!(rust.status, 0, "{}", rust.what());
    let both = s.run(&fakes, &["green_one", &pi("fails")]);
    assert_ne!(both.status, 0, "{}", both.what());
}

#[test]
fn a_misspelled_name_makes_list_and_run_exit_non_zero() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    assert_ne!(s.list(&fakes, &pi("pases")).status, 0);
    assert_ne!(s.list(&fakes, "pi/test/missing.test.ts::passes").status, 0);
    assert_ne!(s.list(&fakes, "gren_one").status, 0);
    // A title that is only a prefix of a test's title is no test either.
    assert_ne!(s.list(&fakes, &pi("pass")).status, 0);
    let out = s.run(&fakes, &[&pi("pases")]);
    assert_ne!(out.status, 0, "a title that matches no test must not read as a pass: {}", out.what());
    assert!(node_calls(&out).is_empty(), "a title that is in no test is not handed to node: {}", out.what());
}

#[test]
fn a_pi_title_with_regex_characters_runs_exactly_that_test() {
    let s = Sample::new();
    let title = "a.c (1+1) [x]";
    let fakes = s.fakes(|r| r.on("node", has("--test"), Reply::ok("✔ a.c (1+1) [x] (0.3ms)\n")));
    let out = s.run(&fakes, &[&pi(title)]);
    assert_eq!(out.status, 0, "{}", out.what());
    assert!(out.stdout.contains(title), "{}", out.what());
    assert!(!out.stdout.contains("abc"), "{}", out.what());
    assert!(!out.stdout.contains("more"), "{}", out.what());
    assert_eq!(node_calls(&out), [vec!["--test", r"--test-name-pattern=^a\.c \(1\+1\) \[x\]$", PI]], "{}", out.what());
    assert_eq!(s.list(&fakes, &pi(title)).status, 0);
}

#[test]
fn build_passes_when_the_typescript_typechecks_and_fails_when_it_does_not() {
    let s = Sample::new();
    let ok = s.build(&s.fakes(|r| r));
    assert_eq!(ok.status, 0, "{}", ok.what());
    let steps: Vec<Vec<&str>> = ok.calls.iter().filter(|c| !c.program.ends_with("git")).map(words).collect();
    assert_eq!(steps, [vec!["nextest", "run", "--workspace", "--no-run"], vec!["run", "typecheck"]], "{}", ok.what());
    assert_eq!(ok.calls_of("pnpm").len(), 1, "the typecheck is a pnpm call: {}", ok.what());
    assert!(ok.calls_of("npm").is_empty(), "no npm call: {}", ok.what());
    let bad = s.build(&s.fakes(|r| {
        r.on("pnpm", has("typecheck"), Reply::exit(2, "pi/bad.ts(1,14): error TS2322: Type 'string' is not assignable to type 'number'.\n", ""))
    }));
    assert_ne!(bad.status, 0, "{}", bad.what());
    assert!(bad.stdout.contains("TS2322") || bad.stderr.contains("TS2322"), "{}", bad.what());
}

// New: the argv of the calls and the classification of a name.

#[test]
fn rust_names_run_in_one_nextest_call_from_the_repo_root_whatever_the_cwd() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.run(&fakes, &["green_one", "watch::e4_adds_an_open_job"]);
    assert_eq!(out.status, 0, "{}", out.what());
    let runs = out.cargo("nextest");
    assert_eq!(runs.len(), 1, "{}", out.what());
    assert!(runs[0].program.ends_with("scripts/cargo.sh"), "{}", out.what());
    assert_eq!(words(runs[0]), ["nextest", "run", "--workspace", "--no-fail-fast", "green_one", "watch::e4_adds_an_open_job"], "{}", out.what());
    assert_eq!(runs[0].cwd.as_deref(), Some(s.root.as_path()), "{}", out.what());
    assert!(node_calls(&out).is_empty(), "a Rust name never reaches node: {}", out.what());
}

#[test]
fn a_pi_name_runs_in_node_from_the_repo_root_and_never_in_nextest() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.run(&fakes, &[&pi("passes")]);
    assert_eq!(out.status, 0, "{}", out.what());
    assert_eq!(node_calls(&out), [vec!["--test", "--test-name-pattern=^passes$", PI]], "{}", out.what());
    assert_eq!(out.calls_of("node")[0].cwd.as_deref(), Some(s.root.as_path()), "{}", out.what());
    assert!(out.cargo("nextest").is_empty(), "a pi name never reaches nextest: {}", out.what());
}

#[test]
fn the_titles_of_one_pi_file_run_in_one_node_call_and_each_file_in_its_own() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.run(&fakes, &[&pi("passes"), &format!("{OTHER_PI}::double"), &pi("a.c (1+1) [x]"), &format!("{OTHER_PI}::tick")]);
    assert_eq!(out.status, 0, "{}", out.what());
    let calls = node_calls(&out);
    assert_eq!(calls.len(), 2, "{}", out.what());
    let of = |file: &str| calls.iter().find(|c| c.last() == Some(&file)).unwrap_or_else(|| panic!("no node call for {file}: {}", out.what())).clone();
    assert_eq!(of(PI), ["--test", "--test-name-pattern=^passes$", r"--test-name-pattern=^a\.c \(1\+1\) \[x\]$", PI], "{}", out.what());
    assert_eq!(of(OTHER_PI), ["--test", "--test-name-pattern=^double$", "--test-name-pattern=^tick$", OTHER_PI], "{}", out.what());
}

#[test]
fn a_pi_title_is_found_in_a_test_or_an_it_call_in_any_of_the_three_quotes() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    for title in ["single", "double", "tick"] {
        assert_eq!(s.list(&fakes, &format!("{OTHER_PI}::{title}")).status, 0, "{title}");
    }
    assert_eq!(s.list(&fakes, &pi("passes")).status, 0);
    assert!(fakes.runner.calls().iter().all(|c| c.program.rsplit('/').next() != Some("node")), "listing a pi title starts no node");
}

#[test]
fn a_path_with_no_title_is_a_rust_name_and_so_is_one_with_an_empty_title() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.run(&fakes, &[PI, &format!("{PI}::")]);
    assert_eq!(out.status, 0, "{}", out.what());
    assert!(node_calls(&out).is_empty(), "{}", out.what());
    let runs = out.cargo("nextest");
    assert_eq!(runs.len(), 1, "{}", out.what());
    assert_eq!(words(runs[0])[4..], [PI.to_owned(), format!("{PI}::")], "{}", out.what());
}

#[test]
fn a_failed_rust_run_still_runs_the_pi_tests_and_run_exits_non_zero() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r.on("cargo.sh", has("red_one"), Reply::exit(100, "", "red_one failed\n")));
    let out = s.run(&fakes, &["red_one", &pi("passes")]);
    assert_ne!(out.status, 0, "{}", out.what());
    assert_eq!(node_calls(&out).len(), 1, "{}", out.what());
}

#[test]
fn a_node_failure_is_not_forgotten_by_a_later_pass() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r.on("node", has(OTHER_PI), Reply::exit(1, "", "double failed\n")));
    let out = s.run(&fakes, &[&format!("{OTHER_PI}::double"), &pi("passes")]);
    assert_ne!(out.status, 0, "{}", out.what());
    assert_eq!(node_calls(&out).len(), 2, "both files run: {}", out.what());
}

#[test]
fn a_run_with_no_name_is_a_usage_error() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.run(&fakes, &[]);
    assert_eq!(out.status, 2, "{}", out.what());
    assert!(out.calls_of("node").is_empty() && out.cargo("nextest").is_empty(), "{}", out.what());
}

#[test]
fn a_rust_list_asks_nextest_for_exactly_that_name_and_is_red_when_it_finds_none() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r.on("cargo.sh", has("green_one"), Reply::ok("crate1::sample green_one\n")));
    assert_eq!(s.list(&fakes, "green_one").status, 0);
    assert_ne!(s.list(&fakes, "gren_one").status, 0);
    let listed: Vec<Vec<String>> = fakes.runner.calls().iter().filter(|c| c.args.first().map(String::as_str) == Some("nextest")).map(|c| c.args.clone()).collect();
    let line = |name: &str| owned(&["nextest", "list", "--workspace", "--message-format", "oneline", name]);
    assert_eq!(listed, [line("green_one"), line("gren_one")]);
}

#[test]
fn a_failed_build_stops_before_the_typecheck() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r.on("cargo.sh", has("--no-run"), Reply::exit(101, "", "error[E0425]: cannot find value\n")));
    let out = s.build(&fakes);
    assert_ne!(out.status, 0, "{}", out.what());
    assert!(out.calls_of("pnpm").is_empty(), "{}", out.what());
}

#[test]
fn nothing_these_tests_ask_for_is_left_unscripted() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.run(&fakes, &["green_one", &pi("passes")]);
    assert_eq!(out.status, 0, "{}", out.what());
    assert!(out.unscripted.is_empty(), "{}", out.what());
}

// ---- web test names: the classifier and the web dispatch (testsel) ---------------------------------------------------

/// A web name `web/<file>.test.ts(x)::<title>` classifies as a web name, not a pi name, and is not refused.
#[test]
fn classify_web_name() {
    assert_eq!(
        kind_of("web/sample.test.tsx::renders a button"),
        Kind::Web { file: "web/sample.test.tsx".into(), title: "renders a button".into() },
        "a tsx web name is a web name"
    );
    assert!(pi_name("web/sample.test.tsx::renders a button").is_none(), "a web name is not a pi name");
}

/// A web name runs through the web package's vitest, filtered by the exact title.
#[test]
fn dispatch_web_name_runs_vitest() {
    let s = Sample::new();
    let fakes = s.fakes(|r| {
        r.on("pnpm", |c| c.args.iter().any(|a| a == "vitest"), Reply::ok("✓ renders a button (2ms)\n"))
    });
    let out = s.run(&fakes, &["web/sample.test.tsx::renders a button"]);
    let vitest: Vec<Vec<&str>> = out
        .calls_of("pnpm")
        .into_iter()
        .filter(|c| c.args.iter().any(|a| a == "vitest"))
        .map(words)
        .collect();
    assert_eq!(
        vitest,
        [vec!["--filter", "cpd-web-client", "exec", "vitest", "run", "--testNamePattern", "renders a button", "web/sample.test.tsx"]],
        "the web dispatch is the package's vitest, filtered by the title: {}",
        out.what()
    );
    assert!(out.cargo("nextest").is_empty(), "a web name never reaches nextest: {}", out.what());
    assert!(node_calls(&out).is_empty(), "a web name never reaches node: {}", out.what());
}

/// A web title the web package does not contain is refused (`list` exits non-zero and starts no Rust or node run).
#[test]
fn list_web_unknown_title() {
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.list(&fakes, "web/sample.test.tsx::no such test");
    assert_ne!(out.status, 0, "a title the web package lacks is refused: {}", out.what());
    assert!(out.cargo("nextest").is_empty(), "a web list never reaches nextest: {}", out.what());
    assert!(out.calls_of("node").is_empty(), "a web list never reaches node: {}", out.what());
}

/// `test build` typechecks the web client only when `web/package.json` exists; without it the web client is skipped.
#[test]
fn build_skips_web_when_absent() {
    let web = |c: &Call| c.args.iter().any(|a| a == "--filter");
    // Without `web/` the web client is not typechecked.
    let s = Sample::new();
    let fakes = s.fakes(|r| r);
    let out = s.build(&fakes);
    assert_eq!(out.status, 0, "{}", out.what());
    let absent: Vec<_> = out.calls_of("pnpm").into_iter().filter(|c| web(c)).collect();
    assert!(absent.is_empty(), "no web client typecheck without web/: {}", out.what());

    // With `web/package.json` the web client is typechecked.
    let s = Sample::new();
    write(&s.root, "web/package.json", "{\"name\":\"cpd-web-client\"}\n");
    let fakes = s.fakes(|r| r.on("pnpm", web, Reply::ok("")));
    let out = s.build(&fakes);
    let present: Vec<Vec<&str>> = out.calls_of("pnpm").into_iter().filter(|c| web(c)).map(words).collect();
    assert!(!present.is_empty(), "the web client is typechecked when web/ exists: {}", out.what());
}

/// Pi and Rust names keep their current classification once web names exist.
#[test]
fn pi_and_rust_names_unchanged() {
    assert_eq!(kind_of("pi/test/sample.test.ts::passes"), Kind::Pi { file: PI.into(), title: "passes".into() }, "a pi name stays a pi name");
    assert_eq!(kind_of("green_one"), Kind::Rust, "a Rust name stays a Rust name");
}
