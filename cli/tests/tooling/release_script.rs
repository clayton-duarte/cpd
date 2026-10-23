//! `scripts/release.sh`: the release PR (`<version>`) and the tag after the squash merge
//! (`tag <version>`), against a temporary bare origin with `cargo` (for `cargo release`),
//! `git-cliff`, `gh` and `npm` mocked on `PATH` (hs-d27).

use crate::common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use common::{TestEnv, output};

const SCRIPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts");

struct Fixture {
    env: TestEnv,
    origin: PathBuf,
    /// The clone whose `scripts/release.sh` runs: its `origin` is the bare repo.
    clone: PathBuf,
    /// A second clone, where the test commits to main as the merge button would.
    work: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let env = TestEnv::new();
        let origin = env.root().join("repos/origin.git");
        env.git(env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        let work = env.root().join("repos/work");
        env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), work.to_str().unwrap()]);
        fs::create_dir_all(work.join("scripts")).unwrap();
        for script in ["release.sh", "cargo.sh"] {
            fs::copy(Path::new(SCRIPTS).join(script), work.join("scripts").join(script)).unwrap();
        }
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../release.toml"), work.join("release.toml")).unwrap();
        fs::write(work.join("Cargo.toml"), "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.0.0\"\npublish = false\n").unwrap();
        fs::write(work.join("Cargo.lock"), "version 0.0.0\n").unwrap();
        fs::write(work.join("CHANGELOG.md"), "# Changelog\n\n").unwrap();
        env.git(&work, &["add", "-A"]);
        env.git(&work, &["commit", "-q", "-m", "Start (#1)"]);
        env.git(&work, &["push", "-q", "origin", "main"]);
        let clone = env.root().join("repos/clone");
        env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
        let fx = Fixture { env, origin, clone, work };
        fx.mock_tools();
        fx
    }

    /// `cargo release --version` succeeds; `cargo release <v> ...` bumps `Cargo.toml` and
    /// `Cargo.lock`, runs release.toml's `pre-release-hook` (if any) once per workspace member
    /// (core, cli, tui) like the real cargo-release, with `WORKSPACE_ROOT` set, and commits
    /// `Release <v>`. `git-cliff --prepend <file> --tag v<x>` prepends one section to the file.
    fn mock_tools(&self) {
        self.env.fake(
            "cargo",
            r#"[ "$1" = release ] || exit 97
[ "$2" = --version ] && exit 0
sed "s/^version = .*/version = \"$2\"/" Cargo.toml > Cargo.toml.new && mv Cargo.toml.new Cargo.toml
echo "version $2" > Cargo.lock
hook=$(sed -n 's/^pre-release-hook = \["sh", "-c", "\(.*\)"\]$/\1/p' release.toml | sed 's/\\"/"/g')
if [ -n "$hook" ]; then
  hook=$(printf '%s' "$hook" | sed "s/{{version}}/$2/g")
  for _member in core cli tui; do
    WORKSPACE_ROOT="$PWD" sh -c "$hook" || exit 1
  done
fi
git add -A
git commit -q -m "Release $2""#,
        );
        self.env.fake(
            "git-cliff",
            r#"while [ $# -gt 0 ]; do
  case "$1" in
    --tag) tag=$2; shift;;
    --prepend) file=$2; shift;;
  esac
  shift
done
[ -n "${file:-}" ] || exit 0
printf '# Changelog\n\n## [%s] - 2026-01-01\n\n- A change (#2)\n\n' "${tag#v}" > "$file.new"
tail -n +3 "$file" >> "$file.new"
mv "$file.new" "$file""#,
        );
        self.env.fake("npm", "exit 0");
        self.env.fake_gh("echo https://github.com/o/r/pull/9");
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        self.run_path(args, None)
    }

    fn run_path(&self, args: &[&str], path: Option<&std::ffi::OsStr>) -> (i32, String, String) {
        let mut cmd = self.env.command("sh");
        cmd.env_remove("CARGO").arg(self.clone.join("scripts/release.sh")).args(args).current_dir(&self.clone);
        if let Some(path) = path {
            cmd.env("PATH", path);
        }
        let out = output(&mut cmd).unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    }

    /// Commits `subject` to origin's main with `Cargo.toml` at `version`, like a squash merge.
    fn squash(&self, subject: &str, version: &str) -> String {
        self.env.git(&self.work, &["pull", "-q", "origin", "main"]);
        let text = fs::read_to_string(self.work.join("Cargo.toml")).unwrap();
        let text: String = text.lines().map(|l| if l.starts_with("version = ") { format!("version = \"{version}\"") } else { l.to_owned() } + "\n").collect();
        fs::write(self.work.join("Cargo.toml"), text).unwrap();
        self.env.git(&self.work, &["commit", "-q", "--allow-empty", "-am", subject]);
        self.env.git(&self.work, &["push", "-q", "origin", "main"]);
        self.env.git(&self.work, &["rev-parse", "HEAD"])
    }

    fn origin_git(&self, args: &[&str]) -> String {
        self.env.git(&self.origin, args)
    }

    /// Asserts a refusal: exit 1, `needle` on stderr, nothing pushed, no `gh`, no leftover worktree.
    fn refused(&self, args: &[&str], needle: &str) {
        let (exit, stdout, stderr) = self.run(args);
        self.assert_refused((exit, stdout, stderr), needle);
    }

    fn assert_refused(&self, (exit, stdout, stderr): (i32, String, String), needle: &str) {
        assert_eq!(exit, 1, "{stdout}{stderr}");
        assert!(stderr.contains(needle), "{stderr}");
        assert_eq!(self.origin_git(&["branch", "--format=%(refname:short)"]), "main");
        assert!(self.env.calls("gh").is_empty());
        assert_eq!(self.env.git(&self.clone, &["worktree", "list"]).lines().count(), 1);
    }
}

#[test]
fn release_opens_the_pr_with_one_release_commit() {
    let fx = Fixture::new();
    let (exit, stdout, stderr) = fx.run(&["0.0.5"]);
    assert_eq!(exit, 0, "{stdout}{stderr}");
    assert!(stdout.contains("https://github.com/o/r/pull/9"), "{stdout}");

    // One commit "Release 0.0.5" on release/v0.0.5, on top of origin's main, with the three files.
    let main = fx.origin_git(&["rev-parse", "main"]);
    assert_eq!(fx.origin_git(&["rev-parse", "release/v0.0.5~1"]), main);
    assert_eq!(fx.origin_git(&["log", "-1", "--format=%s", "release/v0.0.5"]), "Release 0.0.5");
    let files = fx.origin_git(&["diff-tree", "--no-commit-id", "--name-only", "-r", "release/v0.0.5"]);
    assert_eq!(files.lines().collect::<Vec<_>>(), ["CHANGELOG.md", "Cargo.lock", "Cargo.toml"]);
    assert!(fx.origin_git(&["show", "release/v0.0.5:Cargo.toml"]).contains("version = \"0.0.5\""));
    assert!(fx.origin_git(&["show", "release/v0.0.5:CHANGELOG.md"]).contains("## [0.0.5] - 2026-01-01"));

    // cargo-release: no publish, no tag, no push. The Gate ran after npm ci.
    let cargo = fx.env.calls("cargo");
    let bump = cargo.last().unwrap();
    assert_eq!(bump[..2], ["release", "0.0.5"]);
    for flag in ["--workspace", "--no-publish", "--no-tag", "--no-push", "--execute"] {
        assert!(bump.iter().any(|a| a == flag), "{flag} in {bump:?}");
    }
    assert_eq!(fx.env.calls("npm"), [vec!["ci"], vec!["run", "verify"]]);

    // gh: a PR 'Release 0.0.5' on main from the branch.
    let gh = fx.env.calls("gh");
    assert_eq!(gh.len(), 1, "{gh:?}");
    assert_eq!(gh[0][..2], ["pr", "create"]);
    assert!(gh[0].windows(2).any(|w| w == ["--title", "Release 0.0.5"]), "{gh:?}");
    assert!(gh[0].windows(2).any(|w| w == ["--base", "main"]), "{gh:?}");

    // The checkout is untouched: still on main, no extra worktree or branch.
    assert_eq!(fx.env.git(&fx.clone, &["branch", "--show-current"]), "main");
    assert_eq!(fx.env.git(&fx.clone, &["worktree", "list"]).lines().count(), 1);
    assert_eq!(fx.env.git(&fx.clone, &["branch", "--format=%(refname:short)"]), "main");
}

#[test]
fn release_writes_one_changelog_section() {
    let fx = Fixture::new();
    let main = fx.origin_git(&["rev-parse", "main"]);
    fx.origin_git(&["tag", "v0.0.5", &main]);
    let (exit, stdout, stderr) = fx.run(&["0.0.6"]);
    assert_eq!(exit, 0, "{stdout}{stderr}");

    let changelog = fx.origin_git(&["show", "release/v0.0.6:CHANGELOG.md"]);
    let sections = changelog.lines().filter(|l| l.starts_with("## [0.0.6]")).count();
    assert_eq!(sections, 1, "one `## [0.0.6]` section in the release commit's CHANGELOG.md:\n{changelog}");
    let cliff = fx.env.calls("git-cliff");
    assert_eq!(cliff.len(), 1, "git-cliff runs once, not once per crate: {cliff:?}");
}

#[test]
fn release_refuses_bad_versions() {
    let fx = Fixture::new();
    for bad in ["1.2", "v0.0.5", "0.0.x", "0.0.5.1", ""] {
        fx.refused(&[bad], "is not a version x.y.z");
    }
}

#[test]
fn release_refuses_a_version_not_greater_than_main() {
    let fx = Fixture::new();
    fx.refused(&["0.0.0"], "is not greater than the workspace version 0.0.0");
    fx.squash("Release 0.0.5 (#3)", "0.0.5");
    fx.refused(&["0.0.5"], "is not greater than the workspace version 0.0.5");
    fx.refused(&["0.0.4"], "is not greater than the workspace version 0.0.5");
    // 0.0.10 is greater numerically (not as text): it is not refused for its version.
    let (_, _, stderr) = fx.run(&["0.0.10"]);
    assert!(!stderr.contains("not greater"), "{stderr}");
}

#[test]
fn release_refuses_an_existing_tag() {
    let fx = Fixture::new();
    let main = fx.origin_git(&["rev-parse", "main"]);
    fx.origin_git(&["tag", "v0.0.5", &main]);
    fx.refused(&["0.0.5"], "tag v0.0.5 already exists");
}

#[test]
fn release_refuses_when_a_tool_is_missing() {
    let fx = Fixture::new();
    // cargo-release missing: `cargo release --version` fails.
    fx.env.fake("cargo", "exit 101");
    fx.refused(&["0.0.5"], "cargo-release is not installed");

    // git-cliff missing: a PATH with the fakes and only the tools the script needs.
    fx.mock_tools();
    fs::remove_file(fx.env.bin().join("git-cliff")).unwrap();
    let tools = fx.env.root().join("tools");
    fs::create_dir_all(&tools).unwrap();
    for tool in ["sh", "git", "awk", "sed", "head", "tr", "wc", "mktemp", "rm", "dirname", "cat"] {
        let found = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|d| d.join(tool))
            .find(|p| p.exists())
            .unwrap_or_else(|| panic!("{tool} not on PATH"));
        symlink(found, tools.join(tool)).unwrap();
    }
    let path = std::env::join_paths([fx.env.bin(), tools]).unwrap();
    let run = fx.run_path(&["0.0.5"], Some(&path));
    fx.assert_refused(run, "git-cliff is not installed");
}

#[test]
fn tag_tags_the_squash_commit_and_is_idempotent() {
    let fx = Fixture::new();
    fx.squash("Release 0.0.4 (#2)", "0.0.4");
    let release = fx.squash("Release 0.0.5 (#3)", "0.0.5");
    fx.squash("Something later (#4)", "0.0.5");

    let (exit, stdout, stderr) = fx.run(&["tag", "0.0.5"]);
    assert_eq!(exit, 0, "{stdout}{stderr}");
    assert_eq!(fx.origin_git(&["rev-parse", "v0.0.5^{commit}"]), release);
    assert_eq!(fx.origin_git(&["cat-file", "-t", "v0.0.5"]), "tag");
    assert_eq!(fx.origin_git(&["tag", "-l"]), "v0.0.5");

    let (exit, stdout, stderr) = fx.run(&["tag", "0.0.5"]);
    assert_eq!(exit, 0, "{stdout}{stderr}");
    assert!(stdout.contains("already"), "{stdout}");
    assert_eq!(fx.origin_git(&["tag", "-l"]), "v0.0.5");
}

#[test]
fn tag_refuses_without_a_merged_release_a_wrong_version_or_another_sha() {
    let fx = Fixture::new();
    let (exit, _, stderr) = fx.run(&["tag", "0.0.5"]);
    assert_eq!(exit, 1);
    assert!(stderr.contains("no commit on origin/main"), "{stderr}");

    // The release commit's Cargo.toml says another version.
    fx.squash("Release 0.0.6 (#5)", "0.0.9");
    let (exit, _, stderr) = fx.run(&["tag", "0.0.6"]);
    assert_eq!(exit, 1);
    assert!(stderr.contains("has version '0.0.9', not 0.0.6"), "{stderr}");

    // The tag exists on another commit.
    fx.squash("Release 0.0.7 (#6)", "0.0.7");
    let main = fx.origin_git(&["rev-parse", "main~1"]);
    fx.env.git(&fx.clone, &["fetch", "-q", "origin"]);
    fx.env.git(&fx.clone, &["tag", "v0.0.7", &main]);
    let (exit, _, stderr) = fx.run(&["tag", "0.0.7"]);
    assert_eq!(exit, 1);
    assert!(stderr.contains("already exists on"), "{stderr}");
    assert_eq!(fx.origin_git(&["tag", "-l"]), "");
}
