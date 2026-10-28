//! `scripts/cargo.sh`, which the Gate runs cargo through (R9).

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common;
use common::{output, write_executable};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/cargo.sh")
}

/// A fake cargo at `path` that prints its name and arguments.
fn fake_cargo(path: &Path, name: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_executable(path, &format!("#!/bin/sh\necho {name} \"$@\"\n"));
}

/// A fake cargo at `path` that prints the `RUSTC_WRAPPER` it sees.
fn fake_cargo_printing_wrapper(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_executable(path, "#!/bin/sh\necho RUSTC_WRAPPER=${RUSTC_WRAPPER-unset}\n");
}

/// Runs the script with only the given environment (and `PATH` = `path_dir`).
fn run<V: AsRef<OsStr>>(path_dir: &Path, vars: &[(&str, V)]) -> (i32, String, String) {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(script()).args(["build", "--workspace"]).env_clear().env("PATH", path_dir);
    for (key, value) in vars {
        cmd.env(key, value.as_ref());
    }
    let out = output(&mut cmd).unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// Like [`run`], for runs that must succeed: a fake that could not run (exit
/// non-zero, or anything on stderr) fails here, not as an empty stdout.
fn run_ok<V: AsRef<OsStr>>(path_dir: &Path, vars: &[(&str, V)]) -> String {
    let (exit, stdout, stderr) = run(path_dir, vars);
    assert_eq!((exit, stderr.as_str()), (0, ""), "stdout: {stdout:?}");
    stdout
}

#[test]
fn fails_loudly_without_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let (exit, stdout, stderr) = run(&empty, &[("HOME", tmp.path())]);
    assert_eq!(exit, 127);
    assert_eq!(stdout, "");
    assert!(stderr.contains("cargo not found"), "{stderr}");
    assert!(stderr.contains(&format!("{}/.cargo/bin/cargo", tmp.path().display())), "{stderr}");
}

#[test]
fn a_missing_cargo_variable_fails_loudly_too() {
    let tmp = tempfile::tempdir().unwrap();
    let (exit, _, stderr) = run(tmp.path(), &[("CARGO", &tmp.path().join("nope"))]);
    assert_eq!(exit, 127);
    assert!(stderr.contains("cargo not found"), "{stderr}");
}

#[test]
fn resolves_cargo_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let path_dir = tmp.path().join("path");
    let home = tmp.path().join("home");
    let cargo_home = tmp.path().join("cargo-home");
    let explicit = tmp.path().join("explicit/cargo");
    fake_cargo(&explicit, "explicit");
    fake_cargo(&home.join(".cargo/bin/cargo"), "home");
    fake_cargo(&cargo_home.join("bin/cargo"), "cargo-home");

    // Last resort: $HOME/.cargo/bin/cargo, unless CARGO_HOME is set.
    fs::create_dir_all(&path_dir).unwrap();
    assert_eq!(run_ok(&path_dir, &[("HOME", &home)]), "home build --workspace\n");
    assert_eq!(
        run_ok(&path_dir, &[("HOME", &home), ("CARGO_HOME", &cargo_home)]),
        "cargo-home build --workspace\n"
    );

    // cargo on PATH wins over the fallback.
    fake_cargo(&path_dir.join("cargo"), "path");
    assert_eq!(run_ok(&path_dir, &[("HOME", &home)]), "path build --workspace\n");

    // $CARGO wins over everything.
    assert_eq!(run_ok(&path_dir, &[("HOME", &home), ("CARGO", &explicit)]), "explicit build --workspace\n");
}

/// A PATH dir holding a wrapper-printing fake cargo, and a fake sccache when asked.
fn wrapper_path(tmp: &Path, with_sccache: bool) -> PathBuf {
    let path_dir = tmp.join("path");
    fake_cargo_printing_wrapper(&path_dir.join("cargo"));
    if with_sccache {
        fake_cargo(&path_dir.join("sccache"), "sccache");
    }
    path_dir
}

const NO_VARS: &[(&str, &str)] = &[];

#[test]
fn sccache_on_path_becomes_the_rustc_wrapper() {
    let tmp = tempfile::tempdir().unwrap();
    let path_dir = wrapper_path(tmp.path(), true);
    assert_eq!(run_ok(&path_dir, NO_VARS), "RUSTC_WRAPPER=sccache\n");
}

#[test]
fn without_sccache_the_rustc_wrapper_stays_unset() {
    let tmp = tempfile::tempdir().unwrap();
    let path_dir = wrapper_path(tmp.path(), false);
    assert_eq!(run_ok(&path_dir, NO_VARS), "RUSTC_WRAPPER=unset\n");
}

#[test]
fn a_preset_rustc_wrapper_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let path_dir = wrapper_path(tmp.path(), true);
    assert_eq!(run_ok(&path_dir, &[("RUSTC_WRAPPER", "other")]), "RUSTC_WRAPPER=other\n");
}

#[test]
fn an_empty_rustc_wrapper_opts_out() {
    let tmp = tempfile::tempdir().unwrap();
    let path_dir = wrapper_path(tmp.path(), true);
    assert_eq!(run_ok(&path_dir, &[("RUSTC_WRAPPER", "")]), "RUSTC_WRAPPER=\n");
}

/// A PATH dir with a fake cargo and a fake sccache. Each appends one line to
/// the returned log (`sccache <args>` / `cargo <args>`), in call order, and
/// the fake cargo prints the `RUSTC_WRAPPER` it sees. The fake sccache writes
/// its environment (`env`) to `<log>.env`, and runs `on_call` (a shell snippet,
/// with `$@` the arguments) after logging.
fn logging_path(tmp: &Path, on_call: &str) -> (PathBuf, PathBuf) {
    let path_dir = tmp.join("path");
    let log = tmp.join("calls.log");
    fs::create_dir_all(&path_dir).unwrap();
    write_executable(
        &path_dir.join("cargo"),
        &format!("#!/bin/sh\necho cargo \"$@\" >> {log}\necho RUSTC_WRAPPER=${{RUSTC_WRAPPER-unset}}\n", log = log.display()),
    );
    write_executable(
        &path_dir.join("sccache"),
        &format!(
            "#!/bin/sh\necho sccache \"$@\" >> {log}\n/usr/bin/env > {log}.env\n{on_call}\n",
            log = log.display()
        ),
    );
    (path_dir, log)
}

fn read_or_empty(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// What this Mac's sccache 0.18.0 does on a second `--start-server` with the
/// server already up (measured on a private port): stdout `sccache: Starting
/// the server...`, stderr `sccache: error: Server startup failed: Address in
/// use`, exit status 2.
const RUNNING_SERVER_START: &str = r#"case "$1" in
  --start-server)
    echo "sccache: Starting the server..."
    echo "sccache: error: Server startup failed: Address in use" >&2
    exit 2 ;;
esac"#;

#[test]
fn sccache_on_path_has_its_server_started_once_before_cargo_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = logging_path(tmp.path(), "exit 0");
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stderr.as_str()), (0, ""));
    assert_eq!(stdout, "RUSTC_WRAPPER=sccache\n", "stdout is cargo's alone");
    let calls = read_or_empty(&log);
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.iter().filter(|l| **l == "sccache --start-server").count(), 1, "{calls}");
    assert_eq!(lines.last(), Some(&"cargo build --workspace"), "{calls}");
    assert_eq!(lines.len(), 2, "{calls}");
    assert_eq!(lines[0], "sccache --start-server", "{calls}");
}

#[test]
fn the_sccache_server_starts_in_a_clean_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = logging_path(tmp.path(), "exit 0");
    let home = tmp.path().join("home");
    run(
        &path_dir,
        &[
            ("CARGO_BUILD_JOBS", "3"),
            ("RUSTFLAGS", "-Dwarnings"),
            ("CARGO_TARGET_DIR", "/x"),
            ("NEXTEST_PROFILE", "p"),
            ("FOO", "bar"),
            ("SCCACHE_DIR", "/d"),
            ("HOME", home.to_str().unwrap()),
        ],
    );
    let env = read_or_empty(&PathBuf::from(format!("{}.env", log.display())));
    assert!(!env.is_empty(), "the fake sccache was never run: {}", read_or_empty(&log));
    let names: Vec<&str> = env.lines().filter_map(|l| l.split_once('=')).map(|(k, _)| k).collect();
    for gone in ["CARGO_BUILD_JOBS", "RUSTFLAGS", "CARGO_TARGET_DIR", "NEXTEST_PROFILE", "FOO"] {
        assert!(!names.contains(&gone), "{gone} leaked into the server: {env}");
    }
    assert!(env.lines().any(|l| l == "SCCACHE_DIR=/d"), "{env}");
    assert!(env.lines().any(|l| l == format!("HOME={}", home.display())), "{env}");
    assert!(env.lines().any(|l| l == format!("PATH={}", path_dir.display())), "{env}");
}

/// The `TMPDIR=` line the fake sccache server saw, when it saw one.
fn server_tmpdir(vars: &[(&str, &str)]) -> Option<String> {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = logging_path(tmp.path(), "exit 0");
    let vars: Vec<(&str, String)> = vars.iter().map(|(k, v)| (*k, v.replace("<dir>", tmp.path().to_str().unwrap()))).collect();
    run(&path_dir, &vars);
    let env = read_or_empty(&PathBuf::from(format!("{}.env", log.display())));
    assert!(!env.is_empty(), "the fake sccache was never run: {}", read_or_empty(&log));
    let found = env.lines().find_map(|l| l.strip_prefix("TMPDIR=")).map(|v| v.replace(tmp.path().to_str().unwrap(), "<dir>"));
    found
}

#[test]
fn a_gate_run_tmpdir_is_replaced_by_its_parent_for_the_server() {
    assert_eq!(server_tmpdir(&[("TMPDIR", "<dir>/gate-run.abc123")]).as_deref(), Some("<dir>"));
}

#[test]
fn another_tmpdir_reaches_the_server_unchanged() {
    assert_eq!(server_tmpdir(&[("TMPDIR", "<dir>")]).as_deref(), Some("<dir>"));
}

#[test]
fn an_unset_tmpdir_stays_unset_for_the_server() {
    assert_eq!(server_tmpdir(&[]), None);
}

#[test]
fn a_failed_sccache_start_warns_and_cargo_runs_without_the_wrapper() {
    let tmp = tempfile::tempdir().unwrap();
    let on_call = "case \"$1\" in --start-server) echo 'boom: cannot start' >&2; exit 1 ;; esac";
    let (path_dir, _log) = logging_path(tmp.path(), on_call);
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!(exit, 0, "{stderr}");
    assert_eq!(stdout, "RUSTC_WRAPPER=unset\n", "stderr: {stderr}");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    assert!(lines[0].starts_with("scripts/cargo.sh: warning:"), "{stderr}");
    assert!(lines[0].contains("sccache"), "{stderr}");
}

/// Guard (passes before the fix): a preset or empty `RUSTC_WRAPPER` starts no server.
#[test]
fn a_preset_or_empty_rustc_wrapper_starts_no_server() {
    for wrapper in ["other", ""] {
        let tmp = tempfile::tempdir().unwrap();
        let (path_dir, log) = logging_path(tmp.path(), "exit 0");
        let (exit, stdout, _) = run(&path_dir, &[("RUSTC_WRAPPER", wrapper)]);
        assert_eq!((exit, stdout.as_str()), (0, format!("RUSTC_WRAPPER={wrapper}\n").as_str()));
        let calls = read_or_empty(&log);
        assert!(!calls.contains("sccache"), "RUSTC_WRAPPER={wrapper:?}: {calls}");
    }
}

/// Guard (passes before the fix): a server already running is left alone.
#[test]
fn a_running_sccache_server_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, _log) = logging_path(tmp.path(), RUNNING_SERVER_START);
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stdout.as_str(), stderr.as_str()), (0, "RUSTC_WRAPPER=sccache\n", ""));
}

// ---- the `cargo` resource (step-level slot) and its re-entrancy guard ----

const LIST_DECLARED: &str = r#"{"ok":true,"repo":"o/r","resources":[{"free":3,"holders":[],"name":"cargo","scope":"host","slots":3},{"free":3,"holders":[],"name":"workers","scope":"host","slots":3}]}"#;
const LIST_UNDECLARED: &str = r#"{"ok":true,"repo":"o/r","resources":[{"free":3,"holders":[],"name":"workers","scope":"host","slots":3}]}"#;
const LIST_USAGE_ERROR: &str = r#"{"ok":false,"error":{"code":"usage","message":"cpd resource: /x is not in a git worktree.","details":{}}}"#;

/// Writes a fake cpd (shell builtins only) into `path_dir`. Each call appends
/// `cpd <args>` to `log`; a `resource run` call also appends its arguments, one
/// per line, to `<log>.args`. `resource list` prints `list_reply` and exits
/// `list_status`; `resource run cargo --wait 30 -- <cmd>...` execs the command;
/// anything else exits 99.
fn fake_cpd(path_dir: &Path, log: &Path, list_reply: &str, list_status: i32) {
    fs::create_dir_all(path_dir).unwrap();
    let log = log.display();
    write_executable(
        &path_dir.join("cpd"),
        &format!(
            "#!/bin/sh\n\
             echo cpd \"$@\" >> {log}\n\
             if [ \"$*\" = \"resource list\" ]; then\n  printf '%s\\n' '{list_reply}'\n  exit {list_status}\nfi\n\
             if [ \"$1 $2 $3 $4 $5\" = \"resource run cargo --wait 30\" ] && [ \"$6\" = \"--\" ]; then\n\
             \x20 for a in \"$@\"; do printf '%s\\n' \"$a\" >> {log}.args; done\n  shift 6\n  exec \"$@\"\nfi\n\
             exit 99\n"
        ),
    );
}

/// Writes a fake cargo into `path_dir` that appends `cargo <args>` and
/// `held=<CPD_CARGO_RESOURCE_HELD>` to `log`, prints `cargo-out`, and runs
/// `extra` (a shell snippet) before exiting `status`.
fn fake_cargo_logging(path_dir: &Path, log: &Path, status: i32, extra: &str) {
    fs::create_dir_all(path_dir).unwrap();
    let log = log.display();
    write_executable(
        &path_dir.join("cargo"),
        &format!(
            "#!/bin/sh\necho cargo \"$@\" >> {log}\necho held=${{CPD_CARGO_RESOURCE_HELD-unset}} >> {log}\n{extra}\necho cargo-out\nexit {status}\n"
        ),
    );
}

/// Like [`run`], with the given script arguments.
fn run_args<V: AsRef<OsStr>>(path_dir: &Path, args: &[&str], vars: &[(&str, V)]) -> (i32, String, String) {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(script()).args(args).env_clear().env("PATH", path_dir);
    for (key, value) in vars {
        cmd.env(key, value.as_ref());
    }
    let out = output(&mut cmd).unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// A PATH dir with a fake cargo and a fake cpd sharing one log.
fn cpd_path(tmp: &Path, list_reply: &str, list_status: i32) -> (PathBuf, PathBuf) {
    let path_dir = tmp.join("path");
    let log = tmp.join("calls.log");
    fake_cpd(&path_dir, &log, list_reply, list_status);
    fake_cargo_logging(&path_dir, &log, 0, "");
    (path_dir, log)
}

#[test]
fn a_declared_cargo_resource_wraps_the_run_in_a_resource_run() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_DECLARED, 0);
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stdout.as_str(), stderr.as_str()), (0, "cargo-out\n", ""));
    assert_eq!(
        read_or_empty(&log),
        "cpd resource list\ncpd resource run cargo --wait 30 -- cargo build --workspace\ncargo build --workspace\nheld=1\n"
    );
}

#[test]
fn the_wrapped_run_sees_the_held_guard() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_DECLARED, 0);
    run_ok(&path_dir, NO_VARS);
    assert!(read_or_empty(&log).lines().any(|l| l == "held=1"), "{}", read_or_empty(&log));

    // An empty value counts as not held: the run is wrapped.
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_DECLARED, 0);
    run_ok(&path_dir, &[("CPD_CARGO_RESOURCE_HELD", "")]);
    assert!(read_or_empty(&log).lines().any(|l| l == "held=1"), "{}", read_or_empty(&log));
}

#[test]
fn a_nested_script_call_inside_a_held_run_takes_no_second_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_DECLARED, 0);
    let marker = tmp.path().join("marker");
    let extra = format!(
        "if [ ! -e {m} ]; then\n  : > {m}\n  /bin/sh {s} inner > /dev/null\nfi",
        m = marker.display(),
        s = script().display()
    );
    fake_cargo_logging(&path_dir, &log, 0, &extra);
    let (exit, _, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stderr.as_str()), (0, ""));
    let calls = read_or_empty(&log);
    let lines: Vec<&str> = calls.lines().collect();
    let count = |prefix: &str| lines.iter().filter(|l| l.starts_with(prefix)).count();
    assert_eq!(count("cpd resource run"), 1, "{calls}");
    assert_eq!(count("cpd resource list"), 1, "{calls}");
    assert_eq!(count("cpd "), 2, "{calls}");
    assert!(lines.contains(&"cargo build --workspace"), "{calls}");
    assert!(lines.contains(&"cargo inner"), "{calls}");
}

#[test]
fn the_slot_is_taken_after_the_sccache_server_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = logging_path(tmp.path(), "exit 0");
    fake_cpd(&path_dir, &log, LIST_DECLARED, 0);
    fake_cargo_logging(&path_dir, &log, 0, "");
    run_ok(&path_dir, NO_VARS);
    let calls = read_or_empty(&log);
    let order: Vec<&str> = calls
        .lines()
        .filter(|l| l.starts_with("sccache") || l.starts_with("cpd resource run") || l.starts_with("cargo "))
        .collect();
    assert_eq!(
        order,
        [
            "sccache --start-server",
            "cpd resource run cargo --wait 30 -- cargo build --workspace",
            "cargo build --workspace"
        ],
        "{calls}"
    );
}

#[test]
fn a_wrapped_run_keeps_every_argument_and_the_exit_status() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_DECLARED, 0);
    fake_cargo_logging(&path_dir, &log, 3, "");
    let args = ["nextest", "run", "-E", "test(/^a b/)", "--", "--nocapture"];
    let (exit, _, stderr) = run_args(&path_dir, &args, NO_VARS);
    assert_eq!((exit, stderr.as_str()), (3, ""));
    let logged = read_or_empty(&PathBuf::from(format!("{}.args", log.display())));
    assert_eq!(
        logged,
        "resource\nrun\ncargo\n--wait\n30\n--\ncargo\nnextest\nrun\n-E\ntest(/^a b/)\n--\n--nocapture\n"
    );
}

// Green guards: the fallback runs cargo exactly as before.

#[test]
fn an_undeclared_cargo_resource_runs_cargo_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_UNDECLARED, 0);
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stdout.as_str(), stderr.as_str()), (0, "cargo-out\n", ""));
    let calls = read_or_empty(&log);
    let cpd_calls: Vec<&str> = calls.lines().filter(|l| l.starts_with("cpd ")).collect();
    assert!(cpd_calls.iter().all(|l| *l == "cpd resource list"), "{calls}");
    assert!(calls.contains("cargo build --workspace\nheld=unset\n"), "{calls}");
}

#[test]
fn a_failing_resource_list_runs_cargo_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_USAGE_ERROR, 2);
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stdout.as_str(), stderr.as_str()), (0, "cargo-out\n", ""));
    let calls = read_or_empty(&log);
    assert!(!calls.contains("resource run"), "{calls}");
    assert!(calls.contains("cargo build --workspace"), "{calls}");
}

#[test]
fn a_preset_guard_never_calls_cpd() {
    let tmp = tempfile::tempdir().unwrap();
    let (path_dir, log) = cpd_path(tmp.path(), LIST_DECLARED, 0);
    let (exit, stdout, stderr) = run(&path_dir, &[("CPD_CARGO_RESOURCE_HELD", "1")]);
    assert_eq!((exit, stdout.as_str(), stderr.as_str()), (0, "cargo-out\n", ""));
    assert_eq!(read_or_empty(&log), "cargo build --workspace\nheld=1\n");

}

#[test]
fn without_cpd_on_path_cargo_runs_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let path_dir = tmp.path().join("path");
    let log = tmp.path().join("calls.log");
    fake_cargo_logging(&path_dir, &log, 0, "");
    let (exit, stdout, stderr) = run(&path_dir, NO_VARS);
    assert_eq!((exit, stdout.as_str(), stderr.as_str()), (0, "cargo-out\n", ""));
    assert_eq!(read_or_empty(&log), "cargo build --workspace\nheld=unset\n");
}
