//! `scripts/cargo.sh`, which the Gate runs cargo through (R9).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/cargo.sh")
}

/// A fake cargo at `path` that prints its name and arguments.
fn fake_cargo(path: &Path, name: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\necho {name} \"$@\"\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Runs the script with only the given environment (and `PATH` = `path_dir`).
fn run(path_dir: &Path, vars: &[(&str, &Path)]) -> (i32, String, String) {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(script()).args(["build", "--workspace"]).env_clear().env("PATH", path_dir);
    for (key, value) in vars {
        cmd.env(key, value);
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
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
    assert_eq!(run(&path_dir, &[("HOME", &home)]).1, "home build --workspace\n");
    assert_eq!(
        run(&path_dir, &[("HOME", &home), ("CARGO_HOME", &cargo_home)]).1,
        "cargo-home build --workspace\n"
    );

    // cargo on PATH wins over the fallback.
    fake_cargo(&path_dir.join("cargo"), "path");
    assert_eq!(run(&path_dir, &[("HOME", &home)]).1, "path build --workspace\n");

    // $CARGO wins over everything.
    assert_eq!(run(&path_dir, &[("HOME", &home), ("CARGO", &explicit)]).1, "explicit build --workspace\n");
}
