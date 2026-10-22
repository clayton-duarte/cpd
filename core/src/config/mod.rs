//! Configuration and Tiers (contract §8, §9; commands.md §14.2, §14.3):
//! `config.toml`, `cpd config` and `cpd tiers`.
//!
//! Other areas read their keys through [`Config::load`] and the typed fields,
//! and use Tiers through [`Tier`] and [`Catalog`] (from [`load_catalog`]).

mod cache;
pub mod catalog;
mod file;
mod repo;
mod tier;

use serde_json::json;

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::output::Output;

pub use catalog::{PiInstall, load as load_catalog, load_with as load_catalog_with, locate_pi};
pub use file::{Config, Events, Inbox, Source, Sources, Tiers, TickSeconds, Timeouts, Tmux, Watch};
pub use repo::RepoTests;
pub use tier::{Catalog, Letter, Tier, TierModel};

/// `cpd config` takes no arguments.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigArgs {}

/// `cpd tiers` takes no arguments.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct TiersArgs {}

/// `cpd config` (§14.2): the effective configuration, each leaf with its source.
pub fn config(_args: ConfigArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let (config, sources) = Config::load_with_sources(&home)?;
    Ok(Output::success(json!({
        "path": home.config().display().to_string(),
        "config": config,
        "sources": sources,
    })))
}

/// `cpd tiers` (§14.3): the configured models and the efforts pi supports for each.
pub fn tiers(_args: TiersArgs) -> Result<Output, CpdError> {
    let config = Config::load(&CpdHome::from_env()?)?;
    let catalog = load_catalog(&config)?;
    Ok(Output::success(json!({
        "pi_version": catalog.pi_version,
        "tiers": {"O": catalog.o, "S": catalog.s, "H": catalog.h, "G": catalog.g},
    })))
}
