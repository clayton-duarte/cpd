//! `cpd swr` (commands.md §32): read the cache and the outbox, and invalidate a key. Stub until its Job.

use crate::error::CpdError;
use crate::output::Output;
use crate::ports::Ports;

/// The `swr` commands (commands.md §32).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// The cached queries (§32.1)
    List(ListArgs),
    /// One query and its active mutations (§32.2)
    Show(KeyArgs),
    /// Ask the engine to fetch a query now (§32.3)
    Invalidate(KeyArgs),
    /// The health of every endpoint (§32.4)
    Endpoints,
    /// The mutation outbox (§32.5, §32.6)
    #[command(subcommand)]
    Mutation(MutationCommand),
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only the queries of this datasource
    #[arg(long, value_name = "system")]
    pub system: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct KeyArgs {
    /// A query key, `<system>:<kind>:<id>`
    pub key: String,
}

/// The `swr mutation` commands.
#[derive(clap::Subcommand, Debug)]
pub enum MutationCommand {
    /// One mutation (§32.5)
    Show(IdArgs),
    /// The mutations, filtered by status (§32.6)
    List(StatusArgs),
}

#[derive(clap::Args, Debug)]
pub struct IdArgs {
    /// A mutation id, a positive integer
    pub id: String,
}

#[derive(clap::Args, Debug)]
pub struct StatusArgs {
    /// Only the mutations in this status
    #[arg(long, value_name = "status")]
    pub status: Option<String>,
}

pub fn run(_ports: &Ports, _command: Command) -> Result<Output, CpdError> {
    todo!("cpd swr")
}
