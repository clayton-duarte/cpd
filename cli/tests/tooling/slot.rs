//! The test-slot pool is gone (hs-d679, superseding hs-d464 and hs-d497): `cpd slot` is an unknown
//! command, and cpd's own scripts (`scripts/cargo.sh`, `package.json`'s `test`) do not wrap a full
//! run in `cpd slot --`. The removal itself is tested through the built binary and the scripts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::{CPD, TestEnv, output, write_executable};

/// `cpd slot` is an unknown command, not a run in a slot (hs-d679).
#[test]
fn cpd_slot_is_an_unknown_command() {
    let env = TestEnv::new();
    env.write_config("version = 1\nmachine = \"t\"\n");
    let out = output(env.command(CPD).args(["slot", "--", "true"])).unwrap();
    assert_eq!(out.status.code(), Some(2), "cpd slot must be a usage error: {out:?}");
    let err: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["error"]["code"], "usage", "{err}");
    assert!(err["error"]["message"].as_str().unwrap().contains("slot"), "the command is gone: {err}");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A fake `cpd` on `PATH` that records `cpd <args>` in `calls`, says it has `slot` (`slot --help`
/// exits 0, so a script that still wraps takes the branch), and runs `cpd slot -- <cmd>`'s command.
fn fake_cpd(dir: &Path) -> (PathBuf, PathBuf) {
    let (bin, calls) = (dir.join("bin"), dir.join("calls"));
    fs::create_dir_all(&bin).unwrap();
    write_executable(
        &bin.join("cpd"),
        &format!(
            "#!/bin/sh\necho cpd \"$@\" >> {}\n\
             [ \"$1\" = slot ] && [ \"$2\" = --help ] && exit 0\n\
             [ \"$1\" = slot ] && [ \"$2\" = -- ] || exit 99\n\
             shift 2\nexec \"$@\"\n",
            calls.display()
        ),
    );
    (bin, calls)
}

/// Runs `sh <args…>` with only `bin` (and the system's `sh` tools) on `PATH`.
fn run_sh(bin: &Path, args: &[&str], cwd: &Path) -> Output {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(args).current_dir(cwd).env_clear().env("PATH", bin).env("CARGO", bin.join("cargo"));
    output(&mut cmd).unwrap()
}

/// Neither of cpd's own scripts wraps a full run in `cpd slot --`: a fake `cpd` that would run and
/// record the wrapper is never called, and the run reaches cargo (or node) directly (hs-d679).
#[test]
fn the_scripts_do_not_wrap_full_runs_in_cpd_slot() {
    // scripts/cargo.sh: a full `nextest run` reaches cargo directly.
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fake_cpd(tmp.path());
    write_executable(&bin.join("cargo"), &format!("#!/bin/sh\necho cargo \"$@\" >> {}\n", calls.display()));
    let script = repo_root().join("scripts/cargo.sh");
    let out = run_sh(&bin, &[script.to_str().unwrap(), "nextest", "run", "--workspace"], tmp.path());
    assert!(out.status.success(), "{out:?}");
    let ran = fs::read_to_string(&calls).unwrap_or_default();
    assert!(!ran.contains("cpd"), "scripts/cargo.sh wrapped a full run in `cpd slot --`: {ran:?}");
    assert!(ran.contains("cargo nextest run --workspace"), "{ran:?}");

    // package.json's `test`: a full run reaches node directly.
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fake_cpd(tmp.path());
    write_executable(&bin.join("node"), &format!("#!/bin/sh\necho node \"$@\" >> {}\n", calls.display()));
    let package: serde_json::Value = serde_json::from_str(&fs::read_to_string(repo_root().join("package.json")).unwrap()).unwrap();
    let script = package["scripts"]["test"].as_str().unwrap();
    let out = run_sh(&bin, &["-c", script], &repo_root());
    assert!(out.status.success(), "{out:?}");
    let ran = fs::read_to_string(&calls).unwrap_or_default();
    assert!(!ran.contains("cpd"), "package.json's test wrapped a full run in `cpd slot --`: {ran:?}");
    assert!(ran.starts_with("node --test"), "{ran:?}");
}
