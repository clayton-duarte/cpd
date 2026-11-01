//! `cpd install`'s adapter swap, its cleanup of old versions and scratch, and the compact-tools extension that
//! travels with the adapter (docs/implementing.md "The adapter swap"; contract §3.7), in-process on `Fakes`
//! (ADR 0013): the checkout is a clone through the git route, the built and the installed `cpd` are routes of the
//! runner, `HOME` and `CPD_HOME` are directories of a temp dir, a pid is alive when `FakeSpawn` says so and the age
//! of a version is its modification time against `Clock::utc` of the `ManualClock` (set, never waited for). No test
//! starts a process, runs git, reads the wall clock or sleeps.
//!
//! Each test is the test of the same name of `cli/tests/tooling/install_script.rs` (group B), which ran
//! `scripts/install-cpd.sh` with a fake `python3`, with these changes of subject:
//! - the three tests that named a python keep their other assertions and lose the word: the swap is `fs::rename`
//!   and there is no `python3` (`a_first_install_..._on_a_python_without_renameat2` and
//!   `the_installer_goes_on_..._on_a_python_without_renameat2` lose `on_a_python`);
//! - the rename that fails is made to fail by a read-only `share/` (docs: the tests), not by a `python3` whose
//!   `os.replace` fails; the reason on stderr is the operating system's;
//! - the clock moves one second between two installs of one rig (a version is named by the UTC second and the pid);
//! - `a_missing_python3_is_refused_before_the_binary_is_replaced` has no subject any more and is not written.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use tempfile::TempDir;
use time::OffsetDateTime;

use super::{Install, run};
use crate::fakes::git::{GitRoute, Repo, Universe};
use crate::fakes::tmux::TmuxRoute;
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::host::spawn::{Launch, Pid, Spawn as _};
use crate::ports::Fakes;

const MIGRATIONS: &str = "core/src/store/migrations";
const HOUR2: Duration = Duration::from_secs(2 * 3600);

/// What one run of the install left: its exit and its stderr.
struct Run {
    exit: i32,
    stderr: String,
}

/// A checkout (a clone of an origin whose `main` is `origin/main`), a built binary that says its store opens or does
/// not exist, a `HOME` and a `CPD_HOME`.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    route: Arc<GitRoute>,
    install: Install,
}

impl Rig {
    /// The checkout, with no `cpd` route for the installed binary yet.
    fn bare() -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let fakes = Fakes::new();
        let mut universe = Universe::new(fakes.ports.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        let universe = Arc::new(Mutex::new(universe));
        let route = Arc::new(GitRoute::new(universe.clone()));

        let home = root.join("home");
        let cpd_home = root.join("cpd-home");
        let target_dir = root.join("target");
        for dir in [
            &home,
            &cpd_home,
            &root.join("repos"),
            &target_dir.join("release"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        let built = target_dir.join("release/cpd");
        fs::write(&built, "built cpd\n").unwrap();
        fs::set_permissions(&built, fs::Permissions::from_mode(0o700)).unwrap();

        let origin = Repo::init(&universe, &root.join("repos/install-origin"));
        for script in ["install-cpd.sh", "cargo.sh", "tag-version.sh"] {
            origin.write(&format!("scripts/{script}"), &format!("{script}\n"));
        }
        for file in [
            "package.json",
            "extension.ts",
            "lib/cpd.ts",
            "lib/adapter.ts",
            "test/cpd.test.ts",
        ] {
            origin.write(&format!("pi/{file}"), &format!("{file}\n"));
        }
        origin.write(&format!("{MIGRATIONS}/.keep"), "");
        origin.commit("install files");

        let rig = Rig {
            _tmp: tmp,
            fakes,
            route,
            install: Install {
                root: root.join("repos/install-clone"),
                target_dir,
                home,
                cpd_home: CpdHome::resolve(Some(OsString::from(cpd_home)), None).unwrap(),
                pid: 4242,
                lock_wait: super::DEFAULT_LOCK_WAIT,
            },
        };
        (*rig.fakes.runner).clone().with_git(&rig.route);
        let clone = git(
            &rig,
            &root,
            &[
                "clone",
                "-q",
                origin.dir().to_str().unwrap(),
                rig.install.root.to_str().unwrap(),
            ],
        );
        assert_eq!(clone.0, 0, "the fixture's clone: {}", clone.1);
        rig
    }

    /// The checkout, a built binary whose store does not exist, and an installed `cpd` and a tagger that answer every
    /// call with exit 0 and no output.
    fn new() -> Rig {
        let rig = Rig::bare();
        rig.built_says_no_store();
        let (installed, tag) = (
            rig.installed_cpd(),
            rig.install.root.join("scripts/tag-version.sh"),
        );
        (*rig.fakes.runner)
            .clone()
            .on(
                "cpd",
                move |c| Path::new(&c.program) == installed,
                Reply::ok(""),
            )
            .on(
                "tag-version.sh",
                move |c| Path::new(&c.program) == tag,
                Reply::ok(""),
            );
        rig
    }

    fn built(&self) -> PathBuf {
        self.install.target_dir.join("release/cpd")
    }

    fn installed_cpd(&self) -> PathBuf {
        self.install.home.join(".local/bin/cpd")
    }

    /// `<HOME>/.local/share/cpd`.
    fn share(&self) -> PathBuf {
        self.install.home.join(".local/share/cpd")
    }

    fn built_says_no_store(&self) {
        let built = self.built();
        let says = r#"{"ok":true,"version":"0.0.0","protocol":1,"store":null}"#;
        let migrated = r#"{"ok":true,"migrated":[]}"#;
        let version = built.clone();
        let migrate = built;
        (*self.fakes.runner)
            .clone()
            .on(
                "cpd",
                move |c| Path::new(&c.program) == version && c.args == ["--version"],
                Reply::ok(says),
            )
            .on(
                "cpd",
                move |c| Path::new(&c.program) == migrate && c.args == ["config", "--migrate"],
                Reply::ok(migrated),
            );
    }

    /// Runs the install on the rig's ports.
    fn run(&self) -> Run {
        let mut stderr = Vec::new();
        let exit = run(&self.fakes.ports, &self.install, &mut stderr);
        self.route.assert_supported();
        Run {
            exit,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }

    /// One more install, a second of virtual time after the last: a version is named by the UTC second and the pid.
    /// Afterwards every version of `share/` is stamped as made at the virtual time (a directory's modification time
    /// is the real one, which the virtual clock knows nothing of).
    fn install_again(&self) -> Run {
        self.fakes.clock.advance(Duration::from_secs(1));
        let run = self.run();
        self.stamp_versions(Duration::ZERO);
        run
    }

    fn utc(&self) -> OffsetDateTime {
        use crate::clock::Clock as _;
        self.fakes.clock.utc()
    }

    /// Sets the modification time of `path` to `age` before the virtual now.
    fn age(&self, path: &Path, age: Duration) {
        let at = u64::try_from(self.utc().unix_timestamp()).unwrap() - age.as_secs();
        fs::File::open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(at))
            .unwrap();
    }

    fn stamp_versions(&self, age: Duration) {
        for name in self.versions() {
            self.age(&self.share().join(name), age);
        }
    }

    /// What `pi` points to: panics unless it is a symlink with a relative target that is a `pi.<id>`.
    fn linked_version(&self) -> String {
        let pi = self.share().join("pi");
        let meta = fs::symlink_metadata(&pi).unwrap_or_else(|e| panic!("{}: {e}", pi.display()));
        assert!(
            meta.file_type().is_symlink(),
            "{} is not a symlink",
            pi.display()
        );
        let target = fs::read_link(&pi).unwrap().to_string_lossy().into_owned();
        assert!(
            is_version(&target) && target != "pi.legacy",
            "the target `{target}` is not a pi.<id> beside the link (a relative target)"
        );
        target
    }

    /// The names in `share/` that are versions (`pi.<id>`, `pi.legacy`), sorted.
    fn versions(&self) -> Vec<String> {
        self.names(is_version)
    }

    /// The `pi.tmp.<pid>` and `pi.link.<pid>` entries of `share/`.
    fn scratch(&self) -> Vec<String> {
        self.names(|n| n.starts_with("pi.tmp.") || n.starts_with("pi.link."))
    }

    fn names(&self, keep: impl Fn(&str) -> bool) -> Vec<String> {
        // No `share/` is no entry: the install makes it, and a test that finds none is told so by its own assertion.
        let entries = fs::read_dir(self.share())
            .map(|d| d.collect::<Vec<_>>())
            .unwrap_or_default();
        let mut names: Vec<String> = entries
            .into_iter()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| keep(n))
            .collect();
        names.sort();
        names
    }

    /// A real adapter directory as installs before hs-d355 left it.
    fn old_real_adapter(&self) -> PathBuf {
        let pi = self.share().join("pi");
        fs::create_dir_all(pi.join("lib")).unwrap();
        for (file, text) in [
            ("package.json", "old\n"),
            ("extension.ts", "old\n"),
            ("lib/old-only.ts", "old\n"),
        ] {
            fs::write(pi.join(file), text).unwrap();
        }
        pi
    }
}

/// `git <args>` in `dir`, through the runner: (exit, stderr).
fn git(rig: &Rig, dir: &Path, args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    let out = rig
        .fakes
        .ports
        .runner
        .output(&mut cmd, "git", Duration::from_secs(60))
        .unwrap_or_else(|e| panic!("git {args:?}: {e:?}"));
    (
        std::os::unix::process::ExitStatusExt::into_raw(out.status) >> 8,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A `pi.<id>` directory name: `<UTC YYYYmmddTHHMMSS>-<pid>`, or `pi.legacy`.
fn is_version(name: &str) -> bool {
    let Some(id) = name.strip_prefix("pi.") else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    id == "legacy"
        || id.split_once('-').is_some_and(|(stamp, pid)| {
            stamp.len() == 15
                && stamp.as_bytes()[8] == b'T'
                && digits(&stamp[..8])
                && digits(&stamp[9..])
                && digits(pid)
        })
}

#[test]
fn a_first_install_links_pi_to_a_versioned_directory_without_renameat2() {
    let rig = Rig::new();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = rig.linked_version();
    let pi = rig.share().join("pi");
    assert_eq!(
        fs::read_to_string(pi.join("extension.ts")).unwrap(),
        "extension.ts\n"
    );
    assert!(pi.join("package.json").exists(), "pi/package.json");
    assert!(
        !pi.join("test").exists(),
        "tests are not part of the install"
    );
    assert_eq!(rig.versions(), [id], "one version, no pi.legacy");
    assert_eq!(
        rig.scratch(),
        Vec::<String>::new(),
        "no pi.tmp.<pid> or pi.link.<pid> is left"
    );
}

#[test]
fn the_installed_tree_can_be_moved_because_the_link_is_relative() {
    let rig = Rig::new();
    assert_eq!(rig.run().exit, 0);
    rig.linked_version();

    let moved = rig.install.home.join("moved-share");
    fs::rename(rig.share(), &moved).unwrap();

    assert!(
        moved.join("pi/package.json").exists(),
        "the link still resolves after the tree moved"
    );
}

#[test]
fn an_existing_real_adapter_directory_is_migrated_to_pi_legacy_without_renameat2() {
    let rig = Rig::new();
    rig.old_real_adapter();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = rig.linked_version();
    let share = rig.share();
    assert_eq!(
        fs::read_to_string(share.join("pi/extension.ts")).unwrap(),
        "extension.ts\n",
        "the new adapter is visible at pi"
    );
    assert!(
        !share.join("pi/lib/old-only.ts").exists(),
        "no old file shows through the new pi"
    );
    assert_eq!(
        fs::read_to_string(share.join("pi.legacy/lib/old-only.ts")).unwrap(),
        "old\n",
        "pi.legacy holds the old files"
    );
    let mut want = vec!["pi.legacy".to_string(), id];
    want.sort();
    assert_eq!(rig.versions(), want);
    assert_eq!(rig.scratch(), Vec::<String>::new());
}

/// Puts `share/`'s mode back when it drops, so the temp dir can be removed after a failed assertion.
struct Writable(PathBuf);

impl Drop for Writable {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn a_failing_swap_of_the_migration_puts_the_old_directory_back_and_fails_the_install() {
    let rig = Rig::new();
    let pi = rig.old_real_adapter();
    // The renames of the swap fail: nothing can be created or renamed in a read-only `share/`.
    let _writable = Writable(rig.share());
    fs::set_permissions(rig.share(), fs::Permissions::from_mode(0o555)).unwrap();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("cpd install failed: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("Permission denied"),
        "the reason is on stderr: {}",
        run.stderr
    );
    assert!(
        fs::symlink_metadata(&pi).unwrap().file_type().is_dir(),
        "pi is the old real directory again"
    );
    assert_eq!(
        fs::read_to_string(pi.join("lib/old-only.ts")).unwrap(),
        "old\n"
    );
    assert!(
        !rig.share().join("pi.legacy").exists(),
        "pi.legacy was renamed back"
    );
}

#[test]
fn an_existing_symlink_is_swapped_and_its_version_is_kept_as_the_displaced_one() {
    let rig = Rig::new();
    let share = rig.share();
    fs::create_dir_all(share.join("pi.20260101T000000-1")).unwrap();
    fs::write(share.join("pi.20260101T000000-1/package.json"), "old\n").unwrap();
    symlink("pi.20260101T000000-1", share.join("pi")).unwrap();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = rig.linked_version();
    assert_ne!(id, "pi.20260101T000000-1");
    assert_eq!(
        fs::read_to_string(share.join("pi/extension.ts")).unwrap(),
        "extension.ts\n"
    );
    assert_eq!(
        fs::read_to_string(share.join("pi.20260101T000000-1/package.json")).unwrap(),
        "old\n",
        "the displaced version stays one install"
    );
}

#[test]
fn a_second_install_shows_the_new_content_and_keeps_the_displaced_version() {
    let rig = Rig::new();
    assert_eq!(rig.install_again().exit, 0);
    let first = rig.linked_version();
    fs::write(rig.install.root.join("pi/extension.ts"), "second install\n").unwrap();

    let run = rig.install_again();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let second = rig.linked_version();
    assert_ne!(first, second);
    let share = rig.share();
    assert_eq!(
        fs::read_to_string(share.join("pi/extension.ts")).unwrap(),
        "second install\n",
        "the new content is visible through pi"
    );
    assert_eq!(
        fs::read_to_string(share.join(&first).join("extension.ts")).unwrap(),
        "extension.ts\n",
        "the displaced version is whole and kept"
    );
}

#[test]
fn installs_one_after_another_leave_the_current_and_the_displaced_version_once_the_others_are_old()
{
    let rig = Rig::new();
    assert_eq!(rig.install_again().exit, 0);
    let first = rig.linked_version();
    assert_eq!(rig.install_again().exit, 0);
    let second = rig.linked_version();
    // time-claim: the cleanup spares a version modified in the last 60 minutes; the clock is set, not waited for.
    rig.age(&rig.share().join(&first), HOUR2);
    rig.age(&rig.share().join(&second), HOUR2);

    let run = rig.install_again();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let third = rig.linked_version();
    let mut want = vec![second, third];
    want.sort();
    assert_eq!(
        rig.versions(),
        want,
        "the current and the displaced version only; {first} is gone"
    );
}

#[test]
fn versions_modified_in_the_last_hour_are_kept_because_a_concurrent_install_may_not_have_linked_them()
 {
    let rig = Rig::new();
    let mut made = Vec::new();
    for _ in 0..3 {
        assert_eq!(rig.install_again().exit, 0);
        made.push(rig.linked_version());
    }
    made.sort();
    assert_eq!(
        rig.versions(),
        made,
        "three installs within the hour leave three versions"
    );
}

#[test]
fn the_migrated_pi_legacy_is_removed_by_a_later_install_once_it_is_old() {
    let rig = Rig::new();
    rig.old_real_adapter();
    assert_eq!(rig.install_again().exit, 0);
    let first = rig.linked_version();
    rig.age(&rig.share().join("pi.legacy"), HOUR2);

    let run = rig.install_again();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        !rig.share().join("pi.legacy").exists(),
        "pi.legacy is neither current nor displaced any more"
    );
    assert!(
        rig.share().join(&first).exists(),
        "the displaced version stays"
    );
}

#[test]
fn the_cleanup_removes_the_scratch_of_dead_pids_and_spares_a_live_install_and_a_young_version() {
    let rig = Rig::new();
    let share = rig.share();
    fs::create_dir_all(&share).unwrap();
    // `FakeSpawn` knows two pids: one runs, one exited.
    let launch = Launch {
        program: "install".into(),
        ..Launch::default()
    };
    let spawn = &rig.fakes.spawn;
    let (Pid(live), dead) = (spawn.spawn(&launch).unwrap(), spawn.spawn(&launch).unwrap());
    spawn.exit(dead, 0);
    let dead = dead.0;
    assert!(spawn.alive(Pid(live)) && !spawn.alive(Pid(dead)));
    let old_version = "pi.20260101T000000-1";
    let young_version = "pi.20260101T000100-2";
    for dir in [
        old_version,
        young_version,
        &format!("pi.tmp.{dead}"),
        &format!("pi.tmp.{live}"),
    ] {
        fs::create_dir_all(share.join(dir)).unwrap();
    }
    for link in [format!("pi.link.{dead}"), format!("pi.link.{live}")] {
        symlink(young_version, share.join(link)).unwrap();
    }
    // time-claim: a version older than 60 minutes goes, one modified a minute ago stays; the clock is set, not waited for.
    rig.age(&share.join(old_version), HOUR2);
    rig.age(&share.join(young_version), Duration::from_secs(60));

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        !share.join(old_version).exists(),
        "an old version that is neither current nor displaced is removed"
    );
    assert!(
        share.join(young_version).exists(),
        "a version modified a minute ago is kept"
    );
    assert!(
        !share.join(format!("pi.tmp.{dead}")).exists(),
        "pi.tmp.<dead pid> is removed"
    );
    assert!(
        fs::symlink_metadata(share.join(format!("pi.link.{dead}"))).is_err(),
        "pi.link.<dead pid> is removed"
    );
    assert!(
        share.join(format!("pi.tmp.{live}")).exists(),
        "pi.tmp.<live pid> is another install's"
    );
    assert!(
        fs::symlink_metadata(share.join(format!("pi.link.{live}"))).is_ok(),
        "pi.link.<live pid> is another install's"
    );
}

/// Two pids: the first runs, the second exited.
fn live_and_dead_pids(rig: &Rig) -> (u32, u32) {
    let launch = Launch {
        program: "install".into(),
        ..Launch::default()
    };
    let spawn = &rig.fakes.spawn;
    let (live, dead) = (spawn.spawn(&launch).unwrap(), spawn.spawn(&launch).unwrap());
    spawn.exit(dead, 0);
    assert!(spawn.alive(live) && !spawn.alive(dead));
    (live.0, dead.0)
}

/// One install after every version of `share/` was last modified two hours ago (the clock is set, not waited for).
fn install_once_everything_is_old(rig: &Rig) -> Run {
    for name in rig.versions() {
        rig.age(&rig.share().join(name), HOUR2);
    }
    rig.fakes.clock.advance(Duration::from_secs(1));
    let run = rig.run();
    assert_eq!(run.exit, 0, "{}", run.stderr);
    run
}

/// Installs once (version V1), pins V1 with `pi.pin.<pid>`, adds an old unpinned version V2, then installs twice more
/// with everything old, so V1 is neither current nor displaced. Returns V1, V2 and the pin's path.
fn pin_v1_then_install_twice(rig: &Rig, pid: u32) -> (String, String, PathBuf) {
    assert_eq!(rig.install_again().exit, 0);
    let v1 = rig.linked_version();
    let share = rig.share();
    let pin = share.join(format!("pi.pin.{pid}"));
    symlink(&v1, &pin).unwrap();
    let v2 = "pi.20260101T000000-1".to_string();
    fs::create_dir_all(share.join(&v2)).unwrap();
    install_once_everything_is_old(rig);
    install_once_everything_is_old(rig);
    assert_ne!(rig.linked_version(), v1);
    (v1, v2, pin)
}

#[test]
fn a_version_a_live_pid_pins_survives_two_later_installs_once_it_is_old() {
    let rig = Rig::new();
    let (live, _) = live_and_dead_pids(&rig);

    let (v1, v2, pin) = pin_v1_then_install_twice(&rig, live);

    let share = rig.share();
    assert!(
        share.join(&v1).join("package.json").exists(),
        "{v1}, pinned by a live pid, is kept whatever its age"
    );
    assert_eq!(
        fs::read_link(&pin).unwrap().to_string_lossy(),
        v1,
        "the live pin is kept"
    );
    assert!(
        !share.join(&v2).exists(),
        "{v2}, old and unpinned, is removed"
    );
}

#[test]
fn a_pin_of_a_dead_pid_is_removed_and_the_old_version_it_named_is_swept() {
    let rig = Rig::new();
    let (_, dead) = live_and_dead_pids(&rig);

    let (v1, _, pin) = pin_v1_then_install_twice(&rig, dead);

    assert!(
        fs::symlink_metadata(&pin).is_err(),
        "the pin of a dead pid is removed"
    );
    assert!(
        !rig.share().join(&v1).exists(),
        "{v1}, old and no longer pinned, is removed"
    );
}

#[test]
fn a_pin_that_names_no_pid_is_removed() {
    let rig = Rig::new();
    let share = rig.share();
    fs::create_dir_all(&share).unwrap();
    let version = "pi.20260101T000100-2";
    fs::create_dir_all(share.join(version)).unwrap();
    let pin = share.join("pi.pin.abc");
    symlink(version, &pin).unwrap();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        fs::symlink_metadata(&pin).is_err(),
        "pi.pin.abc names no pid and is removed"
    );
}

#[test]
fn a_pin_naming_something_that_is_not_a_version_is_removed_and_never_followed() {
    let rig = Rig::new();
    let (_, dead) = live_and_dead_pids(&rig);
    let share = rig.share();
    fs::create_dir_all(&share).unwrap();
    // A real directory with a file, outside `share/` and inside the rig's temp dir.
    let outside = rig.install.home.parent().unwrap().join("outside-dir");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "mine\n").unwrap();
    let pin = share.join(format!("pi.pin.{dead}"));
    symlink(&outside, &pin).unwrap();

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        fs::symlink_metadata(&pin).is_err(),
        "the pin of an exited pid is removed"
    );
    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).unwrap(),
        "mine\n",
        "the target was never followed"
    );
}

#[test]
fn a_pi_that_is_a_regular_file_is_refused_before_the_binary_is_replaced() {
    let rig = Rig::new();
    fs::create_dir_all(rig.share()).unwrap();
    let pi = rig.share().join("pi");
    fs::write(&pi, "not an adapter\n").unwrap();

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("cpd install refused: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains(&format!(
            "{} is neither a symlink nor a directory",
            pi.display()
        )),
        "{}",
        run.stderr
    );
    assert!(!rig.installed_cpd().exists(), "the binary was not replaced");
    assert_eq!(fs::read_to_string(&pi).unwrap(), "not an adapter\n");
}

const CONFIG: &str = r#"{"ok":true,"config":{"tmux":{"socket":"s","cockpit_session":"cock","work_session":"work"}}}"#;

/// The new `cpd` as a Lead's install finds it: it answers the follow-ups and its `watch --ensure` starts an engine, a
/// holder of `locks/watch.lock` (the `File` is kept until the handler drops). Every call is logged by the runner.
struct NewCpd {
    lock: PathBuf,
    engine: Mutex<Vec<fs::File>>,
}

impl Handler for NewCpd {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args.as_slice() {
            ["config"] => Reply::ok(CONFIG),
            ["watch", "--check"] => Reply::ok(r#"{"ok":true}"#),
            ["watch", "--mark-restart"] => Reply::ok(r#"{"ok":true,"marked":true,"engine_pid":1}"#),
            ["watch", "--ensure"] => {
                fs::create_dir_all(self.lock.parent().unwrap()).unwrap();
                let file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(&self.lock)
                    .unwrap();
                assert!(
                    crate::lock::try_lock(&file).unwrap(),
                    "the engine's lock was free"
                );
                self.engine.lock().unwrap().push(file);
                Reply::ok(r#"{"ok":true,"started":true}"#)
            }
            _ => Reply::ok(""),
        }
    }
}

/// The follow-ups the installed `cpd` was asked for, in order.
fn follow_ups(rig: &Rig) -> Vec<String> {
    let installed = rig.installed_cpd();
    rig.fakes
        .runner
        .calls()
        .into_iter()
        .filter(|c| Path::new(&c.program) == installed)
        .map(|c| c.args.join(" "))
        .filter(|c| c.starts_with("tui") || c.starts_with("watch"))
        .collect()
}

/// Whether some other open file description holds the engine's lock.
fn lock_held(rig: &Rig) -> bool {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(rig.install.cpd_home.watch_lock())
        .unwrap();
    !crate::lock::try_lock(&file).unwrap()
}

#[test]
fn the_installer_goes_on_to_the_views_and_the_engine_after_migrating_without_renameat2() {
    let rig = Rig::bare();
    rig.built_says_no_store();
    rig.old_real_adapter();
    // The cockpit has `main`, the work session has `watch`; a window killed is a call the route answers.
    let tmux = TmuxRoute::new()
        .script(
            |a| a.iter().any(|x| x == "list-windows") && a.iter().any(|x| x.contains("cock")),
            Reply::ok("main\n"),
        )
        .script(
            |a| a.iter().any(|x| x == "list-windows") && a.iter().any(|x| x.contains("work")),
            Reply::ok("watch\n"),
        )
        .script(|a| a.iter().any(|x| x == "kill-window"), Reply::ok(""));
    let engine = Arc::new(NewCpd {
        lock: rig.install.cpd_home.watch_lock(),
        engine: Mutex::default(),
    });
    let tag = rig.install.root.join("scripts/tag-version.sh");
    (*rig.fakes.runner)
        .clone()
        .on(
            "tag-version.sh",
            move |c| Path::new(&c.program) == tag,
            Reply::ok(""),
        )
        .route("tmux", Arc::new(tmux))
        .route("cpd", engine);

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    rig.linked_version();
    assert_eq!(
        follow_ups(&rig),
        [
            "watch --check",
            "tui --respawn",
            "watch --mark-restart",
            "watch --ensure"
        ],
        "the views are respawned and the engine restarted: {}",
        run.stderr
    );
    assert!(lock_held(&rig), "the new engine holds the lock");
}

// ---- The compact-tools extension travels with the adapter (contract §6.9; docs/implementing.md "The compact-tools extension" point 5) ----
//
// The install copies `pi/` minus `test/`: `pi/compact-tools/` (the sources, the schema, LICENSE, NOTICE) is installed whole, so the
// installed `package.json`'s entry resolves, and `pi/test/compact-tools/` goes with `pi/test`. These run the install over the real
// `pi/` of this checkout, not the small adapter of the rig.

/// Copies the directory `from` to `to`, files and directories only.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// The checkout's `pi/` replaced by a copy of this repository's own `pi/`.
fn real_adapter(rig: &Rig) {
    let ours = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../pi"));
    let pi = rig.install.root.join("pi");
    fs::remove_dir_all(&pi).unwrap();
    copy_tree(ours, &pi);
}

/// Every directory under `dir` (at any depth) named `name`.
fn directories_named(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == name) {
                found.push(path.clone());
            }
            found.extend(directories_named(&path, name));
        }
    }
    found
}

#[test]
fn an_install_carries_the_compact_tools_extension_listed_in_its_package_json_and_leaves_every_test_directory_out()
 {
    let rig = Rig::new();
    real_adapter(&rig);
    assert!(
        rig.install.root.join("pi/test/compact-tools").is_dir(),
        "the source tree has the extension's tests under pi/test/compact-tools"
    );

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let id = rig.linked_version();
    let pi = rig.share().join("pi");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi.join("package.json")).unwrap()).unwrap();
    let extensions: Vec<&str> = manifest["pi"]["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    assert_eq!(
        extensions,
        [
            "./extension.ts",
            "./mcp-extension.ts",
            "./compact-tools/extensions/compact-tools.ts",
            "./ask-user-question/index.ts"
        ],
        "the installed package.json"
    );
    for entry in &extensions {
        assert!(
            pi.join(entry).is_file(),
            "the installed entry {entry} resolves"
        );
    }
    for file in [
        "extensions/compact-tools-config.ts",
        "extensions/compact-tools-double-load.ts",
        "schemas/compact-tools.schema.json",
        "LICENSE",
        "NOTICE",
        "header.ts",
    ] {
        assert!(
            pi.join("compact-tools").join(file).is_file(),
            "compact-tools/{file} is installed"
        );
    }
    // The tests are under pi/test, which the install leaves out whole: pi/test/compact-tools goes with it.
    assert_eq!(
        directories_named(&rig.share().join(&id), "test"),
        Vec::<PathBuf>::new(),
        "no test directory anywhere under {id}"
    );
}

// ---- the adapter's production packages, installed with pnpm beside `pi/` (docs/implementing.md `## cpd install`) ----

const PACKAGE_WITH_DEPENDENCIES: &str =
    "{\n  \"name\": \"cpd\",\n  \"dependencies\": {\"left-pad\": \"1.3.0\"}\n}\n";
const PNPM_LOCK: &str = "lockfileVersion: '9.0'\n# a lock\n";
const PNPM_WORKSPACE: &str = "packages: []\n";
const PNPM_REFUSED: &str = "pnpm install --prod --frozen-lockfile failed in";

impl Rig {
    /// Writes `text` at `path` of the checkout (the working tree, as the install reads it).
    fn write_in_checkout(&self, path: &str, text: &str) {
        let file = self.install.root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    /// The checkout has a root `package.json` with dependencies, a `pnpm-lock.yaml` and a `pnpm-workspace.yaml`.
    fn with_pnpm_files(&self) {
        self.write_in_checkout("package.json", PACKAGE_WITH_DEPENDENCIES);
        self.write_in_checkout("pnpm-lock.yaml", PNPM_LOCK);
        self.write_in_checkout("pnpm-workspace.yaml", PNPM_WORKSPACE);
    }

    /// A scripted `pnpm` that answers `reply` to every call.
    fn pnpm_answers(&self, reply: Reply) {
        (*self.fakes.runner).clone().on("pnpm", |_| true, reply);
    }

    fn pnpm_calls(&self) -> Vec<Call> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| c.program.rsplit('/').next() == Some("pnpm"))
            .collect()
    }

    /// An installed `cpd` and a `pi` link of an earlier install: (binary, link target).
    fn earlier_install(&self) -> (PathBuf, String) {
        let bin = self.installed_cpd();
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, "old cpd\n").unwrap();
        let old = "pi.20260101T000000-1";
        fs::create_dir_all(self.share().join(old)).unwrap();
        symlink(old, self.share().join("pi")).unwrap();
        (bin, old.to_string())
    }
}

#[test]
fn a_package_json_with_dependencies_is_installed_beside_the_adapter_with_pnpm() {
    let rig = Rig::new();
    rig.with_pnpm_files();
    let (installed, seen) = (rig.installed_cpd(), Arc::new(Mutex::new(Vec::new())));
    let log = seen.clone();
    (*rig.fakes.runner).clone().on(
        "pnpm",
        move |_| {
            log.lock().unwrap().push(installed.exists());
            true
        },
        Reply::ok(""),
    );

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let calls = rig.pnpm_calls();
    assert_eq!(calls.len(), 1, "exactly one pnpm call: {calls:?}");
    assert_eq!(calls[0].args, ["install", "--prod", "--frozen-lockfile"]);
    assert_eq!(calls[0].cwd.as_deref(), Some(rig.share().as_path()));
    assert_eq!(calls[0].deadline, Duration::from_secs(600));
    for (file, text) in [
        ("package.json", PACKAGE_WITH_DEPENDENCIES),
        ("pnpm-lock.yaml", PNPM_LOCK),
        ("pnpm-workspace.yaml", PNPM_WORKSPACE),
    ] {
        assert_eq!(
            fs::read(rig.share().join(file)).unwrap(),
            text.as_bytes(),
            "{file} in share"
        );
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [false],
        "the installed cpd does not exist yet when pnpm runs"
    );
    assert!(
        rig.installed_cpd().exists(),
        "the binary is replaced afterwards"
    );
}

#[test]
fn a_package_json_without_dependencies_runs_no_pnpm() {
    let variants: [(&str, Option<&str>); 3] = [
        (
            "only devDependencies",
            Some("{\"devDependencies\": {\"left-pad\": \"1.3.0\"}}\n"),
        ),
        ("empty dependencies", Some("{\"dependencies\": {}}\n")),
        ("no package.json", None),
    ];
    for (what, package) in variants {
        let rig = Rig::new();
        if let Some(text) = package {
            rig.write_in_checkout("package.json", text);
        }

        let run = rig.run();

        assert_eq!(run.exit, 0, "{what}: {}", run.stderr);
        assert!(rig.pnpm_calls().is_empty(), "{what}: no pnpm call");
        assert!(
            !rig.share().join("package.json").exists(),
            "{what}: no share/package.json"
        );
        assert!(
            rig.fakes.runner.unscripted().is_empty(),
            "{what}: {:?}",
            rig.fakes.runner.unscripted()
        );
    }
}

#[test]
fn dependencies_without_a_pnpm_lockfile_refuse_the_install() {
    let rig = Rig::new();
    rig.write_in_checkout("package.json", PACKAGE_WITH_DEPENDENCIES);
    rig.pnpm_answers(Reply::ok(""));

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("cpd install refused: package.json has dependencies but the checkout has no pnpm-lock.yaml"),
        "{}",
        run.stderr
    );
    assert!(rig.pnpm_calls().is_empty(), "no pnpm call");
    assert!(!rig.installed_cpd().exists(), "the binary is not installed");
    assert!(
        fs::symlink_metadata(rig.share().join("pi")).is_err(),
        "pi is not linked"
    );
}

#[test]
fn a_failing_pnpm_refuses_the_install_before_the_binary_is_replaced() {
    let rig = Rig::new();
    rig.with_pnpm_files();
    let (bin, old) = rig.earlier_install();
    rig.pnpm_answers(Reply::exit(1, "", "ERR_PNPM_OUTDATED_LOCKFILE\n"));

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    let want = format!(
        "cpd install refused: {PNPM_REFUSED} {}:",
        rig.share().display()
    );
    assert!(run.stderr.starts_with(&want), "{}", run.stderr);
    assert!(
        run.stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
        "{}",
        run.stderr
    );
    assert_eq!(
        fs::read_to_string(bin).unwrap(),
        "old cpd\n",
        "the installed binary is untouched"
    );
    assert_eq!(
        fs::read_link(rig.share().join("pi"))
            .unwrap()
            .to_string_lossy(),
        old,
        "pi keeps its old target"
    );
}

#[test]
fn a_missing_pnpm_refuses_the_install_naming_pnpm() {
    let rig = Rig::new();
    rig.with_pnpm_files();
    let (bin, old) = rig.earlier_install();
    rig.pnpm_answers(Reply::io(std::io::ErrorKind::NotFound));

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    let want = format!(
        "cpd install refused: {PNPM_REFUSED} {}:",
        rig.share().display()
    );
    assert!(run.stderr.starts_with(&want), "{}", run.stderr);
    assert_eq!(
        fs::read_to_string(bin).unwrap(),
        "old cpd\n",
        "the installed binary is untouched"
    );
    assert_eq!(
        fs::read_link(rig.share().join("pi"))
            .unwrap()
            .to_string_lossy(),
        old,
        "pi keeps its old target"
    );
}
