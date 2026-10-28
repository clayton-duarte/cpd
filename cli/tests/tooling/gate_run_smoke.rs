//! `scripts/gate-run.sh`, the wrapper that stays shell (docs/implementing.md, "The split: `scripts/gate-run.sh` and
//! `cpd gate finish`"): the one real-binary smoke test of it. The subject is `sh` exporting `GATE_RUN`, running the
//! command and handing over to the built `cpd`: a named bootstrap exception of the shell scripts that stay shell. The
//! leak check itself is tested in process (core/src/gate/tests.rs).
//!
//! Replaces `gate_run::a_clean_run_of_true_prints_nothing_and_exits_0` and
//! `gate_run::a_failing_command_keeps_its_status_and_still_reports_leaks` (the second is now
//! `cpd_core::gate::tests::a_failing_command_keeps_its_status_and_still_reports_leaks`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/gate-run.sh")
}

/// `sh scripts/gate-run.sh <args>` with `TMPDIR` the temp dir and `cpd` the one at `cpd`.
fn gate_run(tmp: &Path, cpd: &Path, args: &[&str]) -> Output {
    Command::new("sh")
        .arg(script())
        .args(args)
        .env("TMPDIR", tmp)
        .env("GATE_RUN_CPD", cpd)
        .env_remove("GATE_RUN")
        .output()
        .expect("run sh")
}

fn built_cpd() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cpd"))
}

fn entries(dir: &Path) -> Vec<String> {
    fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

#[test]
fn a_clean_run_of_true_prints_nothing_and_exits_0() {
    let tmp = tempfile::tempdir().unwrap();

    let out = gate_run(tmp.path(), &built_cpd(), &["true"]);

    assert_eq!(
        (out.status.code(), String::from_utf8_lossy(&out.stdout).as_ref(), String::from_utf8_lossy(&out.stderr).as_ref()),
        (Some(0), "", ""),
        "{out:?}"
    );
    assert_eq!(entries(tmp.path()), Vec::<String>::new(), "a clean run removes its empty directory");
}

#[test]
fn the_command_sees_a_six_character_run_id_and_a_tmpdir_of_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let ids = tmp.path().join("ids");

    let out = gate_run(tmp.path(), &built_cpd(), &["sh", "-c", &format!("echo \"$GATE_RUN $TMPDIR\" > '{}'; exit 3", ids.display())]);

    assert_eq!((out.status.code(), out.stdout.as_slice(), out.stderr.as_slice()), (Some(3), &b""[..], &b""[..]), "{out:?}");
    let seen = fs::read_to_string(&ids).unwrap();
    let (id, dir) = seen.trim().split_once(' ').unwrap();
    assert_eq!(id.len(), 6, "{seen:?}");
    assert_eq!(Path::new(dir), tmp.path().join(format!("gate-run.{id}")));
}

#[test]
fn a_missing_cpd_binary_returns_the_commands_status() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("no-such-cpd");

    let out = gate_run(tmp.path(), &missing, &["sh", "-c", "exit 3"]);

    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    assert_eq!(entries(tmp.path()), Vec::<String>::new(), "no gate-run.* directory is left: nothing else can check it");
}

/// A `cpd` that records its arguments and whether the command was still alive, then hands over to the built one.
fn recording_cpd(dir: &Path, pid_file: &Path, record: &Path) -> PathBuf {
    let stub = dir.join("cpd");
    let body = format!(
        "#!/bin/sh\necho \"$@\" >> '{rec}'\nkill -0 \"$(cat '{pid}')\" 2>/dev/null && echo alive >> '{rec}'\nexec '{real}' \"$@\"\n",
        rec = record.display(),
        pid = pid_file.display(),
        real = built_cpd().display(),
    );
    write_stub(&stub, &body);
    stub
}

fn write_stub(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// Starts the wrapper on a command that leaves a file in its TMPDIR, writes its pid and blocks on stdin, waits for
/// it to say `ready` (a blocking read, no sleep), sends the wrapper `signal` and returns the wrapper's status, the
/// recorded `cpd` calls and the entries left in `tmp`. A wrapper that does not stop fails the test through nextest's
/// own timeout.
fn cancelled_run(signal: nix::sys::signal::Signal) -> (Option<i32>, String, Vec<String>) {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let tmp = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pid_file = work.path().join("pid");
    let record = work.path().join("record");
    let cpd = recording_cpd(work.path(), &pid_file, &record);
    let command = format!("echo $$ > '{}'; : > \"$TMPDIR/leftover\"; echo ready; read line", pid_file.display());

    let mut child = Command::new("sh")
        .arg(script())
        .args(["sh", "-c", &command])
        .env("TMPDIR", tmp.path())
        .env("GATE_RUN_CPD", &cpd)
        .env_remove("GATE_RUN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run sh");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert_eq!(line, "ready\n");

    nix::sys::signal::kill(nix::unistd::Pid::from_raw(child.id() as i32), signal).unwrap();
    let status = child.wait().unwrap();
    drop(child.stdin.take()); // lets a command that was not stopped end: nothing is left behind
    (status.code(), fs::read_to_string(&record).unwrap_or_default(), entries(tmp.path()))
}

#[test]
fn a_term_stops_the_command_runs_the_leak_check_with_143_and_removes_the_run_directory() {
    let (code, record, left) = cancelled_run(nix::sys::signal::Signal::SIGTERM);

    assert_eq!(code, Some(143), "{record:?}");
    assert!(record.starts_with("gate finish --run ") && record.contains(" --status 143\n"), "{record:?}");
    assert!(!record.contains("alive"), "the command was waited for before the check: {record:?}");
    assert_eq!(left, Vec::<String>::new(), "the run's directory is removed");
}

#[test]
fn an_int_stops_the_command_runs_the_leak_check_with_130_and_removes_the_run_directory() {
    let (code, record, left) = cancelled_run(nix::sys::signal::Signal::SIGINT);

    assert_eq!(code, Some(130), "{record:?}");
    assert!(record.starts_with("gate finish --run ") && record.contains(" --status 130\n"), "{record:?}");
    assert!(!record.contains("alive"), "the command was waited for before the check: {record:?}");
    assert_eq!(left, Vec::<String>::new(), "the run's directory is removed");
}

/// The command's first act is a TERM to the wrapper (`eval` and `kill` are builtins: no exec, no wait for `ready`),
/// as a runner cancelling a step in its first milliseconds does. On a wrapper that sets its traps after it starts the
/// command, the TERM can come before them and kill the wrapper (no status, no `gate finish`). Not deterministic on
/// that script: it is a race of the wrapper's own lines, there is no hook between the launch and the traps.
#[test]
fn a_term_sent_by_the_command_as_its_first_act_still_runs_the_leak_check_with_143() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let record = work.path().join("record");
    let cpd = work.path().join("cpd");
    write_stub(&cpd, &format!("#!/bin/sh\necho \"$@\" >> '{rec}'\nexec '{real}' \"$@\"\n", rec = record.display(), real = built_cpd().display()));

    let out = gate_run(tmp.path(), &cpd, &["eval", "kill -s TERM \"$$\""]);

    let record = fs::read_to_string(&record).unwrap_or_default();
    assert_eq!(out.status.code(), Some(143), "{out:?} {record:?}");
    assert!(record.starts_with("gate finish --run ") && record.contains(" --status 143\n"), "{record:?}");
    assert_eq!(entries(tmp.path()), Vec::<String>::new(), "the run's directory is removed");
}
