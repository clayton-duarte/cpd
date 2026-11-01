//! `scripts/repin.sh` (contract §4.3; design `docs/research/schema-pin.md` §2.4, §3).
//!
//! The script writes `SCHEMA_HASH` in `core/src/store/schema.rs` and resolves a
//! merge conflict on it. The four contracts pinned here:
//!
//! 1. a merge that conflicts only on `SCHEMA_HASH` is resolved, and the file holds the value the
//!    print command gives (and the file is staged);
//! 2. a conflict on `SCHEMA_HASH` plus another hunk of `schema.rs` is refused (non-zero exit,
//!    stderr names `schema.rs`), the other hunk is left exactly as git wrote it, and the file is
//!    not staged;
//! 3. with no conflict, the script only rewrites a stale value, on that one line;
//! 4. rerunning changes nothing: the file and the index are the same after a second run.
//!
//! **The one knob.** The script takes the command that prints the hash from the environment
//! variable `REPIN_HASH_COMMAND` (run with `sh -c`, from the repository root; the hash is the
//! value of its line `SCHEMA_HASH=<hex>`, other lines are ignored). Its default is the print test
//! (an `#[ignore]` test of `core`, run with `scripts/cargo.sh`). The knob exists for these tests
//! only: they run the script in a scratch clone with no Rust workspace, and a nested `cargo
//! build` of the real one would take half a minute per test (design §2.4). So each test sets the
//! variable to a `printf` of a known value, and "the value the print command gives" is that
//! value. The script reads the file at `core/src/store/schema.rs` of the current directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SCHEMA_RS: &str = "core/src/store/schema.rs";

/// The text of `schema.rs` on the merge base: three regions, each apart from the others by
/// unchanged lines so that git reports a change to each as its own hunk.
fn schema_rs(one: &str, hash: &str, two: &str, three: &str) -> String {
    format!(
        "{one}\n\nfn a() {{}}\nfn b() {{}}\nfn c() {{}}\n\n\
         pub const SCHEMA_HASH: &str = \"{hash}\";\n\n\
         fn d() {{}}\nfn e() {{}}\nfn f() {{}}\n\n{two}\n\n\
         fn g() {{}}\nfn h() {{}}\nfn i() {{}}\n\n{three}\n"
    )
}

fn hash(c: char) -> String {
    c.to_string().repeat(64)
}

const BASE: [&str; 3] = ["// one: base", "fn two() -> i32 { 1 }", "// three: base"];

fn base_text() -> String {
    schema_rs(BASE[0], &hash('0'), BASE[1], BASE[2])
}

/// The value the print command gives.
fn fresh() -> String {
    hash('f')
}

/// A scratch git repository with `schema.rs` on `main`.
struct Repo {
    dir: tempfile::TempDir,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core/ has a parent")
        .to_owned()
}

/// An environment in which `git` ignores the user's configuration.
fn git_env(home: &Path) -> Vec<(&'static str, String)> {
    let home = home.display().to_string();
    vec![
        ("HOME", home.clone()),
        ("XDG_CONFIG_HOME", home),
        ("GIT_CONFIG_NOSYSTEM", "1".to_owned()),
        ("GIT_AUTHOR_NAME", "t".to_owned()),
        ("GIT_AUTHOR_EMAIL", "t@t".to_owned()),
        ("GIT_COMMITTER_NAME", "t".to_owned()),
        ("GIT_COMMITTER_EMAIL", "t@t".to_owned()),
    ]
}

impl Repo {
    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn file(&self) -> PathBuf {
        self.work().join(SCHEMA_RS)
    }

    fn read(&self) -> String {
        fs::read_to_string(self.file()).unwrap()
    }

    fn write(&self, text: &str) {
        fs::write(self.file(), text).unwrap();
    }

    fn git_output(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(self.work())
            .envs(git_env(self.dir.path()))
            .output()
            .unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.git_output(args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn commit(&self, text: &str) {
        self.write(text);
        self.git(&["commit", "--quiet", "-am", "change"]);
    }

    /// `main` holds [`base_text`] and one unrelated file.
    fn new() -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo { dir };
        fs::create_dir_all(repo.file().parent().unwrap()).unwrap();
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo.write(&base_text());
        fs::write(repo.work().join("README"), "readme\n").unwrap();
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "--quiet", "-m", "main"]);
        repo
    }

    /// Branch `theirs` and branch `ours` (checked out) both leave `main`, and `ours` then merges
    /// `theirs`. Each side sets its own hash and edits its own region of the file; `ours_two` and
    /// `theirs_two` are the text of the middle region on each side. Returns once the merge has
    /// stopped on a conflict.
    fn conflicted(ours_two: &str, theirs_two: &str) -> Repo {
        let repo = Repo::new();
        repo.git(&["checkout", "--quiet", "-b", "theirs"]);
        repo.commit(&schema_rs(
            "// one: theirs",
            &hash('b'),
            theirs_two,
            BASE[2],
        ));
        repo.git(&["checkout", "--quiet", "-b", "ours", "main"]);
        repo.commit(&schema_rs(BASE[0], &hash('a'), ours_two, "// three: ours"));
        let merge = repo.git_output(&["merge", "--no-edit", "theirs"]);
        assert_eq!(
            merge.status.code(),
            Some(1),
            "the merge must stop on a conflict"
        );
        repo
    }

    /// Runs `scripts/repin.sh` from the repository root, the print command printing [`fresh`].
    fn repin(&self) -> Output {
        let command = format!(
            "printf 'warning: noise\\nSCHEMA_HASH=%s\\ntest result: ok\\n' {}",
            fresh()
        );
        Command::new(repo_root().join("scripts/repin.sh"))
            .current_dir(self.work())
            .envs(git_env(self.dir.path()))
            .env("REPIN_HASH_COMMAND", command)
            .output()
            .unwrap()
    }

    /// Runs the script and asserts that it succeeds.
    fn repin_ok(&self) {
        let out = self.repin();
        assert!(
            out.status.success(),
            "repin.sh failed ({:?}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Paths with an unmerged (conflicted) index entry.
    fn unmerged(&self) -> String {
        self.git(&["ls-files", "--unmerged"])
    }

    /// `git status --porcelain`: the index and the work tree, as one text.
    fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }
}

#[test]
fn a_merge_that_conflicts_only_on_the_hash_is_resolved_to_the_value_the_print_command_gives() {
    // The two sides edit the same hash line, and nothing else in common.
    let repo = Repo::conflicted(BASE[1], BASE[1]);
    assert!(
        repo.read().contains("<<<<<<<"),
        "the scratch merge must conflict on the hash"
    );
    assert_eq!(
        repo.read().matches("<<<<<<<").count(),
        1,
        "and on the hash only"
    );

    repo.repin_ok();

    // Both sides' other edits are kept, the hash is the fresh one, and no marker remains.
    let want = schema_rs("// one: theirs", &fresh(), BASE[1], "// three: ours");
    assert_eq!(repo.read(), want);
    assert_eq!(repo.unmerged(), "", "the file is staged as resolved");
    assert_eq!(
        repo.git(&["diff", "--name-only"]),
        "",
        "and nothing of it is left unstaged"
    );
}

#[test]
fn a_conflict_on_the_hash_and_another_hunk_is_refused_and_the_other_hunk_is_left_as_it_was() {
    let repo = Repo::conflicted("fn two() -> i32 { 2 }", "fn two() -> i32 { 3 }");
    // How git writes the hunk that is not the hash's.
    let other =
        "<<<<<<< HEAD\nfn two() -> i32 { 2 }\n=======\nfn two() -> i32 { 3 }\n>>>>>>> theirs\n";
    assert!(
        repo.read().contains(other),
        "the scratch merge must conflict on the other hunk"
    );
    assert_eq!(repo.read().matches("<<<<<<<").count(), 2, "and on the hash");

    let out = repo.repin();

    assert!(!out.status.success(), "the script must refuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("schema.rs"),
        "the refusal names the file that still conflicts: {stderr:?}"
    );
    assert!(
        repo.read().contains(other),
        "the other hunk is left exactly as git wrote it:\n{}",
        repo.read()
    );
    assert_ne!(repo.unmerged(), "", "the file is not staged as resolved");
}

#[test]
fn without_a_conflict_the_script_only_rewrites_a_stale_value() {
    let repo = Repo::new();
    assert!(!base_text().contains(&fresh()));

    repo.repin_ok();

    assert_eq!(repo.read(), base_text().replace(&hash('0'), &fresh()));
    assert_eq!(
        repo.git(&["diff", "--name-only"]),
        SCHEMA_RS.to_owned() + "\n",
        "no other file changes"
    );
}

#[test]
fn rerunning_changes_nothing() {
    // After a resolved conflict.
    let repo = Repo::conflicted(BASE[1], BASE[1]);
    repo.repin_ok();
    let (text, status) = (repo.read(), repo.status());
    assert_eq!(
        text,
        schema_rs("// one: theirs", &fresh(), BASE[1], "// three: ours"),
        "the first run resolves"
    );
    repo.repin_ok();
    assert_eq!((repo.read(), repo.status()), (text, status));

    // After a rewritten stale value; and on a value that is already fresh.
    let repo = Repo::new();
    repo.repin_ok();
    let (text, status) = (repo.read(), repo.status());
    assert_eq!(
        text,
        base_text().replace(&hash('0'), &fresh()),
        "the first run rewrites"
    );
    repo.repin_ok();
    assert_eq!((repo.read(), repo.status()), (text, status));
}
