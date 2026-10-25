//! `cpd install` (commands.md §14.4, docs/implementing.md "`cpd install`"): the install, in Rust over `Ports`.
//!
//! The steps, in order: the preflight, the migration check, the verdict of the built binary, the binary, the
//! adapter swap, the follow-ups, the tag. Only the migration check, the verdict and the binary exist so far as
//! tests (`migration_tests`); `run` is a stub until the Job that implements it.

use std::io::Write;
use std::path::PathBuf;

use crate::home::CpdHome;
use crate::ports::Ports;

/// What `cpd install --root <checkout>` is given, read once at the edge (the environment is not read below it).
pub struct Install {
    /// `--root`: the checkout, a git worktree with `pi/`, `scripts/` and `core/src/store/migrations/`.
    pub root: PathBuf,
    /// `$CARGO_TARGET_DIR`, else `<root>/target`: the built binary is `<target_dir>/release/cpd`.
    pub target_dir: PathBuf,
    /// `$HOME`: the destination is `<home>/.local/bin/cpd`, the adapter `<home>/.local/share/cpd/pi`.
    pub home: PathBuf,
    /// `$CPD_HOME`: its `cpd.db` is the store the verdict is about.
    pub cpd_home: CpdHome,
    /// The pid of the installing process: it names the temporary files (`.cpd.tmp.<pid>`).
    pub pid: u32,
}

/// Runs the install; the human text (refusals, warnings, `installed ...`) goes to `err`. The exit code: 0, or 1
/// for a refusal, a failure or a follow-up that failed.
pub fn run(_ports: &Ports, _install: &Install, _err: &mut dyn Write) -> i32 {
    todo!("cpd install: the migration check, the verdict, the binary replace")
}

#[cfg(test)]
mod migration_tests;
