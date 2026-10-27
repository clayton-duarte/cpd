//! `cpd resource list` and `cpd resource run` (commands.md §29).

use serde_json::json;

use crate::error::CpdError;
use crate::output::Output;
use crate::ports::Ports;

/// The `resource` commands (commands.md §29).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// The repo's declared resources and who holds them (§29.1)
    List,
    /// Take a resource and run a command holding it (§29.2)
    Run(RunArgs),
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// A resource the repo's policy declares
    pub name: String,
    /// Try again every second for this many minutes [default: 0]
    #[arg(long, value_name = "minutes", value_parser = clap::value_parser!(u64).range(1..))]
    pub wait: Option<u64>,
    /// The command and its arguments, after `--`
    #[arg(last = true, required = true, value_name = "command")]
    pub command: Vec<String>,
}

pub fn run(_ports: &Ports, command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List => Ok(Output::success(json!({"repo": "", "resources": []}))),
        Command::Run(_) => Ok(Output::success(json!({}))),
    }
}
