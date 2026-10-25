//! `cpd slot` (commands.md §23; contract §7.6 "The test slot"): `cpd_core::slot::run_with` in the test process, on a
//! temporary `$CPD_HOME` ([`Fx`]), a virtual clock and a `start` that records the command it is given and says how it
//! ended: no process is started. The real `flock` of a lock file in the temp home stays real (it is ours, not external
//! software). Two tests keep the built binary, because their subject is the OS: a SIGKILL that frees a `flock` and a
//! SIGTERM forwarded to a child. And the scripts of this repo that call `cpd slot` (`scripts/cargo.sh`, `package.json`'s
//! `test`), the named exception of the bootstrap scripts.

use std::ffi::OsString;
use std::fs::{self, File};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::runner::Runner;
use cpd_core::ports::Ports;
use cpd_core::slot::{Args, Ended, Slots, run_with};

use crate::common::fx::Fx;
use crate::common::{CPD, SLEEP_SECONDS, TestEnv, output, spawn, wait_until, write_executable};
#[path = "../store/scripted.rs"]
mod scripted;
use scripted::ScriptedClock;

/// A test environment whose `config.toml` has `test_concurrency` = `n`.
fn env(n: u32) -> TestEnv {
    let env = TestEnv::new();
    env.write_config(&format!("version = 1\nmachine = \"t\"\ntest_concurrency = {n}\n"));
    env
}

/// `cpd slot -- sh -c <script>` started in the background (the built binary), its stderr in the file `stderr`.
fn spawn_slot(env: &TestEnv, script: &str, stderr: &Path) -> Child {
    let mut cmd = env.command(CPD);
    cmd.args(["slot", "--", "sh", "-c", script]).stderr(File::create(stderr).unwrap()).stdout(Stdio::null());
    spawn(&mut cmd).unwrap()
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// `Fx` whose `config.toml` has `test_concurrency` = `n`.
fn fx(n: u32) -> Fx {
    let fx = Fx::new();
    fx.env.write_config(&format!("version = 1\nmachine = \"t\"\ntest_concurrency = {n}\n"));
    fx
}

fn args(command: &[&str]) -> Args {
    Args { command: command.iter().map(OsString::from).collect() }
}

/// The `Ports` of `fx` with `clock` for its time.
fn on(fx: &Fx, clock: ScriptedClock<'static>) -> Ports {
    let runner: Arc<dyn Runner> = fx.fakes.runner.clone();
    Ports { clock: Arc::new(clock), runner, spawn: fx.fakes.spawn.clone() }
}

/// What a command was given: its program, arguments and the variables it sets (`None` for one it removes).
#[derive(Debug, PartialEq)]
struct Given {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, Option<OsString>)>,
}

impl Given {
    fn of(command: &Command) -> Given {
        Given {
            program: command.get_program().to_owned(),
            args: command.get_args().map(ToOwned::to_owned).collect(),
            envs: command.get_envs().map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned))).collect(),
        }
    }

    /// The value this command sets `CPD_TEST_SLOT` to, if it sets it.
    fn slot(&self) -> Option<String> {
        self.envs.iter().find(|(k, _)| k == "CPD_TEST_SLOT").and_then(|(_, v)| v.as_ref()).map(|v| v.to_string_lossy().into_owned())
    }
}

/// `run_with` on `ports`, whose command ends as `ended` says; returns what it came to, what it said on stderr and the
/// commands it was given.
fn run_in(ports: &Ports, command: &[&str], mut ended: impl FnMut(Command) -> Ended) -> (Result<u8, CpdError>, String, Vec<Given>) {
    let (mut said, mut given) = (Vec::new(), Vec::new());
    let result = run_with(ports, args(command), &mut said, &mut |c| {
        given.push(Given::of(&c));
        ended(c)
    });
    (result, String::from_utf8(said).expect("stderr is UTF-8"), given)
}

fn exited(code: i32) -> Ended {
    Ended::Status(ExitStatus::from_raw(code << 8))
}

/// A clock for a run that must never wait: the nap that would be the first panics, instead of looping on virtual time.
fn never_waits() -> ScriptedClock<'static> {
    ScriptedClock::new().at(Duration::ZERO, || panic!("the run waited for a slot nobody frees"))
}

#[test]
fn the_command_runs_in_a_slot_with_its_output_and_its_exit_status() {
    let fx = fx(2);
    let (result, said, given) = run_in(&on(&fx, never_waits()), &["sh", "-c", "echo out; echo err >&2; echo slot=$CPD_TEST_SLOT; exit 7"], |_| exited(7));
    assert_eq!(result.unwrap(), 7);
    // The command's own stdout is the command's alone: `run_with` has no writer for it, and the command is not given one.
    assert_eq!(given.len(), 1);
    assert_eq!((given[0].program.to_str(), given[0].slot().as_deref()), (Some("sh"), Some("0")), "{given:?}");
    assert_eq!(given[0].args, ["-c", "echo out; echo err >&2; echo slot=$CPD_TEST_SLOT; exit 7"]);
    assert_eq!(said, "", "a slot taken at once writes nothing");
}

#[test]
fn a_command_that_cannot_run_or_is_killed_ends_as_the_shell_would() {
    let fx = fx(2);
    let ports = on(&fx, never_waits());
    // The OS's refusals to start: `spawn` of the command it was given fails (nothing runs) as `start_and_wait` finds it.
    let cannot_start = |c: Command| {
        let mut c = c;
        match c.spawn() {
            Ok(_) => panic!("{:?} started", Given::of(&c)),
            Err(e) => Ended::NotStarted(e),
        }
    };
    let (missing, said, _) = run_in(&ports, &["no-such-command-anywhere"], cannot_start);
    assert_eq!(missing.unwrap(), 127);
    assert!(said.contains("no-such-command-anywhere: command not found"), "{said}");
    let plain = fx.env.root().join("plain-file");
    fs::write(&plain, "x").unwrap();
    let (not_executable, _, _) = run_in(&ports, &[plain.to_str().unwrap()], cannot_start);
    assert_eq!(not_executable.unwrap(), 126);
    // Killed by signal 9: the status of a process the signal ended is the signal's number, not an exit code.
    let (killed, _, _) = run_in(&ports, &["sh", "-c", "kill -9 $$"], |_| Ended::Status(ExitStatus::from_raw(9)));
    assert_eq!(killed.unwrap(), 128 + 9);
}

#[test]
fn its_own_refusals_are_the_envelope_on_stderr() {
    let fx = fx(2);
    let ports = on(&fx, never_waits());
    let (none, said, given) = run_in(&ports, &[], |_| panic!("no command to run"));
    let none = none.unwrap_err();
    // `cpd` prints `to_line(err)` on stderr and exits `err.exit()`, writing nothing on stdout.
    assert_eq!((none.exit().code(), said.as_str(), given.len()), (2, "", 0));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&cpd_core::output::to_line(&none)).unwrap()["error"]["code"], "usage");

    fx.env.write_config("version = 1\nmachine = \"t\"\ntest_concurrency = 0\n");
    let (invalid, _, given) = run_in(&ports, &["true"], |_| panic!("no slot, no command"));
    let invalid = invalid.unwrap_err();
    assert_eq!((invalid.exit().code(), given.len()), (1, 0));
    let error = serde_json::from_str::<serde_json::Value>(&cpd_core::output::to_line(&invalid)).unwrap()["error"].clone();
    assert_eq!((error["code"].as_str(), error["details"]["key"].as_str()), (Some("config_invalid"), Some("test_concurrency")));
}

#[test]
fn with_one_slot_a_second_command_waits_until_the_first_ends() {
    let fx = fx(1);
    let (holding, held) = channel();
    let (release, released) = channel::<()>();
    // The first command, on a thread of its own: it takes the only slot and holds it until the test lets it end.
    let first_ports = fx.fakes.ports.clone();
    let first = std::thread::spawn(move || {
        let mut said = Vec::new();
        let result = run_with(&first_ports, args(&["sh", "-c", "hold"]), &mut said, &mut |_| {
            holding.send(()).unwrap();
            released.recv().expect("the test releases the first command");
            exited(0)
        });
        (result, String::from_utf8(said).unwrap())
    });
    held.recv().expect("the first command holds the slot");

    // The second command's first nap is the poll that finds the slot busy; its time-1000-ms step lets the first end (and
    // waits for the slot to be free) before the next try.
    let first = Mutex::new(Some(first));
    let first_ended = Arc::new(Mutex::new(None));
    let clock = {
        let first_ended = Arc::clone(&first_ended);
        ScriptedClock::new().at(Duration::from_millis(1), move || {
            release.send(()).unwrap();
            *first_ended.lock().unwrap() = Some(first.lock().unwrap().take().unwrap().join().unwrap());
        })
    };
    let ports = on(&fx, clock);
    let (result, said, given) = run_in(&ports, &["touch", "second-ran"], |_| exited(0));

    assert_eq!(result.unwrap(), 0);
    assert_eq!(given.len(), 1, "the second command ran once the first had ended");
    let waiting = "slot: waiting for a slot (1 of 1 busy; test_concurrency 1)\n";
    assert!(said.starts_with(waiting) && said.contains("slot: slot 0 taken after "), "{said}");
    let (first_result, first_said) = first_ended.lock().unwrap().take().expect("the first command ended while the second waited");
    assert_eq!(first_result.unwrap(), 0);
    assert_eq!(first_said, "", "the first took its slot at once: nothing on stderr");
}

#[test]
fn inside_a_held_slot_it_takes_no_second_one() {
    let fx = fx(1);
    let ports = on(&fx, never_waits());
    let home = cpd_core::home::CpdHome::from_env().unwrap();
    let held = Slots::new(&home, 1).take_with(&ports, "test", &mut |_, _| Ok(())).unwrap();
    assert_eq!(held.index(), 0, "the only slot is held");

    // The variable says "inside a slot": nothing is taken, nothing is written, and the value is the caller's.
    let (result, said, given) = fx.with_vars(&[("CPD_TEST_SLOT", "5")], || run_in(&ports, &["sh", "-c", "echo $CPD_TEST_SLOT"], |_| exited(0)));
    assert_eq!((result.unwrap(), said.as_str()), (0, ""));
    assert_eq!(given.len(), 1);
    assert_eq!(given[0].slot(), None, "the command is given no CPD_TEST_SLOT of its own: it inherits the caller's 5");
    drop(held);

    // Nested in a slot it took itself, with a single slot, it cannot deadlock either: the outer command's environment
    // (`CPD_TEST_SLOT=0`) is the one the inner `cpd slot` it starts finds.
    let (outer, outer_said, outer_given) = run_in(&ports, &["cpd", "slot", "--", "sh", "-c", "echo $CPD_TEST_SLOT"], |c| {
        let slot = Given::of(&c).slot().expect("the outer command is in slot 0");
        let (inner, said, inner_given) =
            fx.with_vars(&[("CPD_TEST_SLOT", &slot)], || run_in(&ports, &["sh", "-c", "echo $CPD_TEST_SLOT"], |_| exited(0)));
        assert_eq!((inner.unwrap(), said.as_str(), slot.as_str()), (0, "", "0"));
        assert_eq!(inner_given[0].slot(), None, "the inner one takes no slot");
        exited(0)
    });
    assert_eq!((outer.unwrap(), outer_said.as_str(), outer_given[0].slot().as_deref()), (0, "", Some("0")));
}

#[test]
fn a_killed_holder_frees_its_slot_even_though_its_command_lives_on() {
    let env = env(1);
    let (pid_file, ready) = (env.root().join("command.pid"), env.root().join("ready"));
    let held_err = env.root().join("held.err");
    let script = format!("echo $$ > {pid}; touch {ready}; exec sleep {SLEEP_SECONDS}", pid = pid_file.display(), ready = ready.display());
    let mut holder = spawn_slot(&env, &script, &held_err);
    wait_until("the command to hold the slot", || ready.exists());

    // SIGKILL: no clean-up runs, and the command (a child that keeps running) must not keep the slot.
    holder.kill().unwrap();
    holder.wait().unwrap();
    let out = output(env.command(CPD).args(["slot", "--", "echo", "free"])).unwrap();
    let command: i32 = text(&pid_file).trim().parse().unwrap();
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(command), nix::sys::signal::Signal::SIGKILL).unwrap();

    assert_eq!((out.status.code(), String::from_utf8_lossy(&out.stdout).as_ref(), out.stderr.as_slice()), (Some(0), "free\n", &b""[..]));
}

#[test]
fn a_signal_goes_to_the_command_and_the_slot_stays_held_until_it_ends() {
    let env = env(1);
    let (ready, got) = (env.root().join("ready"), env.root().join("got"));
    let held_err = env.root().join("held.err");
    let script = format!(
        "trap 'echo term > {got}; exit 3' TERM; touch {ready}; while :; do sleep 0.05; done",
        got = got.display(),
        ready = ready.display()
    );
    let holder = spawn_slot(&env, &script, &held_err);
    wait_until("the command to hold the slot", || ready.exists());

    nix::sys::signal::kill(nix::unistd::Pid::from_raw(holder.id() as i32), nix::sys::signal::Signal::SIGTERM).unwrap();
    let ended = holder.wait_with_output().unwrap();

    assert_eq!(ended.status.code(), Some(3), "the command's own status, so slot was not killed by the signal");
    assert_eq!(text(&got), "term\n");
}

// ------------------------------------------------------------ the scripts of this repo

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The `cpd` a fake `PATH` holds.
#[derive(Clone, Copy, PartialEq)]
enum FakeCpd {
    /// No `cpd` on `PATH`.
    Absent,
    /// A `cpd` with `slot`: `slot --help` exits 0, `slot -- <cmd>` records the call and runs `<cmd>`.
    WithSlot,
    /// A `cpd` installed before `slot` existed: every `slot …` records the call, `slot --help` exits 2, and `slot -- <cmd>` prints
    /// the usage envelope and exits 0 without running `<cmd>` (the silent skip the probe prevents).
    WithoutSlot,
}

/// A directory of fakes for `scripts/cargo.sh`: a `cargo` that records `cargo <args>` in `calls`, and the `cpd` of `cpd`.
/// The probe `cpd slot --help` is recorded by none of them.
fn fakes(dir: &Path, cpd: FakeCpd) -> (PathBuf, PathBuf) {
    let (bin, calls) = (dir.join("bin"), dir.join("calls"));
    fs::create_dir_all(&bin).unwrap();
    write_executable(&bin.join("cargo"), &format!("#!/bin/sh\necho cargo \"$@\" >> {}\n", calls.display()));
    let probe = |status: u8| format!("[ \"$1\" = slot ] && [ \"$2\" = --help ] && exit {status}\n");
    match cpd {
        FakeCpd::Absent => {}
        FakeCpd::WithSlot => write_executable(
            &bin.join("cpd"),
            &format!("#!/bin/sh\n{}echo cpd \"$@\" >> {}\n[ \"$1\" = slot ] && [ \"$2\" = -- ] || exit 99\nshift 2\nexec \"$@\"\n", probe(0), calls.display()),
        ),
        FakeCpd::WithoutSlot => write_executable(
            &bin.join("cpd"),
            &format!(
                "#!/bin/sh\n{}echo cpd \"$@\" >> {}\necho '{{\"ok\":false,\"error\":{{\"code\":\"usage\"}}}}'\nexit 0\n",
                probe(2),
                calls.display()
            ),
        ),
    }
    (bin, calls)
}

/// Runs `sh <args…>` with only `bin` and the system's `sh` tools on `PATH`.
fn run_sh(bin: &Path, args: &[&str], cwd: &Path) -> std::process::Output {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(args).current_dir(cwd).env_clear().env("PATH", bin).env("CARGO", bin.join("cargo"));
    output(&mut cmd).unwrap()
}

#[test]
fn cargo_script_runs_test_and_nextest_through_the_slot_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fakes(tmp.path(), FakeCpd::WithSlot);
    let script = repo_root().join("scripts/cargo.sh");
    let script = script.to_str().unwrap();
    for args in [["nextest", "run", "--workspace"], ["test", "--workspace", "-q"], ["build", "--workspace", "-q"], ["check", "-q", "-p"]] {
        let out = run_sh(&bin, &[&[script][..], &args].concat(), tmp.path());
        assert!(out.status.success(), "{args:?}: {out:?}");
    }
    let cargo = bin.join("cargo");
    let cargo = cargo.display();
    assert_eq!(
        text(&calls),
        format!(
            "cpd slot -- {cargo} nextest run --workspace\ncargo nextest run --workspace\n\
             cpd slot -- {cargo} test --workspace -q\ncargo test --workspace -q\n\
             cargo build --workspace -q\ncargo check -q -p\n"
        )
    );
}

#[test]
fn cargo_script_runs_without_cpd_on_the_path() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fakes(tmp.path(), FakeCpd::Absent);
    let script = repo_root().join("scripts/cargo.sh");
    let out = run_sh(&bin, &[script.to_str().unwrap(), "nextest", "run"], tmp.path());
    assert!(out.status.success(), "{out:?}");
    assert_eq!(text(&calls), "cargo nextest run\n");
}

#[test]
fn cargo_script_runs_the_command_directly_when_cpd_has_no_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fakes(tmp.path(), FakeCpd::WithoutSlot);
    write_executable(&bin.join("cargo"), &format!("#!/bin/sh\necho cargo \"$@\" >> {}\nexit 5\n", calls.display()));
    let script = repo_root().join("scripts/cargo.sh");
    let out = run_sh(&bin, &[script.to_str().unwrap(), "nextest", "run"], tmp.path());
    assert_eq!(out.status.code(), Some(5), "the command's own status: {out:?}");
    assert_eq!(text(&calls), "cargo nextest run\n");
}

/// `package.json`'s `test` script, run by `sh -c` as npm does (npm appends the arguments of `npm test -- <args>` to the script's
/// command line), with a fake `node` that records its arguments.
fn npm_test_with(cpd: FakeCpd, args: &[&str]) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fakes(tmp.path(), cpd);
    write_executable(&bin.join("node"), &format!("#!/bin/sh\necho node \"$@\" >> {}\n", calls.display()));
    let package: serde_json::Value = serde_json::from_str(&fs::read_to_string(repo_root().join("package.json")).unwrap()).unwrap();
    let script = [package["scripts"]["test"].as_str().unwrap()].into_iter().chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
    let out = run_sh(&bin, &["-c", &script], &repo_root());
    assert!(out.status.success(), "{out:?}");
    text(&calls)
}

fn npm_test(cpd: FakeCpd) -> String {
    npm_test_with(cpd, &[])
}

#[test]
fn npm_test_goes_through_the_slot_and_runs_without_cpd() {
    let files = "--test pi/test/";
    let through = npm_test(FakeCpd::WithSlot);
    assert!(through.starts_with("cpd slot -- node --test pi/test/") && through.contains(&format!("\nnode {files}")), "{through}");
    let direct = npm_test(FakeCpd::Absent);
    assert!(direct.starts_with("node --test pi/test/") && !direct.contains("cpd"), "{direct}");
}

#[test]
fn npm_test_runs_directly_when_cpd_has_no_slot() {
    let direct = npm_test(FakeCpd::WithoutSlot);
    assert!(direct.starts_with("node --test pi/test/") && !direct.contains("cpd"), "{direct}");
}

/// Only a full run takes a slot (contract §7.6 "What is full", hs-d464): the `test` script with no file or name argument, or only
/// options; with a file or a name it runs directly.
#[test]
fn the_npm_test_script_takes_a_slot_for_a_full_run_only() {
    for (args, slot) in [(&[][..], true), (&["--test-reporter=spec"], true), (&["pi/test/x.test.ts"], false), (&["--test-reporter=spec", "pi/test/x.test.ts"], false)] {
        let calls = npm_test_with(FakeCpd::WithSlot, args);
        assert_eq!(calls.contains("cpd slot -- "), slot, "the test script with {args:?}: {calls}");
        assert!(args.iter().all(|a| calls.contains(a)), "the test script with {args:?} passes its arguments on: {calls}");
    }
}

/// What `scripts/cargo.sh <args>` ran, with a fake `cpd` that has `slot`: whether it went through `cpd slot --`. Both ways it
/// ran cargo itself with `<args>`.
fn cargo_script_took_a_slot(args: &[&str]) -> Result<bool, String> {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fakes(tmp.path(), FakeCpd::WithSlot);
    let script = repo_root().join("scripts/cargo.sh");
    let out = run_sh(&bin, &[&[script.to_str().unwrap()][..], args].concat(), tmp.path());
    assert!(out.status.success(), "{args:?}: {out:?}");
    let (cargo, words) = (bin.join("cargo"), args.join(" "));
    let (through, direct) = (format!("cpd slot -- {} {words}\ncargo {words}\n", cargo.display()), format!("cargo {words}\n"));
    match text(&calls) {
        t if t == through => Ok(true),
        t if t == direct => Ok(false),
        t => Err(format!("{args:?}: neither through the slot nor direct: {t:?}")),
    }
}

/// The shapes of contract §7.6 "What is full" (hs-d464): which `scripts/cargo.sh` runs take a test slot.
#[test]
fn cargo_script_takes_a_slot_for_a_full_run_only() {
    let shapes: &[(&[&str], bool)] = &[
        // Full: no test filter, whatever else the run has.
        (&["nextest", "run", "--workspace"], true),
        (&["nextest", "run"], true),
        (&["nextest", "run", "-p", "cpd"], true),
        (&["nextest", "run", "--package", "cpd", "--no-fail-fast"], true),
        (&["test", "--workspace"], true),
        (&["test", "-p", "cpd", "--test", "plan_overlap"], true),
        (&["test", "--lib", "--features", "x", "-j", "4"], true),
        (&["nextest", "run", "--profile", "ci", "--all-targets"], true),
        (&["test", "--", "--skip", "slow"], true),
        // Filtered to named tests: takes none, `--workspace` and `-p` included.
        (&["nextest", "run", "--workspace", "-E", "test(/x/)"], false),
        (&["nextest", "run", "-E", "test(/x/)"], false),
        (&["nextest", "run", "--filterset", "test(x)"], false),
        (&["nextest", "run", "-p", "cpd", "-E", "test(x)"], false),
        (&["nextest", "run", "some_test_name"], false),
        (&["nextest", "run", "foo", "bar"], false),
        (&["nextest", "run", "-p", "cpd", "some_test_name"], false),
        (&["nextest", "run", "--", "foo"], false),
        (&["test", "some_name"], false),
        (&["test", "-p", "cpd", "some_name"], false),
        (&["test", "--workspace", "foo"], false),
        (&["test", "--", "foo"], false),
        (&["test", "--", "--exact", "foo"], false),
        // No test is run.
        (&["test", "--no-run"], false),
        (&["test", "--workspace", "--no-run"], false),
        (&["nextest", "run", "--no-run"], false),
        (&["nextest", "list"], false),
        (&["nextest", "archive", "--archive-file", "x.tar.zst"], false),
        (&["build", "--workspace"], false),
        (&["check", "-p", "cpd"], false),
    ];
    let wrong: Vec<String> = shapes
        .iter()
        .filter_map(|(args, slot)| match cargo_script_took_a_slot(args) {
            Ok(took) if took == *slot => None,
            Ok(took) => Some(format!("{args:?}: took a slot {took}, the contract says {slot}")),
            Err(e) => Some(e),
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
