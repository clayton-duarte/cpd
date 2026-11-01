//! `scripts/pnpm-fresh.sh`, which the quick check and the Gate run first.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common;
use common::{output, write_executable};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/pnpm-fresh.sh")
}

fn fake(bin: &Path, name: &str, calls: &Path, exit: i32) {
    write_executable(
        &bin.join(name),
        &format!(
            "#!/bin/sh\necho {name} \"$@\" >> {}\npwd >> {}\nexit {exit}\n",
            calls.display(),
            calls.display()
        ),
    );
}

/// Runs the script in a temp project; `fakes` are (name, exit) executables on PATH.
/// Returns (output, calls file text, project dir as the shell sees it).
fn run(fakes: &[(&str, i32)], with_bin: bool) -> (Output, String, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let calls = tmp.path().join("calls");
    for (name, exit) in fakes {
        fake(&bin, name, &calls, *exit);
    }
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    let path = if with_bin {
        std::env::join_paths([bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")]).unwrap()
    } else {
        std::env::join_paths([bin]).unwrap()
    };
    let out = output(
        Command::new("/bin/sh")
            .arg(script())
            .current_dir(&project)
            .env_clear()
            .env("PATH", path),
    )
    .unwrap();
    let text = fs::read_to_string(calls).unwrap_or_default();
    (out, text, project)
}

#[test]
fn installs_with_pnpm_install_frozen_lockfile() {
    let (out, calls, project) = run(&[("pnpm", 0)], true);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        calls,
        format!("pnpm install --frozen-lockfile\n{}\n", project.display())
    );
}

#[test]
fn runs_no_npm() {
    let (out, calls, project) = run(&[("npm", 0), ("pnpm", 0)], true);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        calls,
        format!("pnpm install --frozen-lockfile\n{}\n", project.display())
    );
}

#[test]
fn exits_with_the_status_of_pnpm() {
    let (out, _, _) = run(&[("pnpm", 3)], true);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn fails_loud_naming_pnpm_when_it_is_not_on_path() {
    let (out, _, _) = run(&[], false);
    assert_eq!(out.status.code(), Some(127));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr
            .contains("pnpm-fresh: pnpm is not on PATH (package.json pins it with packageManager)"),
        "{stderr}"
    );
}
