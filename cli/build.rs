//! Emits CPD_VERSION=<Cargo.toml version>-<short commit hash> (or `-unknown`).
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn main() {
    // git-path resolves correctly in worktrees (refs live in the common dir).
    let mut watch = vec![git(&["rev-parse", "--git-path", "HEAD"]), git(&["rev-parse", "--git-path", "packed-refs"])];
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watch.push(git(&["rev-parse", "--git-path", &reference]));
    }
    for path in watch.into_iter().flatten() {
        println!("cargo:rerun-if-changed={path}");
    }
    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    println!("cargo:rustc-env=CPD_VERSION={version}-{hash}");
}
