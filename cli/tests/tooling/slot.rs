//! `cpd slot` (commands.md §23; contract §7.6 "The test slot"): the built binary, a temporary `$CPD_HOME`;
//! and the scripts of this repo that call it (`scripts/cargo.sh`, `package.json`'s `test`).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::common::{CPD, SLEEP_SECONDS, TestEnv, output, spawn, wait_until, write_executable};

/// A test environment whose `config.toml` has `test_concurrency` = `n`.
fn env(n: u32) -> TestEnv {
    let env = TestEnv::new();
    env.write_config(&format!("version = 1\nmachine = \"t\"\ntest_concurrency = {n}\n"));
    env
}

/// Holds the file `path` as a shell script that runs until `path` exists: `touch <ready>`, then waits for `release`.
fn holder(env: &TestEnv) -> (String, PathBuf, PathBuf) {
    let (ready, release) = (env.root().join("ready"), env.root().join("release"));
    let script = format!(
        "touch {ready}; i=0; while [ ! -e {release} ] && [ $i -lt 1200 ]; do sleep 0.05; i=$((i+1)); done",
        ready = ready.display(),
        release = release.display()
    );
    (script, ready, release)
}

/// `cpd slot -- sh -c <script>` started in the background, its stderr in the file `stderr`.
fn spawn_slot(env: &TestEnv, script: &str, stderr: &Path) -> Child {
    let mut cmd = env.command(CPD);
    cmd.args(["slot", "--", "sh", "-c", script]).stderr(File::create(stderr).unwrap()).stdout(Stdio::null());
    spawn(&mut cmd).unwrap()
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn the_command_runs_in_a_slot_with_its_output_and_its_exit_status() {
    let env = env(2);
    let out = output(env.command(CPD).args(["slot", "--", "sh", "-c", "echo out; echo err >&2; echo slot=$CPD_TEST_SLOT; exit 7"])).unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "out\nslot=0\n", "stdout is the command's alone");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "err\n", "a slot taken at once writes nothing");
}

#[test]
fn a_command_that_cannot_run_or_is_killed_ends_as_the_shell_would() {
    let env = env(2);
    let missing = output(env.command(CPD).args(["slot", "--", "no-such-command-anywhere"])).unwrap();
    assert_eq!(missing.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no-such-command-anywhere: command not found"), "{missing:?}");
    let plain = env.root().join("plain-file");
    fs::write(&plain, "x").unwrap();
    let not_executable = output(env.command(CPD).arg("slot").arg("--").arg(&plain)).unwrap();
    assert_eq!(not_executable.status.code(), Some(126));
    let killed = output(env.command(CPD).args(["slot", "--", "sh", "-c", "kill -9 $$"])).unwrap();
    assert_eq!(killed.status.code(), Some(128 + 9));
}

#[test]
fn its_own_refusals_are_the_envelope_on_stderr() {
    let env = env(2);
    let none = output(env.command(CPD).arg("slot")).unwrap();
    assert_eq!((none.status.code(), none.stdout.as_slice()), (Some(2), &b""[..]));
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&none.stderr).unwrap()["error"]["code"], "usage");

    env.write_config("version = 1\nmachine = \"t\"\ntest_concurrency = 0\n");
    let invalid = output(env.command(CPD).args(["slot", "--", "true"])).unwrap();
    assert_eq!((invalid.status.code(), invalid.stdout.as_slice()), (Some(1), &b""[..]));
    let error = serde_json::from_slice::<serde_json::Value>(&invalid.stderr).unwrap()["error"].clone();
    assert_eq!((error["code"].as_str(), error["details"]["key"].as_str()), (Some("config_invalid"), Some("test_concurrency")));
}

#[test]
fn with_one_slot_a_second_command_waits_until_the_first_ends() {
    let env = env(1);
    let (script, ready, release) = holder(&env);
    let (first_err, second_err) = (env.root().join("first.err"), env.root().join("second.err"));
    let first = spawn_slot(&env, &script, &first_err);
    wait_until("the first command to hold the slot", || ready.exists());

    let ran = env.root().join("second-ran");
    let second = spawn_slot(&env, &format!("touch {}", ran.display()), &second_err);
    let waiting = "slot: waiting for a slot (1 of 1 busy; test_concurrency 1)\n";
    wait_until("the second command to say it waits", || text(&second_err).contains(waiting));
    assert!(!ran.exists(), "the second command ran while the only slot was held");

    fs::write(&release, "").unwrap();
    let (first, second) = (first.wait_with_output().unwrap(), second.wait_with_output().unwrap());

    assert!(first.status.success() && second.status.success());
    assert!(ran.exists());
    let said = text(&second_err);
    assert!(said.starts_with(waiting) && said.contains("slot: slot 0 taken after "), "{said}");
    assert_eq!(text(&first_err), "", "the first took its slot at once: nothing on stderr");
}

#[test]
fn inside_a_held_slot_it_takes_no_second_one() {
    let env = env(1);
    let (script, ready, release) = holder(&env);
    let held_err = env.root().join("held.err");
    let held = spawn_slot(&env, &script, &held_err);
    wait_until("the command to hold the only slot", || ready.exists());

    // The variable says "inside a slot": nothing is taken, nothing is written, and the value is the caller's.
    let out = output(env.command(CPD).env("CPD_TEST_SLOT", "5").args(["slot", "--", "sh", "-c", "echo $CPD_TEST_SLOT"])).unwrap();
    assert_eq!((out.status.code(), String::from_utf8_lossy(&out.stdout).as_ref(), out.stderr.as_slice()), (Some(0), "5\n", &b""[..]));
    // Nested in a slot it took itself, with a single slot, it cannot deadlock either.
    fs::write(&release, "").unwrap();
    held.wait_with_output().unwrap();
    let nested = output(env.command(CPD).args(["slot", "--", CPD, "slot", "--", "sh", "-c", "echo $CPD_TEST_SLOT"])).unwrap();
    assert_eq!((nested.status.code(), String::from_utf8_lossy(&nested.stdout).as_ref(), nested.stderr.as_slice()), (Some(0), "0\n", &b""[..]));
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
