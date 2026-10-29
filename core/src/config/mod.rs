//! Configuration and Tiers (contract §8, §9; commands.md §14.2, §14.3):
//! `config.toml`, `cpd config` and `cpd tiers`.
//!
//! Other areas read their keys through [`Config::load`] and the typed fields,
//! and use Tiers through [`Tier`] and [`Catalog`] (from [`load_catalog`]).

mod cache;
pub mod catalog;
mod file;
mod migrate;
mod repo;
mod runner;
mod tier;

#[cfg(test)]
mod migrate_tests;

#[cfg(test)]
mod runner_tests;

#[cfg(test)]
mod tier_tests;

use serde_json::json;

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::policy::model::Models;
use crate::policy::PolicyKey;

pub use catalog::{PiInstall, load as load_catalog, load_with as load_catalog_with, locate_pi};
pub use file::{Config, Events, Inbox, Source, Sources, TickSeconds, Timeouts, Tmux, Watch};
pub use repo::RepoTests;
pub use runner::WorkerRunner;
pub use tier::{Catalog, Letter, Tier, TierModel};

/// `cpd config` takes no arguments.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigArgs {
    /// Internal: apply the config migrations and write `config.toml` only when it changed and
    /// is valid. `cpd install` runs it before replacing the binary (contract §3.7).
    #[arg(long)]
    pub migrate: bool,
}

/// `cpd tiers` selects a policy key like `cpd policy show`; none selects the machine layer.
#[derive(clap::Args, Debug)]
pub struct TiersArgs {
    #[command(flatten)]
    key: crate::policy::KeyArgs,
}

/// `cpd config` (§14.2): the effective configuration, each leaf with its source.
pub fn config(args: ConfigArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
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
/// validates the result as plain `config` does, and writes only when the text changed and is valid.
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
            return Err(CpdError::new(ErrorCode::ConfigInvalid, format!("cannot read config.toml: {e}."))
                .with("path", path.display().to_string()));
        }
    };
    let (migrated, applied) = migrate::apply(&text);
    Config::parse(&migrated, &path)?;
    if migrated != text {
        std::fs::write(&path, &migrated)
            .map_err(|e| CpdError::new(ErrorCode::Internal, format!("cannot write config.toml: {e}.")))?;
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
