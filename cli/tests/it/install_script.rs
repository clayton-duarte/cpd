//! `scripts/install-cpd.sh` refusals (contract §3.7): the preview guard (hs-d178 D4), which refuses
//! a build that carries a migration file that is not on `origin/main`, and the store check, which
//! refuses by the `store` verdict of the build's `--version`. Fakes only: cargo "builds" a fake
//! `cpd`, and the checkout, with an `origin/main` of its own, is a temporary git repo.

use crate::common;
#[path = "common/checkout.rs"]
mod checkout;

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output};

use checkout::Checkout;
use common::{spawn, wait_until, TestEnv};

const OPENS: &str = r#"{"opens":true,"refusal":null,"unknown":[],"pending":[]}"#;

struct Install {
    exit: i32,
    stderr: String,
}

/// Runs the checkout's script with a fake cargo, which records that it ran and writes a fake `cpd`
/// whose `--version` is `version`.
fn install(env: &TestEnv, checkout: &Checkout, version: &str) -> Install {
    install_with(env, checkout, version, "")
}

/// As [`install`], with `extra` shell code in the fake `cpd` before its `--version` case.
fn install_with(env: &TestEnv, checkout: &Checkout, version: &str, extra: &str) -> Install {
    install_env(env, checkout, version, extra, &[])
}

/// As [`install_with`], with `vars` in the script's environment.
fn install_env(env: &TestEnv, checkout: &Checkout, version: &str, extra: &str, vars: &[(&str, &str)]) -> Install {
    let built = env.fake("cpd-built", &format!("{extra}\ncase \"$1\" in --version) echo '{version}' ;; esac"));
    let target = env.root().join("target");
    let cargo = env.fake("fake-cargo", &format!("mkdir -p '{t}/release' && cp '{}' '{t}/release/cpd'", built.display(), t = target.display()));
    let out: Output = env
        .command(checkout.script())
        .env("CARGO", cargo)
        .env("CARGO_TARGET_DIR", &target)
        .envs(vars.iter().copied())
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

/// Fake `watch --ensure` body: starts an engine, a process holding `locks/watch.lock` (it is
/// `$CPD_HOME/engine.pid`; the test kills it), and answers once the lock is held.
const STARTS_ENGINE: &str = r#"l="$CPD_HOME/locks/watch.lock"; mkdir -p "$CPD_HOME/locks"
sh -c 'echo $$ > "$CPD_HOME/engine.pid"; exec flock "$1" sleep 120' sh "$l" </dev/null >/dev/null 2>&1 &
until ! flock -n "$l" true; do :; done
echo '{"ok":true,"started":true}'"#;

/// Kills the fake engine and the fake old engine when the test ends.
struct Engines(Vec<PathBuf>, Vec<Child>);

impl Drop for Engines {
    fn drop(&mut self) {
        for pid in &self.0 {
            if let Ok(pid) = fs::read_to_string(pid) {
                let _ = Command::new("kill").arg(pid.trim()).status();
            }
        }
        for child in &mut self.1 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn lock_path(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("locks/watch.lock")
}

fn lock_held(env: &TestEnv) -> bool {
    !Command::new("flock").arg("-n").arg(lock_path(env)).arg("true").status().unwrap().success()
}

/// An old engine: a process holding the lock for `secs` seconds.
fn old_engine(env: &TestEnv, secs: &str) -> Child {
    fs::create_dir_all(lock_path(env).parent().unwrap()).unwrap();
    let child = spawn(Command::new("flock").arg(lock_path(env)).args(["sleep", secs])).unwrap();
    wait_until("the old engine holds the lock", || lock_held(env));
    child
}

const CONFIG: &str = r#"{"ok":true,"config":{"tmux":{"socket":"s","cockpit_session":"cock","work_session":"work"}}}"#;

/// A fake `tmux` on `PATH`: the cockpit has `main` and the work session has `watch`.
fn fake_tmux(env: &TestEnv) {
    env.fake("tmux", "case \"$*\" in *list-windows*cock*) echo main;; *list-windows*work*) echo watch;; esac");
}

fn follow_ups(env: &TestEnv) -> Vec<String> {
    env.calls("cpd-built").into_iter().map(|c| c.join(" ")).filter(|c| c.starts_with("tui") || c.starts_with("watch")).collect()
}

#[test]
fn a_failing_config_is_printed_and_makes_the_script_exit_non_zero() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    fake_tmux(&env);
    let extra = r#"case "$1" in config) echo '{"ok":false,"error":{"code":"lock_timeout"}}'; echo 'waiting' >&2; exit 1;; esac"#;

    let run = install_with(&env, &checkout, &version("null"), extra);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(installed(&env));
    assert!(run.stderr.contains("cpd install follow-up failed: cpd config: "), "{}", run.stderr);
    assert!(run.stderr.contains("lock_timeout"), "{}", run.stderr);
    assert!(follow_ups(&env).is_empty());
}

#[test]
fn a_failing_follow_up_is_printed_and_the_later_ones_still_run() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    fake_tmux(&env);
    let extra = format!(r#"case "$1" in config) echo '{CONFIG}';; tui) echo 'no cockpit' >&2; exit 3;; watch) {STARTS_ENGINE};; esac"#);

    let run = install_with(&env, &checkout, &version("null"), &extra);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("cpd install follow-up failed: cpd tui --respawn exited 3"), "{}", run.stderr);
    assert_eq!(follow_ups(&env), ["tui --respawn", "watch --ensure"]);
}

#[test]
fn the_follow_ups_run_after_a_slow_config() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    fake_tmux(&env);
    // A migration holds the setup lock: cpd config answers only after a while.
    let extra = format!(r#"case "$1" in config) sleep 2; echo '{CONFIG}';; watch) {STARTS_ENGINE};; esac"#);

    let run = install_with(&env, &checkout, &version("null"), &extra);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(follow_ups(&env), ["tui --respawn", "watch --ensure"]);
}

fn engine_restart_extra() -> String {
    format!(r#"case "$1" in config) echo '{CONFIG}';; watch) {STARTS_ENGINE};; esac"#)
}

#[test]
fn an_old_engine_that_holds_its_lock_for_longer_than_ten_seconds_is_waited_for() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    fake_tmux(&env);
    // time-claim: the old engine outlives the former 10 s give-up; no per-poll signal from the script's wait.
    let old = old_engine(&env, "12");
    let _engines = Engines(vec![env.cpd_home().join("engine.pid")], vec![old]);

    let run = install_with(&env, &checkout, &version("null"), &engine_restart_extra());

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(follow_ups(&env), ["tui --respawn", "watch --ensure"]);
    assert!(lock_held(&env), "the new engine holds the lock");
}

#[test]
fn an_old_engine_that_never_frees_its_lock_fails_the_install_with_the_reason() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    fake_tmux(&env);
    // time-claim: the bound is 2 s here; the old engine holds well past it; no per-poll signal from the script's wait.
    let old = old_engine(&env, "120");
    let _engines = Engines(vec![], vec![old]);

    let run = install_env(&env, &checkout, &version("null"), &engine_restart_extra(), &[("CPD_INSTALL_LOCK_WAIT_SECS", "2")]);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("cpd install follow-up failed: the old engine still holds "), "{}", run.stderr);
    assert!(run.stderr.contains("after 2s: the new engine was not started"), "{}", run.stderr);
    assert_eq!(follow_ups(&env), ["tui --respawn"]);
}

#[test]
fn a_new_engine_that_does_not_run_fails_the_install_with_the_reason() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    fake_tmux(&env);
    // --ensure answers, but no process holds the lock.
    let extra = format!(r#"case "$1" in config) echo '{CONFIG}';; watch) echo '{{"ok":true,"started":true}}';; esac"#);

    let run = install_with(&env, &checkout, &version("null"), &extra);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("but no engine holds "), "{}", run.stderr);
    assert!(run.stderr.contains("the engine is not running"), "{}", run.stderr);
}
