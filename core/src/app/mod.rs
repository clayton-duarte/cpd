//! `cpd app` and install's Electron step (commands.md §14.4a and §14.4 step 7a, docs/implementing.md "`cpd app`").
//!
//! Stubs: every function with behaviour returns an `unimplemented` error until the implementation Job. The names are
//! those of docs/implementing.md.

use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::ports::Ports;

#[cfg(test)]
mod tests;

/// What `cpd app` and the install's Electron step run on, built once at the CLI edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// The checkout (`app-root`, or `--root` for the install).
    pub root: PathBuf,
    /// `$HOME`.
    pub home: PathBuf,
    /// `$CPD_HOME`, else `<home>/.cpd`.
    pub cpd_home: PathBuf,
    /// `$CPD_PREVIEW_DIR`, else `<home>/.cache/cpd-preview`.
    pub preview_dir: PathBuf,
    /// Whether ensure runs `plutil` (`cfg!(target_os = "macos")`, set at the edge).
    pub macos: bool,
}

/// What the preview forms run: `preview-web.sh on|reload|off`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    On(Option<PathBuf>),
    Reload(Option<PathBuf>),
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchAction {
    Launched,
    Reloaded,
}

/// The answer of plain `cpd app`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub action: LaunchAction,
    pub pid: u32,
}

/// The Electron of the health file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Electron {
    pub pid: u32,
    pub running: bool,
}

/// The answer of `--status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub output: String,
    pub electron: Option<Electron>,
}

fn unimplemented_error(what: &str) -> CpdError {
    CpdError::new(ErrorCode::Internal, format!("{what} is not implemented"))
}

/// `<home>/.local/share/cpd/app-root`; `app_not_installed` when it is missing.
pub fn read_root(_home: &Path) -> Result<PathBuf, CpdError> {
    Err(unimplemented_error("app::read_root"))
}

/// Writes `app-root` (a temporary file, then a rename).
pub fn write_root(_home: &Path, _root: &Path) -> Result<(), CpdError> {
    Err(unimplemented_error("app::write_root"))
}

/// Builds `web/dist-electron` when missing (or when `rebuild`), then the `chmod` and, on macOS, the `plutil` steps.
pub fn ensure(_ports: &Ports, _app: &App, _rebuild: bool) -> Result<(), CpdError> {
    Err(unimplemented_error("app::ensure"))
}

/// Plain `cpd app`: ensure, then SIGUSR1 to the running Electron, else spawn one.
pub fn launch(_ports: &Ports, _app: &App) -> Result<Launch, CpdError> {
    Err(unimplemented_error("app::launch"))
}

/// `--preview`, `--reload`, `--off`: the text of `scripts/preview-web.sh`.
pub fn preview(_ports: &Ports, _app: &App, _step: Preview) -> Result<String, CpdError> {
    Err(unimplemented_error("app::preview"))
}

/// `--status`.
pub fn status(_ports: &Ports, _app: &App) -> Result<Status, CpdError> {
    Err(unimplemented_error("app::status"))
}
