//! Configuration and Tiers (contract §8, §9; commands.md §14.2, §14.3):
//! `config.toml`, `cpd config` and `cpd tiers`.
//!
//! Other areas read their keys through [`Config::load`] and the typed fields,
//! and use Tiers through [`Tier`] and [`Catalog`] (from [`load_catalog`]).

mod cache;
pub mod catalog;
mod file;
mod migrate;
pub mod preset;
mod repo;
mod runner;
mod tier;
pub mod tier_write;

#[cfg(test)]
mod migrate_tests;

#[cfg(test)]
mod runner_tests;

#[cfg(test)]
mod tier_range_tests;

#[cfg(test)]
mod tier_tests;

#[cfg(test)]
mod preset_tests;

#[cfg(test)]
mod config_write_tests;

#[cfg(test)]
mod tier_write_tests;

#[cfg(test)]
mod explore_tests;

use serde_json::json;

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::policy::PolicyKey;
use crate::policy::model::Models;

pub use catalog::{PiInstall, load as load_catalog, load_with as load_catalog_with, locate_pi};
pub use file::{
    Config, Events, Inbox, Preset, Source, Sources, TickSeconds, Timeouts, Tmux, Watch,
};
pub use repo::RepoTests;
pub use runner::{TOGGLE_COMMENT, WorkerRunner, set_in_text, toggle};
pub use tier::{Catalog, Letter, Range, Role, RoleTiers, Tier, TierModel};

/// `cpd config` takes no arguments, or the subcommand `tiers set` (§14.2a).
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: Option<ConfigCommand>,
    /// Internal: apply the config migrations and write `config.toml` only when it changed and
    /// is valid. `cpd install` runs it before replacing the binary (contract §3.7).
    #[arg(long)]
    pub migrate: bool,
}

/// The subcommands of `cpd config` (§14.2a).
#[derive(clap::Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum ConfigCommand {
    /// Set the four Tier keys of config.toml (§14.2a)
    #[command(subcommand)]
    Tiers(TiersCommand),
}

/// The subcommands of `cpd config tiers` (§14.2a).
#[derive(clap::Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum TiersCommand {
    /// Set the four Tier keys of config.toml (§14.2a)
    Set(SetTiersArgs),
}

/// `cpd config tiers set` (§14.2a): the four Tiers.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct SetTiersArgs {
    /// `context_distill_tier`
    #[arg(long, value_name = "T")]
    pub auxiliar: Option<String>,
    /// `worker_tier`
    #[arg(long, value_name = "T")]
    pub worker: Option<String>,
    /// `lead_tier`
    #[arg(long, value_name = "T")]
    pub lead: Option<String>,
    /// `manager_tier`
    #[arg(long, value_name = "T")]
    pub manager: Option<String>,
}

/// `cpd tiers` selects a policy key like `cpd policy show`; none selects the machine layer.
#[derive(clap::Args, Debug)]
pub struct TiersArgs {
    #[command(flatten)]
    key: crate::policy::KeyArgs,
    /// A candidate model `<L>=<ref>` for the letter `L`, for this call only (repeatable)
    #[arg(long = "model", value_name = "L=ref")]
    model: Vec<String>,
}

/// `cpd config` (§14.2): the effective configuration, each leaf with its source.
pub fn config(args: ConfigArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    if let Some(ConfigCommand::Tiers(TiersCommand::Set(_))) = args.command {
        todo!("stub: `config tiers set` is not implemented yet")
    }
    if args.migrate {
        return migrate(&home);
    }
    let (config, sources) = Config::load_with_sources(&home)?;
    Ok(Output::success(json!({
        "path": home.config().display().to_string(),
        "config": config,
        "sources": sources,
    })))
}

/// `cpd config --migrate` (commands.md §14.2): applies the migrations to `$CPD_HOME/config.toml`,
/// validates the result as plain `config` does, and writes only when the text changed and is valid, atomically (a temporary file and a rename).
fn migrate(home: &CpdHome) -> Result<Output, CpdError> {
    let path = home.config();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CpdError::new(
                ErrorCode::ConfigMissing,
                "config.toml is missing: it is required, because `machine` has no default.",
            )
            .with("path", path.display().to_string()));
        }
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::ConfigInvalid,
                format!("cannot read config.toml: {e}."),
            )
            .with("path", path.display().to_string()));
        }
    };
    let (migrated, applied) = migrate::apply(&text);
    Config::parse(&migrated, &path)?;
    if migrated != text {
        crate::atomic::write_atomic(&path, &migrated).map_err(|e| {
            CpdError::new(
                ErrorCode::Internal,
                format!("cannot write config.toml: {e}."),
            )
        })?;
    }
    Ok(Output::success(json!({
        "path": path.display().to_string(),
        "migrated": applied,
    })))
}

/// `cpd tiers` (§14.3): the models of the selected key and the efforts pi supports for each.
pub fn tiers(args: TiersArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let (key, resolved) = match args.key.select(&mut env.store)? {
        Some(key) => {
            crate::policy::check_file_names(&env.home)?;
            let resolved = crate::policy::resolve(&env.home, &key)?;
            (Some(key), resolved)
        }
        None => (None, crate::policy::resolve_machine(&env.home)?),
    };
    let catalog = catalog::load_with(&Models::of(&resolved.policy), &mut env.store)?;
    let mut out = json!({

        "pi_version": catalog.pi_version,
        "tiers": {"O": catalog.o, "S": catalog.s, "H": catalog.h, "G": catalog.g},
    });
    if let Some(key) = key {
        let (owner, repo) = match &key {
            PolicyKey::Owner(o) => (o.as_str(), None),
            PolicyKey::Repo(o, n) => (o.as_str(), Some(n.as_str())),
        };
        out["key"] = json!({"owner": owner, "repo": repo});
    }
    Ok(Output::success(out))
}
