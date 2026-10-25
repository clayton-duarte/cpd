//! `cpd --version`'s `version` (docs/implementing.md "The version string", commands.md §1.2):
//! `<Cargo.toml version>-<commit>`, the commit being the short hash the build was given in
//! `CPD_COMMIT`, `dev` when it was given none; `cli/build.rs` reads `CPD_COMMIT`, reruns when it
//! changes, runs no `git` and watches no `.git` file; `scripts/install-cpd.sh` builds with the
//! checkout's short commit hash, so an installed binary reports exactly the commit it was built from.

use crate::common;
#[path = "../common/checkout.rs"]
mod checkout;

use std::fs;
use std::path::{Path, PathBuf};

use checkout::Checkout;
use common::{output, TestEnv};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The `CPD_COMMIT` the build of these tests was given: the same cargo run built `cpd`.
const BUILD_COMMIT: Option<&str> = option_env!("CPD_COMMIT");

const CLI_DIR: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn cpd_reports_the_commit_its_build_was_given_or_dev() {
    let env = TestEnv::empty();
    let ok = env.cpd(&["--version"]).ok();
    let expected = match BUILD_COMMIT {
        Some(commit) if !commit.is_empty() => format!("{VERSION}-{commit}"),
        _ => format!("{VERSION}-dev"),
    };
    assert_eq!(ok["version"], expected.as_str(), "built with CPD_COMMIT={BUILD_COMMIT:?}");
}

/// `cli/build.rs`, compiled on its own by the `rustc` beside the cargo that built these tests.
struct BuildScript {
    env: TestEnv,
    exe: PathBuf,
}

impl BuildScript {
    /// Compiles the build script, and puts a fake `git` first on `PATH` that records its calls
    /// and runs the real one.
    fn new() -> BuildScript {
        let env = TestEnv::empty();
        let git = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join("git"))
            .find(|p| p.is_file())
            .expect("git on PATH");
        env.fake("git", &format!("exec '{}' \"$@\"", git.display()));
        let rustc = Path::new(env!("CARGO")).with_file_name("rustc");
        let exe = env.root().join("build-script");
        let build_rs = Path::new(CLI_DIR).join("build.rs");
        let out = output(
            std::process::Command::new(&rustc)
                .args(["--edition", "2024", "--crate-name", "build_script_build", "-o"])
                .arg(&exe)
                .arg(&build_rs),
        )
        .unwrap_or_else(|e| panic!("run {}: {e}", rustc.display()));
        assert!(out.status.success(), "rustc {}: {}", build_rs.display(), String::from_utf8_lossy(&out.stderr));
        BuildScript { env, exe }
    }

    /// Runs the build script as cargo would, in the package's directory (a git checkout, `git` on
    /// `PATH`), with `CPD_COMMIT` set to `commit` or unset; returns its directives, the
    /// `cargo:` or `cargo::` prefix removed.
    fn run(&self, commit: Option<&str>) -> Vec<String> {
        let out_dir = self.env.root().join("out");
        fs::create_dir_all(&out_dir).unwrap();
        let mut cmd = self.env.command(&self.exe);
        cmd.current_dir(CLI_DIR)
            .env("CARGO_MANIFEST_DIR", CLI_DIR)
            .env("CARGO_PKG_VERSION", VERSION)
            .env("CARGO_PKG_NAME", "cpd")
            .env("OUT_DIR", &out_dir)
            .env_remove("CPD_COMMIT");
        if let Some(commit) = commit {
            cmd.env("CPD_COMMIT", commit);
        }
        let out = output(&mut cmd).unwrap();
        assert!(out.status.success(), "the build script failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("cargo::").or_else(|| line.strip_prefix("cargo:")))
            .map(str::to_owned)
            .collect()
    }

    /// The `CPD_VERSION` the build script sets for `cpd` (exactly one).
    fn cpd_version(&self, commit: Option<&str>) -> String {
        let directives = self.run(commit);
        let versions: Vec<&str> = directives.iter().filter_map(|d| d.strip_prefix("rustc-env=CPD_VERSION=")).collect();
        assert_eq!(versions.len(), 1, "one CPD_VERSION: {directives:?}");
        versions[0].to_owned()
    }

    fn assert_git_never_ran(&self) {
        assert_eq!(self.env.calls("git"), Vec::<Vec<String>>::new(), "the build script ran git");
    }
}

#[test]
fn a_build_given_cpd_commit_reports_that_commit_not_the_checkout_s() {
    let script = BuildScript::new();
    assert_eq!(script.cpd_version(Some("abc1234")), format!("{VERSION}-abc1234"));
    script.assert_git_never_ran();
}

#[test]
fn a_build_given_no_commit_reports_dev_even_in_a_git_checkout() {
    let script = BuildScript::new();
    assert_eq!(script.cpd_version(None), format!("{VERSION}-dev"), "CPD_COMMIT unset");
    assert_eq!(script.cpd_version(Some("")), format!("{VERSION}-dev"), "CPD_COMMIT empty");
    script.assert_git_never_ran();
}

#[test]
fn the_build_reruns_when_cpd_commit_changes_and_watches_no_git_file() {
    let script = BuildScript::new();
    let directives = script.run(Some("abc1234"));
    assert!(directives.iter().any(|d| d == "rerun-if-env-changed=CPD_COMMIT"), "{directives:?}");
    let git_watches: Vec<&String> =
        directives.iter().filter(|d| d.strip_prefix("rerun-if-changed=").is_some_and(|path| path.contains(".git"))).collect();
    assert!(git_watches.is_empty(), "a commit would rebuild cpd: {git_watches:?}");
    script.assert_git_never_ran();
}

#[test]
fn the_install_script_builds_with_the_checkout_s_short_commit() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    let given = env.root().join("cpd-commit");
    // Records the CPD_COMMIT it was given, then fails: the script stops before installing anything.
    let cargo = env.fake("fake-cargo", &format!("printf '%s' \"${{CPD_COMMIT-unset}}\" > '{}'\nexit 1", given.display()));
    let out = output(env.command(checkout.script()).env("CARGO", cargo).env("CARGO_TARGET_DIR", env.root().join("target"))).unwrap();
    assert_eq!(env.calls("fake-cargo").len(), 1, "cargo ran once: {}", String::from_utf8_lossy(&out.stderr));
    let head = env.git(&checkout.root, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(fs::read_to_string(&given).unwrap(), head);
}

// ---- the derived version: Cargo.toml (`cpd version sync` fixes it; its tests are core/src/version_sync/tests.rs) ----

fn repo_root() -> PathBuf {
    Path::new(CLI_DIR).join("..")
}

/// The `.sql` files directly in the real migrations directory.
fn migration_files() -> usize {
    sql_files(&repo_root().join("core/src/store/migrations"))
}

fn sql_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|x| x == "sql"))
        .count()
}

#[test]
fn cargo_version_is_zero_protocol_migration_files() {
    let protocol = cpd_core::events::PROTOCOL;
    let n = migration_files();
    let expected = format!("0.{protocol}.{n}");
    assert!(
        VERSION == expected,
        "Cargo.toml version {VERSION} is not {expected} (protocol {protocol}, {n} migration files): run scripts/sync-version.sh"
    );
}
