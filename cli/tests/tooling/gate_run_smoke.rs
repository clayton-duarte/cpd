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
    assert!(out.stdout.is_empty(), "{out:?}");
}
