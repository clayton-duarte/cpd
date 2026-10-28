//! The test-slot pool is gone (hs-d679, superseding hs-d464 and hs-d497): `cpd slot` is an unknown
//! command. cpd's scripts never wrap a run in `cpd slot`, and `scripts/cargo.sh` takes a `cargo`
//! resource slot per run (pro-d132). Tested through the built binary and the scripts.

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
    assert!(out.stderr.is_empty(), "cpd prints its one JSON value on stdout: {out:?}");
    let err: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(err["error"]["code"], "usage", "{err}");
    assert!(err["error"]["message"].as_str().unwrap().contains("slot"), "the command is gone: {err}");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A fake `cpd` on `PATH` that records `cpd <args>` in `calls`. cpd's scripts never wrap a run in
/// `cpd slot` (the pool is gone, hs-d679) and scripts/cargo.sh takes a `cargo` resource slot per run
/// (pro-d132): it answers `resource list` with a policy declaring `cargo`, runs `resource run cargo
/// --wait 30 -- <cmd>`'s command, and exits 98 on `slot`, 99 otherwise.
fn fake_cpd(dir: &Path) -> (PathBuf, PathBuf) {
    let (bin, calls) = (dir.join("bin"), dir.join("calls"));
    fs::create_dir_all(&bin).unwrap();
    let list = r#"{"ok":true,"repo":"o/r","resources":[{"free":3,"holders":[],"name":"cargo","scope":"host","slots":3},{"free":3,"holders":[],"name":"workers","scope":"host","slots":3}]}"#;
    write_executable(
        &bin.join("cpd"),
        &format!(
            "#!/bin/sh\necho cpd \"$@\" >> {}\n\
             [ \"$1\" = slot ] && exit 98\n\
             [ \"$1\" = resource ] && [ \"$2\" = list ] && {{ echo '{list}'; exit 0; }}\n\
             [ \"$1\" = resource ] && [ \"$2\" = run ] || exit 99\n\
             shift 6\nexec \"$@\"\n",
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

/// scripts/cargo.sh runs a full `nextest run` in a `cargo` resource slot, never in `cpd slot` (pro-d132).
#[test]
fn cargo_sh_takes_a_cargo_resource_slot_never_a_cpd_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fake_cpd(tmp.path());
    write_executable(&bin.join("cargo"), &format!("#!/bin/sh\necho cargo \"$@\" >> {}\n", calls.display()));
    let script = repo_root().join("scripts/cargo.sh");
    let out = run_sh(&bin, &[script.to_str().unwrap(), "nextest", "run", "--workspace"], tmp.path());
    assert!(out.status.success(), "{out:?}");
    let ran = fs::read_to_string(&calls).unwrap_or_default();
    let wrapped = format!("cpd resource run cargo --wait 30 -- {} nextest run --workspace", bin.join("cargo").display());
    assert!(ran.lines().any(|l| l == wrapped), "{ran:?}");
    assert!(ran.lines().any(|l| l == "cargo nextest run --workspace"), "{ran:?}");
    assert!(!ran.lines().any(|l| l.starts_with("cpd slot")), "{ran:?}");
}

/// package.json's `test` runs node directly: a fake `cpd` is never called.
#[test]
fn package_json_test_runs_node_without_cpd() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, calls) = fake_cpd(tmp.path());
    write_executable(&bin.join("node"), &format!("#!/bin/sh\necho node \"$@\" >> {}\n", calls.display()));
    let package: serde_json::Value = serde_json::from_str(&fs::read_to_string(repo_root().join("package.json")).unwrap()).unwrap();
    let script = package["scripts"]["test"].as_str().unwrap();
    let out = run_sh(&bin, &["-c", script], &repo_root());
    assert!(out.status.success(), "{out:?}");
    let ran = fs::read_to_string(&calls).unwrap_or_default();
    assert!(!ran.contains("cpd"), "package.json's test called cpd: {ran:?}");
    assert!(ran.starts_with("node --test"), "{ran:?}");
}
