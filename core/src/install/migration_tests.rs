//! `cpd install`'s migration check (the branch check, contract §3.7), the verdict of the built binary (the store
//! check) and the binary replace, in-process on `Fakes` (ADR 0013): the checkout is a clone, through the git route,
//! of an origin that is its own `origin/main`; the built `cpd` and the installed one are `cpd` routes of the
//! runner, answering by the path they are run at; `HOME` and `CPD_HOME` are directories of a temp dir. No test
//! starts a process, runs git, reads the wall clock or sleeps.
//!
//! Each of the first ten tests is the test of the same name of `cli/tests/tooling/install_script.rs`, which ran
//! `scripts/install-cpd.sh` with a fake cargo, a fake `cpd` and a temporary git repo. "Refused before building"
//! is now "refused before replacing anything": the script built the binary first, so the assertion that cargo did
//! not run is the one that the built binary was not run (`--version`), which the migration check precedes.

use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{Install, run};
use crate::fakes::git::{GitRoute, Repo, Universe};
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::ports::Fakes;

const OPENS: &str = r#"{"opens":true,"refusal":null,"unknown":[],"pending":[]}"#;

const MIGRATIONS: &str = "core/src/store/migrations";

/// What one run of the install left: its exit and its stderr.
struct Run {
    exit: i32,
    stderr: String,
}

/// The install harness's default answer of the built binary's `config --migrate`: nothing stale. A test's `on`
/// rule wins over it, and every other call of the built binary that no rule matched is `NotFound` as before.
struct BuiltConfigMigrate {
    built: PathBuf,
}

impl Handler for BuiltConfigMigrate {
    fn handle(&self, call: &Call) -> Reply {
        if Path::new(&call.program) == self.built && call.args == ["config", "--migrate"] {
            Reply::ok(r#"{"ok":true,"migrated":[]}"#)
        } else {
            Reply::io(std::io::ErrorKind::NotFound)
        }
    }
}

/// A checkout (a clone of an origin whose `main` is `origin/main`), a built binary, a `HOME` and a `CPD_HOME`.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    route: Arc<GitRoute>,
    origin: Repo,
    install: Install,
}

impl Rig {
    /// A checkout whose `origin/main` has the migration files `on_main` (each with its own name as text), the
    /// adapter with a `test/` that an install must not copy, and a built binary that says nothing yet
    /// ([`Rig::built_says`]). The installed `cpd` and `scripts/tag-version.sh` answer every call with exit 0 and no output.
    fn new(on_main: &[&str]) -> Rig {
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
        let installed = home.join(".local/bin/cpd");
        let tag = root.join("repos/install-clone/scripts/tag-version.sh");
        for dir in [&home, &cpd_home, &root.join("repos")] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::create_dir_all(target_dir.join("release")).unwrap();
        let built = target_dir.join("release/cpd");
        fs::write(&built, "built cpd\n").unwrap();
        fs::set_permissions(&built, fs::Permissions::from_mode(0o600)).unwrap();

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
        for name in on_main {
            origin.write(&format!("{MIGRATIONS}/{name}"), &format!("{name}\n"));
        }
        origin.commit("install files");

        Rig::route(&fakes, &route, &built, &installed, &tag);
        let rig = Rig {
            _tmp: tmp,
            fakes,
            route,
            origin,
            install: Install {
                root: root.join("repos/install-clone"),
                target_dir,
                home,
                cpd_home: CpdHome::resolve(Some(OsString::from(cpd_home)), None).unwrap(),
                pid: 4242,
                lock_wait: super::DEFAULT_LOCK_WAIT,
            },
        };
        let clone = rig.git(
            &root,
            &[
                "clone",
                "-q",
                rig.origin.dir().to_str().unwrap(),
                rig.install.root.to_str().unwrap(),
            ],
        );
        assert_eq!(clone.0, 0, "the fixture's clone: {}", clone.1);
        rig
    }

    /// The runner's git route, the installed `cpd` and the tagger answering exit 0 and nothing, and the built
    /// binary's `config --migrate` answering "nothing migrated" unless a test scripts it ([`Rig::built_config`]).
    fn route(fakes: &Fakes, route: &Arc<GitRoute>, built: &Path, installed: &Path, tag: &Path) {
        let (built, installed, tag) = (
            built.to_path_buf(),
            installed.to_path_buf(),
            tag.to_path_buf(),
        );
        (*fakes.runner)
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
            )
            .route("cpd", Arc::new(BuiltConfigMigrate { built }))
            .with_git(route);
    }

    fn built(&self) -> PathBuf {
        self.install.target_dir.join("release/cpd")
    }

    fn installed(&self) -> PathBuf {
        self.install.home.join(".local/bin/cpd")
    }

    fn cpd_home(&self) -> &Path {
        self.install.cpd_home.root()
    }

    /// What `<target>/release/cpd --version` prints.
    fn built_says(&self, version: &str) {
        let built = self.built();
        (*self.fakes.runner).clone().on(
            "cpd",
            move |c| Path::new(&c.program) == built && c.args == ["--version"],
            Reply::ok(version),
        );
    }

    /// What `<target>/release/cpd config --migrate` prints; a test's rule wins over the route's default
    /// (["nothing migrated"]).
    fn built_config(&self, reply: &str) {
        let built = self.built();
        let reply = reply.to_owned();
        (*self.fakes.runner).clone().on(
            "cpd",
            move |c| Path::new(&c.program) == built && c.args == ["config", "--migrate"],
            Reply::ok(&reply),
        );
    }

    /// `git <args>` in `dir`, through the runner: (exit, stderr).
    fn git(&self, dir: &Path, args: &[&str]) -> (i32, String) {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir).args(args);
        let out = self
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

    /// Writes (adds or changes) a migration file of the checkout's working tree, uncommitted.
    fn write_migration(&self, name: &str, text: &str) {
        fs::write(self.install.root.join(MIGRATIONS).join(name), text).unwrap();
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

    /// The calls of the built binary.
    fn built_calls(&self) -> Vec<Call> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| Path::new(&c.program) == self.built())
            .collect()
    }

    /// Something was installed: the binary or the adapter.
    fn installed_anything(&self) -> bool {
        self.installed().exists() || self.install.home.join(".local/share/cpd/pi").exists()
    }

    /// The installed binary and its temporary files are not there.
    fn dest_entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.install.home.join(".local/bin"))
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn version(store: &str) -> String {
    format!(r#"{{"ok":true,"version":"0.0.0","protocol":1,"store":{store}}}"#)
}

fn refusing(code: &str, message: &str) -> String {
    version(&format!(
        r#"{{"opens":false,"refusal":{{"code":"{code}","message":"{message}"}},"unknown":[],"pending":[]}}"#
    ))
}

/// The install refused before it replaced anything: the built binary was not even asked its version.
fn assert_refused_before_building(rig: &Rig, run: &Run) {
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("cpd install refused: "),
        "{}",
        run.stderr
    );
    assert!(
        rig.built_calls().is_empty(),
        "the built binary ran: {:?}",
        rig.built_calls()
    );
    assert!(!rig.installed_anything());
}

#[test]
fn a_build_that_adds_a_migration_origin_main_lacks_is_refused_before_building() {
    let rig = Rig::new(&["0001-initial.sql"]);
    // Untracked, as a new file of a Lead Branch preview is until it is committed.
    rig.write_migration(
        "29991231T2359-branch-only.sql",
        "-- cpd-migration: additive\n",
    );
    rig.built_says(&version(OPENS));

    let run = rig.run();

    assert_refused_before_building(&rig, &run);
    assert!(
        run.stderr.contains(
            "this build carries migration 29991231T2359-branch-only.sql that is not on origin/main"
        ),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains(&format!(
            "apply it to {}/cpd.db before the Workflow merges",
            rig.cpd_home().display()
        )),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains(
            "Merge main into this branch, or wait until the Workflow merges, then install again."
        ),
        "{}",
        run.stderr
    );
}

#[test]
fn a_committed_branch_migration_is_refused_too_and_all_the_names_are_listed() {
    let rig = Rig::new(&["0001-initial.sql"]);
    rig.write_migration("29991231T2358-first.sql", "x\n");
    rig.write_migration("29991231T2359-second.sql", "y\n");
    let root = rig.install.root.clone();
    assert_eq!(rig.git(&root, &["add", "-A"]).0, 0);
    assert_eq!(rig.git(&root, &["commit", "-q", "-m", "migrations"]).0, 0);
    rig.built_says(&version(OPENS));

    let run = rig.run();

    assert_refused_before_building(&rig, &run);
    assert!(
        run.stderr.contains(
            "migration 29991231T2358-first.sql, 29991231T2359-second.sql that is not on origin/main"
        ),
        "{}",
        run.stderr
    );
}

#[test]
fn a_migration_file_changed_from_origin_main_is_refused() {
    let rig = Rig::new(&["0001-initial.sql"]);
    rig.write_migration("0001-initial.sql", "changed\n");
    rig.built_says(&version(OPENS));

    let run = rig.run();

    assert_refused_before_building(&rig, &run);
    assert!(
        run.stderr
            .contains("migration 0001-initial.sql that is not on origin/main"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_migration_that_main_gained_after_the_branch_is_not_the_branch_check() {
    let rig = Rig::new(&["0001-initial.sql"]);
    // origin/main gets a migration the clone's tree lacks.
    rig.origin
        .write(&format!("{MIGRATIONS}/29991231T2359-later.sql"), "later\n");
    rig.origin.commit("later");
    rig.built_says(&version(OPENS));

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(rig.installed_anything());
}

#[test]
fn a_build_whose_migrations_are_all_on_origin_main_installs() {
    let rig = Rig::new(&["0001-initial.sql", "0002-more.sql"]);
    rig.built_says(&version(OPENS));

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(rig.installed().exists());
    // The script's assertion that cargo ran once: the build is the shell's now, and the built binary is asked once for
    // the store verdict, then once for the config migration and validation (both before it replaces the binary).
    let calls = rig.built_calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].args, ["--version"], "{calls:?}");
    assert_eq!(calls[1].args, ["config", "--migrate"], "{calls:?}");
}

#[test]
fn a_failed_fetch_is_a_refusal_because_the_script_cannot_know() {
    let rig = Rig::new(&["0001-initial.sql"]);
    let root = rig.install.root.clone();
    let nowhere = root.parent().unwrap().join("nowhere");
    assert_eq!(
        rig.git(
            &root,
            &["remote", "set-url", "origin", nowhere.to_str().unwrap()]
        )
        .0,
        0
    );
    rig.built_says(&version(OPENS));

    let run = rig.run();

    assert_refused_before_building(&rig, &run);
    assert!(
        run.stderr.contains("cannot fetch origin/main"),
        "{}",
        run.stderr
    );
}

#[test]
fn the_store_verdict_refuses_with_its_message_and_the_fix_of_its_code() {
    for (code, fix) in [
        (
            "store_too_new",
            "merge main into this branch, then install again",
        ),
        (
            "migration_changed",
            "restore it from origin/main or wait for the store to be repaired",
        ),
        (
            "migration_order",
            "rename the branch's breaking migration to a newer name with scripts/new-migration.sh, then install again",
        ),
    ] {
        let rig = Rig::new(&[]);
        rig.built_says(&refusing(code, "the verdict words"));

        let run = rig.run();

        assert_eq!(run.exit, 1, "{code}: {}", run.stderr);
        assert!(
            run.stderr.contains(&format!(
                "this build cannot open {}/cpd.db: the verdict words",
                rig.cpd_home().display()
            )),
            "{code}: {}",
            run.stderr
        );
        assert!(run.stderr.contains(fix), "{code}: {}", run.stderr);
        assert!(!rig.installed_anything(), "{code}");
    }
}

#[test]
fn another_refusal_code_prints_the_message_alone() {
    let rig = Rig::new(&[]);
    rig.built_says(&refusing("store_failed", "not a store"));

    let run = rig.run();

    assert_eq!(run.exit, 1);
    assert!(
        run.stderr.contains("cpd.db: not a store\n"),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("merge main"), "{}", run.stderr);
    assert!(!rig.installed_anything());
}

#[test]
fn a_store_that_does_not_exist_or_opens_installs_and_unknown_additive_migrations_are_no_refusal() {
    for store in [
        "null",
        OPENS,
        r#"{"opens":true,"refusal":null,"unknown":["29991231T2359-later"],"pending":[]}"#,
    ] {
        let rig = Rig::new(&[]);
        rig.built_says(&version(store));

        let run = rig.run();

        assert_eq!(run.exit, 0, "{store}: {}", run.stderr);
        assert!(rig.installed_anything(), "{store}");
    }
}

#[test]
fn a_version_without_a_store_verdict_or_that_fails_is_refused() {
    let rig = Rig::new(&[]);
    // An integer-era build: the legacy numbers only.
    rig.built_says(r#"{"ok":true,"version":"0.0.0"}"#);

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("reports no store verdict"),
        "{}",
        run.stderr
    );
    assert!(!rig.installed_anything());
}

// ---- The binary replace (docs/implementing.md "`cpd install`" step 4) ----

#[test]
fn the_installed_binary_is_a_copy_of_the_built_one_with_mode_755_and_no_temporary_file_is_left() {
    let rig = Rig::new(&[]);
    rig.built_says(&version("null"));

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(fs::read(rig.installed()).unwrap(), b"built cpd\n");
    assert_eq!(
        fs::metadata(rig.installed()).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(rig.dest_entries(), ["cpd"], "no .cpd.tmp.<pid> is left");
    assert!(
        run.stderr
            .contains(&format!("installed {} and ", rig.installed().display())),
        "{}",
        run.stderr
    );
}

#[test]
fn an_existing_installed_binary_is_replaced_by_the_built_one() {
    let rig = Rig::new(&[]);
    fs::create_dir_all(rig.installed().parent().unwrap()).unwrap();
    let mut old = fs::File::create(rig.installed()).unwrap();
    old.write_all(b"old cpd\n").unwrap();
    drop(old);
    rig.built_says(&version("null"));

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(fs::read(rig.installed()).unwrap(), b"built cpd\n");
    assert_eq!(rig.dest_entries(), ["cpd"]);
}

#[test]
fn a_refused_install_leaves_the_installed_binary_as_it_was() {
    for refusal in [Some("store_too_new"), None] {
        let rig = Rig::new(&["0001-initial.sql"]);
        fs::create_dir_all(rig.installed().parent().unwrap()).unwrap();
        fs::write(rig.installed(), "old cpd\n").unwrap();
        match refusal {
            Some(code) => rig.built_says(&refusing(code, "no")),
            None => {
                rig.write_migration("29991231T2359-branch-only.sql", "x\n");
                rig.built_says(&version(OPENS));
            }
        }

        let run = rig.run();

        assert_eq!(run.exit, 1, "{refusal:?}: {}", run.stderr);
        assert_eq!(
            fs::read(rig.installed()).unwrap(),
            b"old cpd\n",
            "{refusal:?}"
        );
        assert_eq!(rig.dest_entries(), ["cpd"], "{refusal:?}");
    }
}

// ---- The config migration and validation (contract §3.7, commands.md §14.4 step 4) ----

/// The install runs the built binary's `config --migrate` with the caller's `CPD_HOME`.
#[test]
fn the_install_runs_the_built_binarys_config_migrate() {
    let rig = Rig::new(&[]);
    rig.built_says(&version("null"));

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let call = rig
        .built_calls()
        .into_iter()
        .find(|c| c.args == ["config", "--migrate"])
        .expect("the built binary ran config --migrate");
    let cpd_home = call
        .envs
        .iter()
        .find(|(key, _)| key == "CPD_HOME")
        .and_then(|(_, value)| value.clone());
    assert_eq!(
        cpd_home.as_deref(),
        rig.cpd_home().to_str(),
        "CPD_HOME is the caller's"
    );
}

/// A config the build cannot migrate is refused before the binary is replaced: the installed
/// binary stays byte-identical.
#[test]
fn a_config_the_build_cannot_migrate_is_refused_before_replacing_the_binary() {
    let rig = Rig::new(&[]);
    fs::create_dir_all(rig.installed().parent().unwrap()).unwrap();
    fs::write(rig.installed(), "old cpd\n").unwrap();
    rig.built_says(&version("null"));
    rig.built_config(
        r#"{"ok":false,"error":{"code":"config_invalid","message":"must be GX","details":{"key":"context_distill_tier"}}}"#,
    );

    let run = rig.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("cpd install refused:"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("cannot migrate"), "{}", run.stderr);
    assert_eq!(
        fs::read(rig.installed()).unwrap(),
        b"old cpd\n",
        "the binary was replaced"
    );
}

/// Each migration the build applied is reported on stderr.
#[test]
fn the_migrations_the_build_applied_are_reported_on_stderr() {
    let rig = Rig::new(&[]);
    rig.built_says(&version("null"));
    rig.built_config(r#"{"ok":true,"path":"x","migrated":["distill-tier-gx"]}"#);

    let run = rig.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("cpd install: config.toml migrated: distill-tier-gx"),
        "{}",
        run.stderr
    );
}
