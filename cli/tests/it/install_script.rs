//! `scripts/install-cpd.sh` refusals (contract §3.7): the preview guard (hs-d178 D4), which refuses
//! a build that carries a migration file that is not on `origin/main`, and the store check, which
//! refuses by the `store` verdict of the build's `--version`. Fakes only: cargo "builds" a fake
//! `cpd`, and the checkout, with an `origin/main` of its own, is a temporary git repo.

mod common;
#[path = "common/checkout.rs"]
mod checkout;

use std::fs;
use std::process::Output;

use checkout::Checkout;
use common::TestEnv;

const OPENS: &str = r#"{"opens":true,"refusal":null,"unknown":[],"pending":[]}"#;

struct Install {
    exit: i32,
    stderr: String,
}

/// Runs the checkout's script with a fake cargo, which records that it ran and writes a fake `cpd`
/// whose `--version` is `version`.
fn install(env: &TestEnv, checkout: &Checkout, version: &str) -> Install {
    let built = env.fake("cpd-built", &format!("case \"$1\" in --version) echo '{version}' ;; esac"));
    let target = env.root().join("target");
    let cargo = env.fake("fake-cargo", &format!("mkdir -p '{t}/release' && cp '{}' '{t}/release/cpd'", built.display(), t = target.display()));
    let out: Output = env
        .command(checkout.script())
        .env("CARGO", cargo)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    Install { exit: out.status.code().unwrap(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

fn version(store: &str) -> String {
    format!(r#"{{"ok":true,"version":"0.0.0","protocol":1,"store":{store}}}"#)
}

fn installed(env: &TestEnv) -> bool {
    env.home().join(".local/bin/cpd").exists() || env.home().join(".local/share/cpd/pi").exists()
}

/// The script refused before it built or replaced anything.
fn assert_refused_before_building(env: &TestEnv, run: &Install) {
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("cpd install refused: "), "{}", run.stderr);
    assert!(env.calls("fake-cargo").is_empty(), "cargo ran");
    assert!(!installed(env));
}

#[test]
fn a_build_that_adds_a_migration_origin_main_lacks_is_refused_before_building() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    // Untracked, as a new file of a Lead Branch preview is until it is committed.
    checkout.write_migration("29991231T2359-branch-only.sql", "-- cpd-migration: additive\n");

    let run = install(&env, &checkout, &version(OPENS));

    assert_refused_before_building(&env, &run);
    assert!(run.stderr.contains("this build carries migration 29991231T2359-branch-only.sql that is not on origin/main"), "{}", run.stderr);
    assert!(run.stderr.contains(&format!("apply it to {}/cpd.db before the PR merges", env.cpd_home().display())), "{}", run.stderr);
    assert!(run.stderr.contains("Merge main into this branch, or wait until the PR merges, then install again."), "{}", run.stderr);
}

#[test]
fn a_committed_branch_migration_is_refused_too_and_all_the_names_are_listed() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    checkout.write_migration("29991231T2358-first.sql", "x\n");
    checkout.write_migration("29991231T2359-second.sql", "y\n");
    env.git(&checkout.root, &["add", "-A"]);
    env.git(&checkout.root, &["commit", "-q", "-m", "migrations"]);

    let run = install(&env, &checkout, &version(OPENS));

    assert_refused_before_building(&env, &run);
    assert!(run.stderr.contains("migration 29991231T2358-first.sql, 29991231T2359-second.sql that is not on origin/main"), "{}", run.stderr);
}

#[test]
fn a_migration_file_changed_from_origin_main_is_refused() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    checkout.write_migration("0001-initial.sql", "changed\n");

    let run = install(&env, &checkout, &version(OPENS));

    assert_refused_before_building(&env, &run);
    assert!(run.stderr.contains("migration 0001-initial.sql that is not on origin/main"), "{}", run.stderr);
}

#[test]
fn a_migration_that_main_gained_after_the_branch_is_not_the_branch_check() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    // origin/main gets a migration the clone's tree lacks.
    let origin = env.root().join("repos/install-origin");
    fs::write(origin.join("core/src/store/migrations/29991231T2359-later.sql"), "later\n").unwrap();
    env.git(&origin, &["add", "-A"]);
    env.git(&origin, &["commit", "-q", "-m", "later"]);

    let run = install(&env, &checkout, &version(OPENS));

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(installed(&env));
}

#[test]
fn a_build_whose_migrations_are_all_on_origin_main_installs() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql", "0002-more.sql"]);

    let run = install(&env, &checkout, &version(OPENS));

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(env.home().join(".local/bin/cpd").exists());
    assert_eq!(env.calls("fake-cargo").len(), 1);
}

#[test]
fn a_failed_fetch_is_a_refusal_because_the_script_cannot_know() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &["0001-initial.sql"]);
    env.git(&checkout.root, &["remote", "set-url", "origin", env.root().join("nowhere").to_str().unwrap()]);

    let run = install(&env, &checkout, &version(OPENS));

    assert_refused_before_building(&env, &run);
    assert!(run.stderr.contains("cannot fetch origin/main"), "{}", run.stderr);
}

fn refusing(code: &str, message: &str) -> String {
    version(&format!(r#"{{"opens":false,"refusal":{{"code":"{code}","message":"{message}"}},"unknown":[],"pending":[]}}"#))
}

#[test]
fn the_store_verdict_refuses_with_its_message_and_the_fix_of_its_code() {
    for (code, fix) in [
        ("store_too_new", "merge main into this branch, then install again"),
        ("migration_changed", "restore it from origin/main or wait for the store to be repaired"),
        ("migration_order", "rename the branch's breaking migration to a newer name with scripts/new-migration.sh, then install again"),
    ] {
        let env = TestEnv::new();
        let checkout = Checkout::new(&env, &[]);
        let run = install(&env, &checkout, &refusing(code, "the verdict words"));

        assert_eq!(run.exit, 1, "{code}: {}", run.stderr);
        assert!(run.stderr.contains(&format!("this build cannot open {}/cpd.db: the verdict words", env.cpd_home().display())), "{code}: {}", run.stderr);
        assert!(run.stderr.contains(fix), "{code}: {}", run.stderr);
        assert!(!installed(&env), "{code}");
    }
}

#[test]
fn another_refusal_code_prints_the_message_alone() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    let run = install(&env, &checkout, &refusing("store_failed", "not a store"));
    assert_eq!(run.exit, 1);
    assert!(run.stderr.contains("cpd.db: not a store\n"), "{}", run.stderr);
    assert!(!run.stderr.contains("merge main"), "{}", run.stderr);
    assert!(!installed(&env));
}

#[test]
fn a_store_that_does_not_exist_or_opens_installs_and_unknown_additive_migrations_are_no_refusal() {
    for store in ["null", OPENS, r#"{"opens":true,"refusal":null,"unknown":["29991231T2359-later"],"pending":[]}"#] {
        let env = TestEnv::new();
        let checkout = Checkout::new(&env, &[]);
        let run = install(&env, &checkout, &version(store));
        assert_eq!(run.exit, 0, "{store}: {}", run.stderr);
        assert!(installed(&env), "{store}");
    }
}

#[test]
fn a_version_without_a_store_verdict_or_that_fails_is_refused() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    // An integer-era build: the legacy numbers only.
    let run = install(&env, &checkout, r#"{"ok":true,"version":"0.0.0"}"#);
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("reports no store verdict"), "{}", run.stderr);
    assert!(!installed(&env));
}
