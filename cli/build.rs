//! Emits CPD_VERSION=<Cargo.toml version>-<commit>: the commit is the short hash the build was
//! given in `CPD_COMMIT` (set by scripts/install-cpd.sh, scripts/release.sh and the Gate), `dev`
//! when it was given none. No `git` runs and no `.git` file is watched, so a commit rebuilds
//! nothing (docs/implementing.md "The version string").

fn main() {
    println!("cargo:rerun-if-env-changed=CPD_COMMIT");
    let commit = std::env::var("CPD_COMMIT").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| "dev".to_string());
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    println!("cargo:rustc-env=CPD_VERSION={version}-{commit}");
}
