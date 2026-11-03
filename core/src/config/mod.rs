//! Configuration and Tiers (contract §8, §9; commands.md §14.2, §14.3):
//! `config.toml`, `cpd config` and `cpd tiers`.
//!
//! Other areas read their keys through [`Config::load`] and the typed fields,
//! and use Tiers through [`Tier`] and [`Catalog`] (from [`load_catalog`]).

mod cache;
pub mod catalog;
pub mod experimental;
mod file;
mod migrate;
pub mod preset;
mod repo;
mod runner;
mod tier;
pub mod tier_write;

#[cfg(test)]
mod experimental_tests;

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
pub use experimental::{Experimental, Feature};
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
    pub auxiliar: String,
    /// `worker_tier`
    #[arg(long, value_name = "T")]
    pub worker: String,
    /// `lead_tier`
    #[arg(long, value_name = "T")]
    pub lead: String,
    /// `manager_tier`
    #[arg(long, value_name = "T")]
    pub manager: String,
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
    if let Some(ConfigCommand::Tiers(TiersCommand::Set(set))) = args.command {
        return tiers_set(&home, set);
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
    let mut models = Models::of(&resolved.policy);
    for candidate in &args.model {
        apply_candidate(&mut models, candidate)?;
    }
    let catalog = catalog::load_with(&models, &mut env.store)?;
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

/// One `--model <L>=<ref>` of `cpd tiers` (§14.3): the candidate replaces the model of letter `L` in `models`.
fn apply_candidate(models: &mut Models, candidate: &str) -> Result<(), CpdError> {
    let usage =
        |why: String| CpdError::new(ErrorCode::Usage, format!("--model {candidate:?}: {why}."));
    let (letter, model_ref) = candidate
        .split_once('=')
        .ok_or_else(|| usage("expected <L>=<ref>".into()))?;
    let model_ref = crate::policy::model::model_ref(model_ref).map_err(usage)?;
    let slot = match letter {
        "O" => &mut models.o,
        "S" => &mut models.s,
        "H" => &mut models.h,
        "G" => &mut models.g,
        other => return Err(usage(format!("{other:?} is not one of O, S, H, G"))),
    };
    *slot = model_ref;
    Ok(())
}

/// `cpd config tiers set` (§14.2a): validates the four Tiers, then rewrites only the keys whose value changes.
/// Opens the store for the catalog. Every refusal happens before the write.
fn tiers_set(home: &CpdHome, args: SetTiersArgs) -> Result<Output, CpdError> {
    let path = home.config();
    let text = std::fs::read_to_string(&path).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::ConfigMissing
        } else {
            ErrorCode::ConfigInvalid
        };
        CpdError::new(code, format!("cannot read config.toml: {e}."))
            .with("path", path.display().to_string())
    })?;
    let (current, _) = Config::parse(&text, &path)?;
    let keyed = [
        (
            "context_distill_tier",
            &args.auxiliar,
            current.context_distill_tier,
        ),
        ("worker_tier", &args.worker, current.worker_tier),
        ("lead_tier", &args.lead, current.lead_tier),
        ("manager_tier", &args.manager, current.manager_tier),
    ];
    let mut set = Vec::new();
    for (key, value, held) in keyed {
        let tier = Tier::parse(value).map_err(|e| e.with("key", key))?;
        if tier != held {
            set.push((key, value.as_str()));
        }
    }
    let mut env = Env::open()?;
    let date = {
        let now = env.ports.clock.utc();
        format!(
            "{:04}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day()
        )
    };
    let new = tier_write::set_tiers_in_text(&text, &set, &date);
    let (config, _) = Config::parse(&new, &path)?;

    let models = crate::policy::resolve_machine(home)?.policy.model;
    let catalog = catalog::load_with(&models, &mut env.store)?;
    for (key, _, _) in keyed {
        let tier = match key {
            "context_distill_tier" => config.context_distill_tier,
            "worker_tier" => config.worker_tier,
            "lead_tier" => config.lead_tier,
            _ => config.manager_tier,
        };
        catalog.check(tier).map_err(|e| e.with("key", key))?;
    }
    let roles = config.role_tiers();
    for (role, key) in [
        (Role::Auxiliar, "context_distill_tier"),
        (Role::Worker, "worker_tier"),
        (Role::Lead, "lead_tier"),
        (Role::Manager, "manager_tier"),
    ] {
        if catalog.range(role, &roles).is_none() {
            return Err(CpdError::new(
                ErrorCode::ConfigInvalid,
                format!("{key}: no valid rung of the models lies in the span of this role."),
            )
            .with("key", key));
        }
    }

    if !set.is_empty() {
        crate::atomic::write_atomic(&path, &new).map_err(|e| {
            CpdError::new(
                ErrorCode::Internal,
                format!("cannot write config.toml: {e}."),
            )
        })?;
    }
    Ok(Output::success(json!({
        "changed": set.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        "already": set.is_empty(),
        "config": config,
    })))
}
