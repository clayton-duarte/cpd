//! `cpd review` (hs-d496): the plannotator sessions a Session has open. A stub: every subcommand refuses with
//! `not_implemented` until Job pnbox-corei writes the commands.

use crate::error::{CpdError, ErrorCode};
use crate::output::Output;

/// The `review` commands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Record an open plannotator session
    Open(OpenArgs),
    /// Close one or every open plannotator session
    Close(CloseArgs),
    /// List the open plannotator sessions of a Session
    List(ListArgs),
}

#[derive(clap::Args, Debug)]
pub struct OpenArgs {
    #[arg(long)]
    pub session: String,
    #[arg(long)]
    pub pn: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub subject: String,
    #[arg(long)]
    pub port: u16,
}

#[derive(clap::Args, Debug)]
pub struct CloseArgs {
    #[arg(long)]
    pub session: String,
    #[arg(long, conflicts_with = "all")]
    pub pn: Option<String>,
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub ended: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    #[arg(long)]
    pub session: String,
}

/// Runs a `review` command: refuses, for now.
pub fn run(_command: Command) -> Result<Output, CpdError> {
    Err(CpdError::new(ErrorCode::Internal, "not_implemented: cpd review is not written yet."))
}
