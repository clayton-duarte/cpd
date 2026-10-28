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

const CLI_DIR: &str = env!("CARGO_MANIFEST_DIR");

/// `cli/build.rs`, included whole: its logic is the function `directives`, which the tests call. Its `main` (the
/// environment and `println!`) is the one line of it the tests do not run.
#[allow(dead_code)] // build.rs's `main` is the build's entry point, which no test calls
#[path = "../../build.rs"]
mod build_script;

/// The `cargo:` lines the build script prints for `commit` (what `CPD_COMMIT` holds, `None` when it is unset).
fn run(commit: Option<&str>) -> Vec<String> {
    build_script::directives(commit, VERSION)
        .into_iter()
        .map(|line| line.strip_prefix("cargo::").or_else(|| line.strip_prefix("cargo:")).unwrap_or_else(|| panic!("not a directive: {line}")).to_owned())
        .collect()
}

/// The `CPD_VERSION` the build script sets for `cpd` (exactly one).
fn cpd_version(commit: Option<&str>) -> String {
    let directives = run(commit);
    let versions: Vec<&str> = directives.iter().filter_map(|d| d.strip_prefix("rustc-env=CPD_VERSION=")).collect();
    assert_eq!(versions.len(), 1, "one CPD_VERSION: {directives:?}");
    versions[0].to_owned()
}

/// The build script starts no process, so it cannot run `git`: its source names no `Command`.
fn assert_git_never_ran() {
    let source = include_str!("../../build.rs");
    assert!(!source.contains("Command") && !source.contains("process::"), "the build script ran git");
}

#[test]
fn a_build_given_cpd_commit_reports_that_commit_not_the_checkout_s() {
    assert_eq!(cpd_version(Some("abc1234")), format!("{VERSION}-abc1234"));
    assert_git_never_ran();
}

#[test]
fn a_build_given_no_commit_reports_dev_even_in_a_git_checkout() {
    assert_eq!(cpd_version(None), format!("{VERSION}-dev"), "CPD_COMMIT unset");
    assert_eq!(cpd_version(Some("")), format!("{VERSION}-dev"), "CPD_COMMIT empty");
    assert_git_never_ran();
}

#[test]
fn the_build_reruns_when_cpd_commit_changes_and_watches_no_git_file() {
    let directives = run(Some("abc1234"));
    assert!(directives.iter().any(|d| d == "rerun-if-env-changed=CPD_COMMIT"), "{directives:?}");
    let git_watches: Vec<&String> =
        directives.iter().filter(|d| d.strip_prefix("rerun-if-changed=").is_some_and(|path| path.contains(".git"))).collect();
    assert!(git_watches.is_empty(), "a commit would rebuild cpd: {git_watches:?}");
    assert_git_never_ran();
}

#[test]
fn the_install_script_builds_with_the_checkout_s_short_commit() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    let given = env.root().join("cpd-commit");
    // Records the CPD_COMMIT it was given, then fails: the script stops before installing anything.
    let cargo = env.fake("fake-cargo", &format!("printf '%s' \"${{CPD_COMMIT-unset}}\" > '{}'\nexit 1", given.display()));
    // cargo.sh calls `cpd resource list` when a `cpd` is on PATH: a fake that fails keeps the real one out and falls back to cargo.
    env.fake("cpd", "exit 1");
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
