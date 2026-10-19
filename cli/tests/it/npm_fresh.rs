//! `scripts/npm-fresh.sh`, which the quick check and the Gate run first.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common;
use common::{output, write_executable};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/npm-fresh.sh")
}

const LOCK: &str = r#"{"packages":{"":{"name":"x"},"node_modules/a":{"version":"1.0.0"}}}"#;

/// Runs the script in a temp project with a fake npm; returns the fake's recorded calls.
fn run(installed: Option<&str>) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let calls = tmp.path().join("calls");
    write_executable(
        &bin.join("npm"),
        &format!("#!/bin/sh\necho npm \"$@\" >> {}\n", calls.display()),
    );
    let project = tmp.path().join("project");
    fs::create_dir_all(project.join("node_modules")).unwrap();
    fs::write(project.join("package-lock.json"), LOCK).unwrap();
    if let Some(hidden) = installed {
        fs::write(project.join("node_modules/.package-lock.json"), hidden).unwrap();
    }
    let node_dir = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .find(|dir| dir.join("node").is_file())
        .expect("node on PATH");
    let path = std::env::join_paths([bin, node_dir, PathBuf::from("/usr/bin"), PathBuf::from("/bin")]).unwrap();
    let out = output(
        Command::new("/bin/sh").arg(script()).current_dir(&project).env_clear().env("PATH", path),
    )
    .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    fs::read_to_string(calls).unwrap_or_default()
}

#[test]
fn installs_when_hidden_lockfile_is_missing() {
    assert_eq!(run(None), "npm ci --no-audit --no-fund\n");
}

#[test]
fn installs_when_packages_differ() {
    let stale = r#"{"packages":{"node_modules/a":{"version":"0.9.0"}}}"#;
    assert_eq!(run(Some(stale)), "npm ci --no-audit --no-fund\n");
}

#[test]
fn installs_when_a_package_is_missing() {
    assert_eq!(run(Some(r#"{"packages":{}}"#)), "npm ci --no-audit --no-fund\n");
}

#[test]
fn does_nothing_when_packages_match() {
    let fresh = r#"{"packages":{"node_modules/a":{"version":"1.0.0"}}}"#;
    assert_eq!(run(Some(fresh)), "");
}
