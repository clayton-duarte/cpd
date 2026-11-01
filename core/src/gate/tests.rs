//! The tests of `gate::finish`, in process: the old exec tests of `scripts/gate-run.sh` (cli/tests/tooling/gate_run.rs),
//! names and assertions kept. The processes are `FakeSpawn`'s (a `Launch` whose environment carries `GATE_RUN`); the
//! sockets, directories and paths are real files of a temp dir. The report text and the status 64 are pinned.
//!
//! `finish` reads `TMPDIR` (the run's directory) and `TMUX_TMPDIR` from the process environment, which is global:
//! [`Fx`] sets both behind a lock and puts them back.

use std::ffi::OsString;
use std::fs;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::{Finished, finish};
use crate::host::spawn::{Launch, Pid, Signal};
use crate::ports::Fakes;

static LOCK: Mutex<()> = Mutex::new(());

const ID: &str = "aB3dE9";
const OTHER: &str = "zZ9yY8";

struct Fx {
    fakes: Fakes,
    /// Holds the run's directory (`gate-run.<id>`) and the other run's.
    tmp: tempfile::TempDir,
    /// The tmux root, `$TMUX_TMPDIR`. Apart from `tmp` and short: a socket's path must be shorter than `SUN_LEN`
    /// (104 bytes on macOS), and the Gate's `TMPDIR` (`/var/folders/…/T/gate-run.XXXXXX`) already takes 65 of them.
    tmux_root: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl Fx {
    fn new() -> Fx {
        let lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let tmp = tempfile::tempdir().expect("a temp dir");
        let tmux_root = tempfile::Builder::new()
            .prefix("g")
            .tempdir_in("/tmp")
            .expect("a short temp dir");
        let fx_saved = vec![
            ("TMPDIR", std::env::var_os("TMPDIR")),
            ("TMUX_TMPDIR", std::env::var_os("TMUX_TMPDIR")),
        ];
        let fx = Fx {
            fakes: Fakes::new(),
            tmp,
            tmux_root,
            saved: fx_saved,
            _lock: lock,
        };
        fs::create_dir_all(fx.tmux_dir()).unwrap();
        fx.make_run_dir();
        // SAFETY: the lock serializes every `Fx`; nextest gives a test a process of its own.
        unsafe {
            std::env::set_var("TMPDIR", fx.run_dir());
            std::env::set_var("TMUX_TMPDIR", fx.tmux_root.path());
        }
        fx
    }

    fn run_dir(&self) -> PathBuf {
        self.tmp.path().join(format!("gate-run.{ID}"))
    }

    fn make_run_dir(&self) {
        fs::create_dir_all(self.run_dir()).unwrap();
    }

    /// `$TMUX_TMPDIR/tmux-<uid>`: where tmux puts its sockets.
    fn tmux_dir(&self) -> PathBuf {
        // The uid is the temp dir owner's: this user's.
        let uid =
            std::os::unix::fs::MetadataExt::uid(&fs::metadata(self.tmux_root.path()).unwrap());
        self.tmux_root.path().join(format!("tmux-{uid}"))
    }

    fn finish(&self, status: i32) -> Finished {
        finish(&self.fakes.ports, ID, status)
    }

    /// A process of the run `id` that a test left: `zsh -c sleep 120`, a child of pid 1 (`init`), whose environment holds
    /// `GATE_RUN` and `environment`.
    fn left_by_a_test(&self, id: &str, init: &str, environment: &[(&str, &str)]) -> Pid {
        let mut env_set = vec![("GATE_RUN".to_owned(), id.to_owned())];
        env_set.extend(
            environment
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        let launch = Launch {
            program: "zsh".into(),
            args: vec!["-c".into(), "sleep 120".into()],
            env_set,
            ..Launch::default()
        };
        let pid = self.fakes.ports.spawn.spawn(&launch).unwrap();
        self.fakes.spawn.set_cmdline(pid, "zsh -c sleep 120");
        self.fakes.spawn.set_parent(pid, Pid(1));
        self.fakes.spawn.set_cmdline(Pid(1), init);
        pid
    }

    /// A process of the run `id`, as a command of the run would leave it: a tmux server.
    fn process(&self, id: &str) -> Pid {
        let launch = Launch {
            program: "tmux".into(),
            args: vec![
                "-L".into(),
                "cpdt-left".into(),
                "new-session".into(),
                "-d".into(),
            ],
            env_set: vec![("GATE_RUN".into(), id.into())],
            setsid: true,
            ..Launch::default()
        };
        let pid = self.fakes.ports.spawn.spawn(&launch).unwrap();
        self.fakes
            .spawn
            .set_cmdline(pid, "tmux -L cpdt-left new-session -d");
        pid
    }

    /// A process as an `env -i` daemon leaves it: its environment is ONLY `TMPDIR=<tmpdir>`, no `GATE_RUN`.
    fn daemon_with_tmpdir(&self, tmpdir: &Path) -> Pid {
        let launch = Launch {
            program: "sccache".into(),
            args: vec!["--start-server".into()],
            env_set: vec![("TMPDIR".to_owned(), tmpdir.to_string_lossy().into_owned())],
            ..Launch::default()
        };
        let pid = self.fakes.ports.spawn.spawn(&launch).unwrap();
        self.fakes.spawn.set_cmdline(pid, "sccache --start-server");
        self.fakes.spawn.set_parent(pid, Pid(1));
        pid
    }

    /// A socket file named `cpdt-<id>-1-0` in tmux's directory, left by a server that ended (teardown.md).
    fn socket(&self, id: &str) -> PathBuf {
        let path = self.tmux_dir().join(format!("cpdt-{id}-1-0"));
        drop(UnixListener::bind(&path).expect("a socket file"));
        path
    }

    fn tmp_entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.retain(|n| n.starts_with("gate-run."));
        names
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        // SAFETY: as in `new`.
        unsafe {
            for (key, value) in &self.saved {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

#[test]
fn the_hint_for_a_left_process_names_a_command_this_os_has() {
    let fx = Fx::new();
    let pid = fx.process(ID);

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    if Path::new("/proc/self").exists() {
        assert!(
            done.report.contains(&format!("/proc/{}/environ", pid.0)),
            "{done:?}"
        );
    } else {
        assert!(
            done.report.contains(&format!("ps -E -p {}", pid.0)),
            "{done:?}"
        );
        assert!(!done.report.contains("/proc/"), "{done:?}");
    }
}

#[test]
fn a_clean_run_prints_nothing_and_returns_the_commands_status() {
    let fx = Fx::new();

    let done = fx.finish(3);

    assert_eq!(
        done,
        Finished {
            status: 3,
            report: String::new()
        }
    );
    assert_eq!(
        fx.tmp_entries(),
        Vec::<String>::new(),
        "a clean run removes its empty directory"
    );
    assert!(fx.fakes.spawn.signals().is_empty());
}

#[test]
fn a_process_left_with_the_run_id_fails_the_run_and_is_stopped() {
    let fx = Fx::new();
    let pid = fx.process(ID);

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(
        done.report.starts_with(&format!(
            "gate-run: FAIL: this run (GATE_RUN={ID}) left 1 things behind:\n"
        )),
        "{done:?}"
    );
    assert!(
        done.report.contains(&format!("  process {} (ppid ", pid.0)),
        "the process is not named: {done:?}"
    );
    assert!(
        done.report.contains(") tmux -L cpdt-left new-session -d\n"),
        "its command line is not shown: {done:?}"
    );
    assert!(
        done.report.ends_with("gate-run: stopped 1 process(es) and removed 1 path(s) of this run (nothing else was touched)\n"),
        "{done:?}"
    );
    assert_eq!(fx.fakes.spawn.signals(), vec![(pid, Signal::Kill)]);
    assert!(
        !fx.fakes.ports.spawn.alive(pid),
        "process {pid:?} is still running"
    );
}

#[test]
fn a_dir_left_in_the_runs_tmpdir_fails_the_run_and_is_removed() {
    let fx = Fx::new();
    let left = fx.run_dir().join("cpd-test-left");
    fs::create_dir(&left).unwrap();

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(done.report.contains("gate-run: FAIL"), "{done:?}");
    assert!(
        done.report
            .contains(&format!("  path    {}\n", left.display())),
        "the directory {left:?} is not named: {done:?}"
    );
    assert!(!exists(&left), "{left:?} is still there");
    assert_eq!(
        fx.tmp_entries(),
        Vec::<String>::new(),
        "the run's directory is removed"
    );
}

#[test]
fn a_socket_named_with_the_run_id_fails_the_run() {
    let fx = Fx::new();
    let socket = fx.socket(ID);
    assert!(exists(&socket), "the fixture made no socket file");

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(done.report.contains("gate-run: FAIL"), "{done:?}");
    assert!(
        done.report
            .contains(&format!("  socket  {}\n", socket.display())),
        "the socket is not named: {done:?}"
    );
    assert!(!exists(&socket), "the socket file is still there");
}

#[test]
fn another_runs_leftovers_do_not_fail_this_run() {
    let fx = Fx::new();
    // A second gate run, still running its command: a directory of its own, a process and a socket with its id.
    let other_dir = fx
        .tmp
        .path()
        .join(format!("gate-run.{OTHER}"))
        .join("other-left");
    fs::create_dir_all(&other_dir).unwrap();
    let other_process = fx.process(OTHER);
    let other_socket = fx.socket(OTHER);

    let done = fx.finish(0);

    assert_eq!(
        done,
        Finished {
            status: 0,
            report: String::new()
        }
    );
    assert!(
        fx.fakes.spawn.signals().is_empty(),
        "only what carries the run's id is stopped"
    );
    assert!(fx.fakes.ports.spawn.alive(other_process));
    assert!(
        exists(&other_dir) && exists(&other_socket),
        "only what is the run's is removed"
    );
}

#[test]
fn a_failing_command_keeps_its_status_and_still_reports_leaks() {
    let fx = Fx::new();
    fs::create_dir(fx.run_dir().join("cpd-test-left")).unwrap();

    let done = fx.finish(5);

    assert_eq!(done.status, 5, "{done:?}");
    assert!(
        done.report.contains("gate-run: FAIL") && done.report.contains("cpd-test-left"),
        "the leak is not reported: {done:?}"
    );
    assert_eq!(
        fx.tmp_entries(),
        Vec::<String>::new(),
        "the leak is removed"
    );
}

#[test]
fn a_process_of_the_run_that_has_already_exited_is_not_a_leftover() {
    let fx = Fx::new();
    let pid = fx.process(ID);
    fx.fakes.spawn.exit(pid, 0);

    let done = fx.finish(0);

    assert_eq!(
        done,
        Finished {
            status: 0,
            report: String::new()
        }
    );
    assert!(fx.fakes.spawn.signals().is_empty());
}

#[test]
fn the_tripwire_log_lines_fail_the_run_and_are_printed() {
    let fx = Fx::new();
    let line = "tripwire: real tmux called by cpd::x t::y: tmux ls";
    fs::write(fx.run_dir().join("tripwire.log"), format!("{line}\n")).unwrap();

    let done = fx.finish(0);

    assert_ne!(done.status, 0, "{done:?}");
    assert!(
        done.report.contains(&format!(
            "gate-run: FAIL: a test called the real tmux:\n{line}\n"
        )),
        "the line is not printed: {done:?}"
    );
}

#[test]
fn the_tripwire_log_empty_or_absent_leaves_the_run_unchanged() {
    let fx = Fx::new();

    let absent = fx.finish(0);
    fx.make_run_dir();
    fs::write(fx.run_dir().join("tripwire.log"), "").unwrap();
    let empty = fx.finish(0);

    assert_eq!(
        absent,
        Finished {
            status: 0,
            report: String::new()
        }
    );
    assert_eq!(
        empty,
        Finished {
            status: 0,
            report: String::new()
        }
    );
}

// The three tests of pi/test/gate-run.test.ts (the Gate leak check as a shell script, a fake `ps` and a fake /proc),
// which the Rust leak check replaced: the run's processes are `FakeSpawn`'s, and a leak ends the run with status 64.

const TEST_NAME: &str = "tui::tui_merge::the_confirm_closes_the_view_at_once";

#[test]
fn a_leftover_process_is_reported_with_the_nextest_test_name_of_its_environment_and_its_parent_chain_ps_e_macos()
 {
    let fx = Fx::new();
    let pid = fx.left_by_a_test(
        ID,
        "/sbin/launchd",
        &[
            ("NEXTEST_TEST_NAME", TEST_NAME),
            ("NEXTEST_BINARY_ID", "cpd::tui"),
        ],
    );

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(done.report.contains("left 1 things behind"), "{done:?}");
    assert!(
        done.report
            .contains(&format!("  process {} (ppid 1) zsh -c sleep 120\n", pid.0)),
        "{done:?}"
    );
    assert!(
        done.report
            .contains(&format!("    NEXTEST_TEST_NAME={TEST_NAME}\n")),
        "{done:?}"
    );
    assert!(
        done.report.contains("    parents: 1 /sbin/launchd\n"),
        "{done:?}"
    );
}

#[test]
fn a_leftover_process_is_reported_with_the_nextest_test_name_of_its_environment_and_its_parent_chain_proc_linux()
 {
    let fx = Fx::new();
    let pid = fx.left_by_a_test(
        ID,
        "/sbin/init",
        &[
            ("NEXTEST_TEST_NAME", TEST_NAME),
            ("NEXTEST_BINARY_ID", "cpd::tui"),
        ],
    );

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(done.report.contains("left 1 things behind"), "{done:?}");
    assert!(
        done.report
            .contains(&format!("  process {} (ppid 1) zsh -c sleep 120\n", pid.0)),
        "{done:?}"
    );
    assert!(
        done.report
            .contains(&format!("    NEXTEST_TEST_NAME={TEST_NAME}\n")),
        "{done:?}"
    );
    assert!(
        done.report.contains("\n    parents: 1 /sbin/init\n"),
        "{done:?}"
    );
}

#[test]
fn a_leftover_process_whose_environment_has_no_nextest_test_name_says_so_instead_of_naming_a_test()
{
    let fx = Fx::new();
    fx.left_by_a_test(ID, "/sbin/init", &[]);

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(
        done.report
            .contains("NEXTEST_TEST_NAME=(not in its environment"),
        "{done:?}"
    );
}

#[test]
fn a_process_with_the_runs_tmpdir_and_no_gate_run_fails_the_run_and_is_stopped() {
    let fx = Fx::new();
    let pid = fx.daemon_with_tmpdir(&fx.run_dir());

    let done = fx.finish(0);

    assert_eq!(done.status, 64, "{done:?}");
    assert!(
        done.report.starts_with(&format!(
            "gate-run: FAIL: this run (GATE_RUN={ID}) left 1 things behind:\n"
        )),
        "{done:?}"
    );
    assert!(
        done.report.contains(&format!(
            "  process {} (ppid 1) sccache --start-server\n",
            pid.0
        )),
        "{done:?}"
    );
    assert!(done.report.ends_with("gate-run: stopped 1 process(es) and removed 1 path(s) of this run (nothing else was touched)\n"), "{done:?}");
    assert_eq!(fx.fakes.spawn.signals(), vec![(pid, Signal::Kill)]);
    assert!(
        !fx.fakes.ports.spawn.alive(pid),
        "process {pid:?} is still running"
    );
}

#[test]
fn a_process_with_both_gate_run_and_the_runs_tmpdir_is_one_leftover_killed_once() {
    let fx = Fx::new();
    let pid = fx.left_by_a_test(ID, "init", &[("TMPDIR", &fx.run_dir().to_string_lossy())]);

    let done = fx.finish(0);

    assert!(done.report.contains("left 1 things behind"), "{done:?}");
    assert_eq!(
        fx.fakes.spawn.signals(),
        vec![(pid, Signal::Kill)],
        "{done:?}"
    );
}

#[test]
fn a_process_whose_tmpdir_is_the_parent_of_the_run_dir_is_not_a_leftover() {
    let fx = Fx::new();
    let pid = fx.daemon_with_tmpdir(fx.tmp.path());

    let done = fx.finish(0);

    assert_eq!(
        done,
        Finished {
            status: 0,
            report: String::new()
        }
    );
    assert!(fx.fakes.spawn.signals().is_empty());
    assert!(
        fx.fakes.ports.spawn.alive(pid),
        "process {pid:?} was stopped"
    );
}

#[test]
fn a_process_with_another_runs_tmpdir_is_not_a_leftover() {
    let fx = Fx::new();
    let pid = fx.daemon_with_tmpdir(&fx.tmp.path().join(format!("gate-run.{OTHER}")));

    let done = fx.finish(0);

    assert_eq!(
        done,
        Finished {
            status: 0,
            report: String::new()
        }
    );
    assert!(fx.fakes.spawn.signals().is_empty());
    assert!(
        fx.fakes.ports.spawn.alive(pid),
        "process {pid:?} was stopped"
    );
}

#[test]
fn a_process_with_the_runs_tmpdir_that_has_already_exited_is_not_a_leftover() {
    let fx = Fx::new();
    let pid = fx.daemon_with_tmpdir(&fx.run_dir());
    fx.fakes.spawn.exit(pid, 0);

    let done = fx.finish(0);

    assert_eq!(
        done,
        Finished {
            status: 0,
            report: String::new()
        }
    );
    assert!(fx.fakes.spawn.signals().is_empty());
}
