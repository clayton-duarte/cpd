//! The fake tmux (docs/implementing.md, Tests, "The fake tmux"). Stub: nothing is implemented yet.

pub mod format;
pub mod state;
pub mod target;

pub use state::{Pane, SessionRow, State, WindowRow};

use std::path::PathBuf;

/// One client call: everything after `tmux`, including the global flags.
pub struct Invocation {
    pub argv: Vec<String>,
    pub tmux_tmpdir: PathBuf,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The one entry the bin and the unit tests share.
pub fn run(_inv: &Invocation) -> Outcome {
    Outcome { code: 99, stdout: String::new(), stderr: String::new() }
}

#[cfg(test)]
mod tests;
