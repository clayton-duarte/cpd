//! `scripts/install-cpd.sh` refusals (contract §3.7): the preview guard (hs-d178 D4), which refuses
//! a build that carries a migration file that is not on `origin/main`, and the store check, which
//! refuses by the `store` verdict of the build's `--version`. Fakes only: cargo "builds" a fake
//! `cpd`, and the checkout, with an `origin/main` of its own, is a temporary git repo.

use crate::common;
#[path = "common/checkout.rs"]
mod checkout;

use std::fs;
use std::path::{Path, PathBuf};
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

// ---- The adapter swap (hs-d355; contract §3.7 "The adapter swap", docs/implementing.md "The adapter swap") ----
//
// One code path for Linux and macOS: `~/.local/share/cpd/pi` is a symlink to a versioned directory, swapped by one
// `rename(2)`. The tests run the swap on a temporary HOME, with a `python3` that refuses `renameat2` as macOS's does.

use std::os::unix::fs::symlink;
use std::time::{Duration, SystemTime};

/// The first `python3` of the real `PATH`.
fn real_python3() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|dir| dir.join("python3")).find(|p| p.is_file()).expect("python3 on PATH")
}

/// A `python3` first on `PATH` that behaves as macOS's: `ctypes.CDLL(None).renameat2` raises
/// `AttributeError: dlsym(RTLD_DEFAULT, renameat2): symbol not found`. Everything else is the real python3.
fn mac_python(env: &TestEnv) {
    let dir = env.root().join("mac-python");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("sitecustomize.py"),
        "import ctypes\n_getattr = ctypes.CDLL.__getattr__\n\
         def _mac(self, name):\n    if name == 'renameat2':\n        raise AttributeError('dlsym(RTLD_DEFAULT, renameat2): symbol not found')\n    return _getattr(self, name)\n\
         ctypes.CDLL.__getattr__ = _mac\n",
    )
    .unwrap();
    env.fake("python3", &format!("PYTHONPATH='{}' exec '{}' \"$@\"", dir.display(), real_python3().display()));
}

/// A `python3` whose `os.replace` (the swap's one rename) fails, and that is the real python3 otherwise.
fn python_whose_replace_fails(env: &TestEnv) {
    env.fake(
        "python3",
        &format!("case \"$*\" in *os.replace*) echo 'simulated: rename failed' >&2; exit 1;; esac\nexec '{}' \"$@\"", real_python3().display()),
    );
}

fn share(env: &TestEnv) -> PathBuf {
    env.home().join(".local/share/cpd")
}

/// A `pi.<id>` directory name: `<UTC YYYYmmddTHHMMSS>-<pid>`, or `pi.legacy`.
fn is_version(name: &str) -> bool {
    let Some(id) = name.strip_prefix("pi.") else { return false };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    id == "legacy"
        || id.split_once('-').is_some_and(|(stamp, pid)| {
            stamp.len() == 15 && stamp.as_bytes()[8] == b'T' && digits(&stamp[..8]) && digits(&stamp[9..]) && digits(pid)
        })
}

/// What `pi` points to: panics unless it is a symlink with a relative target that is a version name.
fn linked_version(env: &TestEnv) -> String {
    let pi = share(env).join("pi");
    let meta = fs::symlink_metadata(&pi).unwrap_or_else(|e| panic!("{}: {e}", pi.display()));
    assert!(meta.file_type().is_symlink(), "{} is not a symlink", pi.display());
    let target = fs::read_link(&pi).unwrap().to_string_lossy().into_owned();
    assert!(is_version(&target) && target != "pi.legacy", "the target `{target}` is not a pi.<id> beside the link (a relative target)");
    target
}

/// The names in `share/` that are versions (`pi.<id>`, `pi.legacy`), sorted.
fn versions(env: &TestEnv) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(share(env)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| is_version(n)).collect();
    names.sort();
    names
}

/// The `pi.tmp.<pid>` and `pi.link.<pid>` entries of `share/`.
fn scratch(env: &TestEnv) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(share(env)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("pi.tmp.") || n.starts_with("pi.link.")).collect();
    names.sort();
    names
}

/// A real adapter directory as installs before hs-d355 left it.
fn old_real_adapter(env: &TestEnv) -> PathBuf {
    let pi = share(env).join("pi");
    fs::create_dir_all(pi.join("lib")).unwrap();
    for (file, text) in [("package.json", "old\n"), ("extension.ts", "old\n"), ("lib/old-only.ts", "old\n")] {
        fs::write(pi.join(file), text).unwrap();
    }
    pi
}

fn age(path: &Path, by: Duration) {
    fs::File::open(path).unwrap().set_modified(SystemTime::now() - by).unwrap();
}

const HOUR2: Duration = Duration::from_secs(2 * 3600);

fn mac_install(env: &TestEnv, checkout: &Checkout) -> Install {
    install(env, checkout, &version("null"))
}

#[test]
fn a_first_install_links_pi_to_a_versioned_directory_on_a_python_without_renameat2() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = linked_version(&env);
    let pi = share(&env).join("pi");
    assert_eq!(fs::read_to_string(pi.join("extension.ts")).unwrap(), "extension.ts\n");
    assert!(pi.join("package.json").exists(), "pi/package.json");
    assert!(!pi.join("test").exists(), "tests are not part of the install");
    assert_eq!(versions(&env), [id], "one version, no pi.legacy");
    assert_eq!(scratch(&env), Vec::<String>::new(), "no pi.tmp.<pid> or pi.link.<pid> is left");
}

#[test]
fn the_installed_tree_can_be_moved_because_the_link_is_relative() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    assert_eq!(mac_install(&env, &checkout).exit, 0);
    linked_version(&env);

    let moved = env.root().join("moved-share");
    fs::rename(share(&env), &moved).unwrap();

    assert!(moved.join("pi/package.json").exists(), "the link still resolves after the tree moved");
}

#[test]
fn an_existing_real_adapter_directory_is_migrated_to_pi_legacy_without_renameat2() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    old_real_adapter(&env);

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = linked_version(&env);
    let share = share(&env);
    assert_eq!(fs::read_to_string(share.join("pi/extension.ts")).unwrap(), "extension.ts\n", "the new adapter is visible at pi");
    assert!(!share.join("pi/lib/old-only.ts").exists(), "no old file shows through the new pi");
    assert_eq!(fs::read_to_string(share.join("pi.legacy/lib/old-only.ts")).unwrap(), "old\n", "pi.legacy holds the old files");
    let mut want = vec!["pi.legacy".to_string(), id];
    want.sort();
    assert_eq!(versions(&env), want);
    assert_eq!(scratch(&env), Vec::<String>::new());
}

#[test]
fn a_failing_swap_of_the_migration_puts_the_old_directory_back_and_fails_the_install() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    python_whose_replace_fails(&env);
    let pi = old_real_adapter(&env);

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("simulated: rename failed"), "the reason is on stderr: {}", run.stderr);
    assert!(fs::symlink_metadata(&pi).unwrap().file_type().is_dir(), "pi is the old real directory again");
    assert_eq!(fs::read_to_string(pi.join("lib/old-only.ts")).unwrap(), "old\n");
    assert!(!share(&env).join("pi.legacy").exists(), "pi.legacy was renamed back");
}

#[test]
fn an_existing_symlink_is_swapped_and_its_version_is_kept_as_the_displaced_one() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    let share = share(&env);
    fs::create_dir_all(share.join("pi.20260101T000000-1")).unwrap();
    fs::write(share.join("pi.20260101T000000-1/package.json"), "old\n").unwrap();
    symlink("pi.20260101T000000-1", share.join("pi")).unwrap();

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = linked_version(&env);
    assert_ne!(id, "pi.20260101T000000-1");
    assert_eq!(fs::read_to_string(share.join("pi/extension.ts")).unwrap(), "extension.ts\n");
    assert_eq!(fs::read_to_string(share.join("pi.20260101T000000-1/package.json")).unwrap(), "old\n", "the displaced version stays one install");
}

#[test]
fn a_second_install_shows_the_new_content_and_keeps_the_displaced_version() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    assert_eq!(mac_install(&env, &checkout).exit, 0);
    let first = linked_version(&env);
    fs::write(checkout.root.join("pi/extension.ts"), "second install\n").unwrap();

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let second = linked_version(&env);
    assert_ne!(first, second);
    let share = share(&env);
    assert_eq!(fs::read_to_string(share.join("pi/extension.ts")).unwrap(), "second install\n", "the new content is visible through pi");
    assert_eq!(fs::read_to_string(share.join(&first).join("extension.ts")).unwrap(), "extension.ts\n", "the displaced version is whole and kept");
}

#[test]
fn installs_one_after_another_leave_the_current_and_the_displaced_version_once_the_others_are_old() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    assert_eq!(mac_install(&env, &checkout).exit, 0);
    let first = linked_version(&env);
    assert_eq!(mac_install(&env, &checkout).exit, 0);
    let second = linked_version(&env);
    // time-claim: the cleanup spares a version modified in the last 60 minutes; the clock is set, not waited for.
    age(&share(&env).join(&first), HOUR2);
    age(&share(&env).join(&second), HOUR2);

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let third = linked_version(&env);
    let mut want = vec![second, third];
    want.sort();
    assert_eq!(versions(&env), want, "the current and the displaced version only; {first} is gone");
}

#[test]
fn versions_modified_in_the_last_hour_are_kept_because_a_concurrent_install_may_not_have_linked_them() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    let mut made = Vec::new();
    for _ in 0..3 {
        assert_eq!(mac_install(&env, &checkout).exit, 0);
        made.push(linked_version(&env));
    }
    made.sort();
    assert_eq!(versions(&env), made, "three installs within the hour leave three versions");
}

#[test]
fn the_migrated_pi_legacy_is_removed_by_a_later_install_once_it_is_old() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    old_real_adapter(&env);
    assert_eq!(mac_install(&env, &checkout).exit, 0);
    let first = linked_version(&env);
    age(&share(&env).join("pi.legacy"), HOUR2);

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(!share(&env).join("pi.legacy").exists(), "pi.legacy is neither current nor displaced any more");
    assert!(share(&env).join(&first).exists(), "the displaced version stays");
}

#[test]
fn the_cleanup_removes_the_scratch_of_dead_pids_and_spares_a_live_install_and_a_young_version() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    let share = share(&env);
    fs::create_dir_all(&share).unwrap();
    let dead = {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    };
    let mut live = Command::new("sleep").arg("120").spawn().unwrap();
    let pid = live.id();
    let old_version = "pi.20260101T000000-1";
    let young_version = "pi.20260101T000100-2";
    for dir in [old_version, young_version, &format!("pi.tmp.{dead}"), &format!("pi.tmp.{pid}")] {
        fs::create_dir_all(share.join(dir)).unwrap();
    }
    for link in [format!("pi.link.{dead}"), format!("pi.link.{pid}")] {
        symlink(young_version, share.join(link)).unwrap();
    }
    // time-claim: a version older than 60 minutes goes, one modified a minute ago stays; the clock is set, not waited for.
    age(&share.join(old_version), HOUR2);
    age(&share.join(young_version), Duration::from_secs(60));

    let run = mac_install(&env, &checkout);
    let _ = live.kill();
    let _ = live.wait();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(!share.join(old_version).exists(), "an old version that is neither current nor displaced is removed");
    assert!(share.join(young_version).exists(), "a version modified a minute ago is kept");
    assert!(!share.join(format!("pi.tmp.{dead}")).exists(), "pi.tmp.<dead pid> is removed");
    assert!(fs::symlink_metadata(share.join(format!("pi.link.{dead}"))).is_err(), "pi.link.<dead pid> is removed");
    assert!(share.join(format!("pi.tmp.{pid}")).exists(), "pi.tmp.<live pid> is another install's");
    assert!(fs::symlink_metadata(share.join(format!("pi.link.{pid}"))).is_ok(), "pi.link.<live pid> is another install's");
}

#[test]
fn a_pi_that_is_a_regular_file_is_refused_before_the_binary_is_replaced() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    fs::create_dir_all(share(&env)).unwrap();
    let pi = share(&env).join("pi");
    fs::write(&pi, "not an adapter\n").unwrap();

    let run = mac_install(&env, &checkout);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("cpd install refused: "), "{}", run.stderr);
    assert!(run.stderr.contains(&format!("{} is neither a symlink nor a directory", pi.display())) || run.stderr.contains("pi is neither a symlink nor a directory"), "{}", run.stderr);
    assert!(!env.home().join(".local/bin/cpd").exists(), "the binary was not replaced");
    assert_eq!(fs::read_to_string(&pi).unwrap(), "not an adapter\n");
}

/// A directory of symlinks to the usual tools, `python3` not among them: a `PATH` on which python3 is missing.
fn path_without_python3(env: &TestEnv) -> String {
    let tools = env.root().join("tools");
    fs::create_dir_all(&tools).unwrap();
    let path = std::env::var_os("PATH").unwrap_or_default();
    for tool in [
        "git", "sed", "grep", "cmp", "basename", "dirname", "cp", "mv", "rm", "mkdir", "mktemp", "cat", "chmod", "date", "find", "ln", "readlink", "sleep", "kill", "tr",
        "uname", "env", "tmux", "flock", "touch", "true", "false", "test", "head", "tail", "id", "wc", "sort", "awk", "sh", "printf", "echo", "ls", "pwd", "expr",
    ] {
        if let Some(found) = std::env::split_paths(&path).map(|d| d.join(tool)).find(|p| p.is_file()) {
            let _ = symlink(found, tools.join(tool));
        }
    }
    assert!(!tools.join("python3").exists());
    let path = format!("{}:{}", env.bin().display(), tools.display());
    let probe = Command::new("/bin/sh").args(["-c", "command -v python3"]).env("PATH", &path).output().unwrap();
    assert!(!probe.status.success(), "python3 is still on the test's PATH");
    path
}

#[test]
fn a_missing_python3_is_refused_before_the_binary_is_replaced() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    let path = path_without_python3(&env);

    let run = install_env(&env, &checkout, &version("null"), "", &[("PATH", &path)]);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("cpd install refused: python3 is required for the atomic adapter swap"), "{}", run.stderr);
    assert!(!env.home().join(".local/bin/cpd").exists(), "the binary was not replaced");
    assert!(!share(&env).join("pi").exists(), "no adapter was installed");
}

#[test]
fn the_installer_goes_on_to_the_views_and_the_engine_after_migrating_on_a_python_without_renameat2() {
    let env = TestEnv::new();
    let checkout = Checkout::new(&env, &[]);
    mac_python(&env);
    fake_tmux(&env);
    old_real_adapter(&env);
    let _engines = Engines(vec![env.cpd_home().join("engine.pid")], vec![]);

    let run = install_with(&env, &checkout, &version("null"), &engine_restart_extra());

    assert_eq!(run.exit, 0, "{}", run.stderr);
    linked_version(&env);
    assert_eq!(follow_ups(&env), ["tui --respawn", "watch --ensure"], "the views are respawned and the engine restarted: {}", run.stderr);
    assert!(lock_held(&env), "the new engine holds the lock");
}

// ---- The portability check (hs-d355; docs/implementing.md "The portability check") ----
//
// `scripts/check-portable.sh [<file>...]` refuses Linux-only calls in the scripts Macs run; the Gate runs it.

const SCRIPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts");

struct Check {
    exit: i32,
    stdout: String,
    stderr: String,
}

impl Check {
    /// Both streams: the docs name the `ok` line without a stream.
    fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// Runs `script` with `args` in `cwd`.
fn run_check(script: &Path, cwd: &Path, args: &[&str]) -> Check {
    let out = common::output(Command::new(script).current_dir(cwd).args(args)).unwrap();
    Check { exit: out.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

/// Checks a fixture whose second line is `line` (the first is a shebang, a full-line comment).
fn check_line(line: &str) -> Check {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("fx.sh"), format!("#!/bin/sh\n{line}\n")).unwrap();
    run_check(&Path::new(SCRIPTS).join("check-portable.sh"), dir.path(), &["fx.sh"])
}

/// One Linux-only habit per entry (docs: the table of refused patterns): each is refused with `fx.sh:2:`.
const REFUSED: &[&str] = &[
    "python3 -c 'import ctypes; ctypes.CDLL(None).renameat2' a b",
    "RENAME_EXCHANGE=2",
    "cat /proc/sys/kernel/random/uuid",
    "cat /sys/class/net/eth0/address",
    "dir=$(readlink -f \"$0\")",
    "dir=$(readlink -e \"$0\")",
    "dir=$(readlink -m \"$0\")",
    "dir=$(realpath \"$0\")",
    "stat -c %Y file",
    "stat --format=%Y file",
    "date -d yesterday",
    "date --date=yesterday",
    "date +%s.%N",
    "date -r 1700000000",
    "sed -i 's/a/b/' file",
    "sed --in-place 's/a/b/' file",
    "sed -r 's/(a)/\\1/' file",
    "flock -n \"$lock\" true",
    "while ! flock -n \"$lock\" true; do sleep 1; done",
    "timeout 5 sleep 1",
    "setsid sleep 1 &",
    "ls | xargs -r rm",
    "ls | xargs --no-run-if-empty rm",
    "ls | xargs -d '\\n' rm",
    "grep -P 'a\\d' file",
    "grep --perl-regexp 'a' file",
    "sha256sum file",
    "sha1sum file",
    "md5sum file",
    "jobs=$(nproc)",
    "mv -T a b",
    "ln -T a b",
    "cp --reflink=auto a b",
    "cp --parents a b",
    "find . -printf '%p\\n'",
    "mktemp --tmpdir x.XXXXXX",
    "touch -d yesterday file",
    "base64 -w0 file",
    "sort -V file",
    "du -b file",
    "df --output=avail /",
    // a trailing comment is not a full-line comment
    "echo hi # see /proc/self",
];

/// The portable form of the same: each passes.
const ALLOWED: &[&str] = &[
    "python3 -c 'import os,sys; os.replace(sys.argv[1], sys.argv[2])' a b",
    "node -p 'crypto.randomUUID()'",
    "ps -o etime= -p \"$pid\"",
    "uname -s",
    "target=$(readlink \"$link\")",
    "dir=$(cd \"$dir\" && pwd -P)",
    "python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' \"$0\"",
    "python3 -c 'import os,sys; print(os.stat(sys.argv[1]).st_mtime)' file",
    "[ a -nt b ] && echo newer",
    "date -u +%Y%m%dT%H%M%S",
    "date +%s",
    "sed -i.bak 's/a/b/' file",
    "sed -E 's/(a)/\\1/' file",
    "sed -n 's/a/b/p' file",
    "python3 -c 'import fcntl,sys; fcntl.flock(open(sys.argv[1], \"a\"), fcntl.LOCK_EX | fcntl.LOCK_NB)' \"$lock\"",
    "sleep 1 &",
    "kill \"$pid\"",
    "python3 -c 'import subprocess; subprocess.run([\"true\"], timeout=5)'",
    "ls | while read -r f; do rm \"$f\"; done",
    "[ -n \"$list\" ] && rm $list",
    "grep -E 'a[0-9]' file",
    "shasum -a 256 file",
    "shasum -a 1 file",
    "jobs=$(getconf _NPROCESSORS_ONLN)",
    "mv a b",
    "ln -s a b",
    "cp a b",
    "find . -name x",
    "mktemp -d",
    "touch file",
    "base64 file",
    "sort -n file",
    "du -k file",
    "df -k /",
    "echo hi",
];

#[test]
fn every_refused_pattern_fails_with_the_file_and_line_and_an_alternative() {
    let mut wrong = Vec::new();
    for line in REFUSED {
        let run = check_line(line);
        let named = run.stderr.lines().any(|l| l.starts_with("fx.sh:2: ") && l.contains(": use "));
        if run.exit != 1 || !named || !run.stderr.trim_end().ends_with("check-portable: FAIL (1 hits)") {
            wrong.push(format!("`{line}`: exit {}, stderr {:?}", run.exit, run.stderr));
        }
    }
    assert!(wrong.is_empty(), "not refused as `<file>:<line>: <pattern>: use <alternative>` and `check-portable: FAIL (1 hits)`:\n{}", wrong.join("\n"));
}

#[test]
fn the_portable_alternative_of_every_pattern_passes() {
    let mut wrong = Vec::new();
    for line in ALLOWED {
        let run = check_line(line);
        if run.exit != 0 || !run.all().contains("check-portable: ok (1 files, 0 exempt)") {
            wrong.push(format!("`{line}`: exit {}, output {:?}", run.exit, run.all()));
        }
    }
    assert!(wrong.is_empty(), "refused, or no `check-portable: ok (1 files, 0 exempt)`:\n{}", wrong.join("\n"));
}

#[test]
fn a_full_line_comment_may_name_a_pattern() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("fx.sh"), "#!/bin/sh\n# renameat2 and /proc/ and readlink -f are Linux-only\n    # sed -i without a suffix\n\t# flock\necho hi\n").unwrap();
    let run = run_check(&Path::new(SCRIPTS).join("check-portable.sh"), dir.path(), &["fx.sh"]);
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(run.all().contains("check-portable: ok (1 files, 0 exempt)"), "{}", run.all());
}

#[test]
fn an_exempt_line_with_a_reason_passes_and_is_counted_and_one_without_a_reason_fails() {
    let ok = check_line("readlink -f \"$0\" # portable: a uname guard routes a Mac around this line");
    assert_eq!(ok.exit, 0, "{}", ok.stderr);
    assert!(ok.all().contains("check-portable: ok (1 files, 1 exempt)"), "{}", ok.all());

    for line in ["readlink -f \"$0\" # portable:", "readlink -f \"$0\" # portable:   ", "echo hi # portable:"] {
        let run = check_line(line);
        assert_eq!(run.exit, 1, "`{line}`: {}", run.stderr);
        assert!(run.stderr.contains("fx.sh:2: "), "`{line}`: a `# portable:` with no reason is a hit: {}", run.stderr);
        assert!(run.stderr.trim_end().ends_with("check-portable: FAIL (1 hits)"), "`{line}`: {}", run.stderr);
    }
}

#[test]
fn every_hit_is_listed_with_its_own_line_and_counted_and_only_the_failing_file_is_named() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.sh"), "#!/bin/sh\necho fine\n").unwrap();
    fs::write(dir.path().join("b.sh"), "#!/bin/sh\nnproc\necho fine\ncat /proc/cpuinfo\n").unwrap();
    let script = Path::new(SCRIPTS).join("check-portable.sh");

    let run = run_check(&script, dir.path(), &["a.sh", "b.sh"]);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("b.sh:2: ") && run.stderr.contains("b.sh:4: "), "{}", run.stderr);
    assert!(!run.stderr.contains("a.sh:"), "{}", run.stderr);
    assert!(run.stderr.trim_end().ends_with("check-portable: FAIL (2 hits)"), "{}", run.stderr);

    let both = run_check(&script, dir.path(), &["a.sh", "a.sh"]);
    assert_eq!(both.exit, 0, "{}", both.stderr);
    assert!(both.all().contains("check-portable: ok (2 files, 0 exempt)"), "{}", both.all());
}

/// A copy of the repository's `scripts/` in a temporary directory: what the check classifies when run without arguments.
fn copied_scripts() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("scripts")).unwrap();
    for entry in fs::read_dir(SCRIPTS).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            fs::copy(&path, dir.path().join("scripts").join(path.file_name().unwrap())).unwrap();
        }
    }
    dir
}

#[test]
fn the_covered_scripts_of_the_repository_pass() {
    // The Gate's run: no argument checks the covered list (nine scripts; the Linux-only ones are listed with a reason each).
    let root = Path::new(SCRIPTS).join("..");
    let run = run_check(&Path::new(SCRIPTS).join("check-portable.sh"), &root, &[]);
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(run.all().contains("check-portable: ok (9 files, "), "{}", run.all());
}

#[test]
fn a_script_in_neither_list_fails_so_a_new_script_cannot_skip_the_decision() {
    let dir = copied_scripts();
    let script = dir.path().join("scripts/check-portable.sh");
    let before = run_check(&script, dir.path(), &[]);
    assert_eq!(before.exit, 0, "the copy passes before the new script: {}", before.stderr);
    assert!(before.all().contains("check-portable: ok (9 files, "), "{}", before.all());

    fs::write(dir.path().join("scripts/brand-new.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    let run = run_check(&script, dir.path(), &[]);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("brand-new.sh: not classified: add it to the covered or the Linux-only list"), "{}", run.stderr);
    assert!(run.stderr.trim_end().ends_with("check-portable: FAIL (1 hits)"), "{}", run.stderr);
}

#[test]
fn a_linux_only_script_is_not_covered_and_a_covered_one_is_checked_by_the_default_run() {
    let dir = copied_scripts();
    let script = dir.path().join("scripts/check-portable.sh");
    // test-time.sh measures the Gate on hs (`date +%s.%N`): listed as Linux-only, not checked.
    let mut text = fs::read_to_string(dir.path().join("scripts/test-time.sh")).unwrap();
    text.push_str("\ndate +%s.%N\n");
    fs::write(dir.path().join("scripts/test-time.sh"), text).unwrap();
    let run = run_check(&script, dir.path(), &[]);
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(run.all().contains("check-portable: ok (9 files, "), "{}", run.all());

    let mut text = fs::read_to_string(dir.path().join("scripts/npm-fresh.sh")).unwrap();
    text.push_str("\nreadlink -f \"$0\"\n");
    fs::write(dir.path().join("scripts/npm-fresh.sh"), text).unwrap();
    let run = run_check(&script, dir.path(), &[]);
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.contains("scripts/npm-fresh.sh:"), "{}", run.stderr);
}
