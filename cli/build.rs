//! Emits CPD_VERSION=<Cargo.toml version>-<commit>: the commit is the short hash the build was
//! given in `CPD_COMMIT` (set by scripts/install-cpd.sh, scripts/release.sh and the Gate), `dev`
//! when it was given none. No `git` runs and no `.git` file is watched, so a commit rebuilds
//! nothing (docs/implementing.md "The version string").
//!
//! The logic is [`directives`], a function of its two inputs: `cli/tests/tooling/version.rs` includes this file
//! (`#[path]`) and calls it, so its tests read its result and compile no build script.

/// The lines cargo reads from the build script's stdout, for the commit the build was given (`CPD_COMMIT`, if set)
/// and the package version.
pub fn directives(commit: Option<&str>, version: &str) -> Vec<String> {
    let commit = commit.filter(|c| !c.is_empty()).unwrap_or("dev");
    vec![
        "cargo:rerun-if-env-changed=CPD_COMMIT".to_owned(),
        format!("cargo:rustc-env=CPD_VERSION={version}-{commit}"),
    ]
}

fn main() {
    let commit = std::env::var("CPD_COMMIT").ok();
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    for line in directives(commit.as_deref(), &version) {
        println!("{line}");
    }
}
