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

// ---- the derived version: Cargo.toml and scripts/sync-version.sh ----

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

const FIXTURE_CARGO_TOML: &str = "[workspace]\nmembers = [\"cli\"]\n\n[workspace.package]\nversion = \"0.0.5\"\nedition = \"2024\"\n\n[workspace.dependencies]\nfoo = { version = \"0.0.5\" }\n\n[profile.dev]\ndebug = 0\n";

fn fixture_cargo_lock() -> String {
    let mut s = String::from("# This file is automatically @generated by Cargo.\nversion = 4\n");
    for name in ["cpd", "cpd-core", "cpd-engine", "cpd-tui"] {
        s.push_str(&format!("\n[[package]]\nname = \"{name}\"\nversion = \"0.0.5\"\n"));
    }
    s.push_str("\n[[package]]\nname = \"other\"\nversion = \"0.0.5\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n");
    s
}

/// A fake repo root with kind.rs (PROTOCOL 7), three `.sql` files and one `.rs`, Cargo.toml and
/// Cargo.lock, and the real `scripts/<names>` copied in.
fn fixture(env: &TestEnv, scripts: &[&str]) -> PathBuf {
    let root = env.root().join("fixture");
    let migrations = root.join("core/src/store/migrations");
    fs::create_dir_all(&migrations).unwrap();
    fs::create_dir_all(root.join("core/src/events")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("core/src/events/kind.rs"), "//! kinds\n\npub const PROTOCOL: u32 = 7;\npub const OTHER: u32 = 9;\n").unwrap();
    for f in ["0001-initial.sql", "0002-a.sql", "20261018T0828-b.sql", "20261018T0828-b.rs"] {
        fs::write(migrations.join(f), "-- cpd-migration: additive\n").unwrap();
    }
    fs::write(root.join("Cargo.toml"), FIXTURE_CARGO_TOML).unwrap();
    fs::write(root.join("Cargo.lock"), fixture_cargo_lock()).unwrap();
    for name in scripts {
        let to = root.join("scripts").join(name);
        fs::copy(repo_root().join("scripts").join(name), &to).unwrap_or_else(|e| panic!("copy scripts/{name}: {e}"));
    }
    root
}

fn run_sync(env: &TestEnv, root: &Path, args: &[&str]) -> std::process::Output {
    output(env.command(root.join("scripts/sync-version.sh")).args(args).current_dir(root)).unwrap()
}

fn read(root: &Path, file: &str) -> String {
    fs::read_to_string(root.join(file)).unwrap()
}

#[test]
fn sync_print_derives_the_version_from_kind_rs_and_the_sql_files() {
    let env = TestEnv::empty();
    let root = fixture(&env, &["sync-version.sh"]);
    let out = run_sync(&env, &root, &["--print"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0.7.3\n");
    assert_eq!(read(&root, "Cargo.toml"), FIXTURE_CARGO_TOML);
    assert_eq!(read(&root, "Cargo.lock"), fixture_cargo_lock());
}

#[test]
fn sync_rewrites_cargo_toml_and_the_workspace_members_of_cargo_lock() {
    let env = TestEnv::empty();
    let root = fixture(&env, &["sync-version.sh"]);
    let out = run_sync(&env, &root, &[]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).lines().any(|l| l.starts_with("sync-version:")), "stdout: {}", String::from_utf8_lossy(&out.stdout));
    let toml = read(&root, "Cargo.toml");
    assert_eq!(toml, FIXTURE_CARGO_TOML.replacen("version = \"0.0.5\"\nedition", "version = \"0.7.3\"\nedition", 1));
    assert!(toml.contains("foo = { version = \"0.0.5\" }\n"));
    let mut lock = fixture_cargo_lock();
    for name in ["cpd", "cpd-core", "cpd-engine", "cpd-tui"] {
        lock = lock.replacen(&format!("name = \"{name}\"\nversion = \"0.0.5\""), &format!("name = \"{name}\"\nversion = \"0.7.3\""), 1);
    }
    assert_eq!(read(&root, "Cargo.lock"), lock);
    assert!(lock.contains("name = \"other\"\nversion = \"0.0.5\""));
}

#[test]
fn sync_is_idempotent() {
    let env = TestEnv::empty();
    let root = fixture(&env, &["sync-version.sh"]);
    let first = run_sync(&env, &root, &[]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let (toml, lock) = (read(&root, "Cargo.toml"), read(&root, "Cargo.lock"));
    assert!(toml.contains("version = \"0.7.3\""), "first run rewrote nothing: {toml}");
    let second = run_sync(&env, &root, &[]);
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(read(&root, "Cargo.toml"), toml);
    assert_eq!(read(&root, "Cargo.lock"), lock);
}

#[test]
fn sync_without_a_protocol_line_fails_and_changes_nothing() {
    let env = TestEnv::empty();
    let root = fixture(&env, &["sync-version.sh"]);
    let check = |what: &str| {
        let out = run_sync(&env, &root, &[]);
        assert_eq!(out.status.code(), Some(1), "{what}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).lines().any(|l| l.starts_with("sync-version:")), "{what}: stderr {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(read(&root, "Cargo.toml"), FIXTURE_CARGO_TOML, "{what}");
        assert_eq!(read(&root, "Cargo.lock"), fixture_cargo_lock(), "{what}");
    };
    let kind = root.join("core/src/events/kind.rs");
    fs::write(&kind, "//! kinds\npub const OTHER: u32 = 9;\n").unwrap();
    check("no PROTOCOL line");
    fs::write(&kind, "pub const PROTOCOL: u32 = 7;\n").unwrap();
    let migrations = root.join("core/src/store/migrations");
    for e in fs::read_dir(&migrations).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "sql") {
            fs::remove_file(p).unwrap();
        }
    }
    check("no .sql file");
}

#[test]
fn sync_agrees_with_the_rust_constants() {
    let env = TestEnv::empty();
    let script = repo_root().join("scripts/sync-version.sh");
    let out = output(env.command(&script).arg("--print").current_dir(repo_root())).unwrap();
    assert!(out.status.success(), "{}: {}", script.display(), String::from_utf8_lossy(&out.stderr));
    let expected = format!("0.{}.{}\n", cpd_core::events::PROTOCOL, migration_files());
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn new_migration_runs_the_sync() {
    let env = TestEnv::empty();
    let root = fixture(&env, &["new-migration.sh", "sync-version.sh"]);
    let out = output(env.command(root.join("scripts/new-migration.sh")).arg("demo").current_dir(&root)).unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "stdout: {stdout}");
    assert!(lines[0].contains("core/src/store/migrations/") && lines[0].ends_with("-demo.sql") && Path::new(lines[0]).is_file(), "{stdout}");
    assert!(read(&root, "Cargo.toml").contains("version = \"0.7.4\"\nedition"), "{}", read(&root, "Cargo.toml"));
}
