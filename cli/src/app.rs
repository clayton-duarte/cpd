//! `cpd app` (commands.md §14.4a): the CLI entry. The work is `cpd_core::app` over `Ports`. Stub: the handler returns an
//! `unimplemented` error until the implementation Job.

use std::path::PathBuf;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;
use cpd_core::ports::Ports;

/// Launch the Electron app of the installed checkout, or run its web preview (commands.md §14.4a)
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
#[command(group(clap::ArgGroup::new("form").args(["preview", "reload", "off", "status"]).multiple(false)))]
#[command(group(clap::ArgGroup::new("takes_worktree").args(["preview", "reload"])))]
pub struct Args {
    /// Start the web preview of `<worktree>` (`scripts/preview-web.sh on`)
    #[arg(long)]
    pub preview: bool,
    /// Rebuild and reload the web preview (`scripts/preview-web.sh reload`)
    #[arg(long)]
    pub reload: bool,
    /// Stop the web preview (`scripts/preview-web.sh off`)
    #[arg(long)]
    pub off: bool,
    /// Report the web preview and the Electron app (`scripts/preview-web.sh status`)
    #[arg(long)]
    pub status: bool,
    /// The worktree of `--preview` or `--reload`
    #[arg(requires = "takes_worktree", conflicts_with_all = ["off", "status"])]
    pub worktree: Option<PathBuf>,
}

pub fn run(_ports: &Ports, _args: Args) -> Result<Output, CpdError> {
    Err(CpdError::new(
        ErrorCode::Internal,
        "cpd app is not implemented",
    ))
}
