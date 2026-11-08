//! `cpd install` (commands.md §14.4, docs/implementing.md "`cpd install`"): the CLI entry of the
//! install. The work is `cpd_core::install::run` over `Ports`; this module only resolves the
//! arguments and the environment the core's `Install` is given, and turns its exit into the one
//! output value or the error envelope. It must not call `std::process::exit`: the `Fx` test kit runs
//! it in process (docs/implementing.md, Tests).

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::install::{self, Install, InstallOptions};
use cpd_core::output::Output;
use cpd_core::ports::Ports;

/// `cpd install --root <checkout> [--no-follow-up]` (commands.md §14.4).
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The checkout to install from: a git worktree with `pi/`, `scripts/` and `core/src/store/migrations/`.
    #[arg(long)]
    pub root: PathBuf,
    /// Write the binary and the adapter, but run no follow-up: a running engine and its views are left as they are.
    #[arg(long)]
    pub no_follow_up: bool,
}

/// Resolves the install from `args` and this process's environment, runs it, writes its human text to
/// stderr, and returns the one output value on success or the envelope whose message is its last line
/// on a non-zero exit.
pub fn run(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let options = InstallOptions {
        no_follow_up: args.no_follow_up,
    };
    let install = resolve(args)?;
    let mut stderr = Vec::new();
    let exit = install::run_with_options(ports, &install, options, &mut stderr);
    let _ = std::io::stderr().write_all(&stderr);
    if exit == 0 {
        return Ok(Output::success(json!({
            "binary": install.home.join(".local/bin/cpd").display().to_string(),
            "adapter": install.home.join(".local/share/cpd/pi").display().to_string(),
            "version": crate::cli::version(),
        })));
    }
    let text = String::from_utf8_lossy(&stderr);
    let message = text
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or("cpd install failed.")
        .to_owned();
    Err(CpdError::new(ErrorCode::InstallFailed, message).with("exit", exit))
}

/// The `Install` of `--root` and the environment, read once at this edge (the environment is not read
/// below it): `target_dir` from `$CARGO_TARGET_DIR`, else `<root>/target`; `home` from `$HOME`;
/// `cpd_home` from `$CPD_HOME`; `lock_wait` from `CPD_INSTALL_LOCK_WAIT_SECS`, else the core's default.
fn resolve(args: Args) -> Result<Install, CpdError> {
    let root = args.root;
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        CpdError::new(
            ErrorCode::Internal,
            "HOME is not set: cannot find where to install cpd.",
        )
    })?;
    let lock_wait = std::env::var_os("CPD_INSTALL_LOCK_WAIT_SECS")
        .and_then(|value| value.to_str().and_then(|s| s.parse::<u64>().ok()))
        .map(Duration::from_secs)
        .unwrap_or(install::DEFAULT_LOCK_WAIT);
    let preview_dir = std::env::var_os("CPD_PREVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache/cpd-preview"));
    Ok(Install {
        root,
        target_dir,
        home,
        cpd_home: CpdHome::from_env()?,
        pid: std::process::id(),
        lock_wait,
        preview_dir,
        macos: cfg!(target_os = "macos"),
    })
}
