//! pi's model catalog (contract §9): which efforts each configured Tier model
//! supports, asked of pi itself and never kept in a table here.
//!
//! cpd 2 has no Node of its own, so a small embedded ES module (`catalog.mjs`)
//! runs with pi's Node and prints pi's thinking levels per model as JSON.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use super::cache;
use super::file::Config;
use super::tier::{Catalog, Letter, TierModel, effort_of_level};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::store::Store;

/// The embedded module; its header says how it is run.
const MODULE: &str = include_str!("catalog.mjs");

/// A pi install located as contract §9 says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiInstall {
    /// `<agentDir>`: two levels above the realpath of `pi`.
    pub agent_dir: PathBuf,
    /// The first line of `<agentDir>/install/current-version`.
    pub version: String,
    /// `…/node_modules/@earendil-works/pi-coding-agent/dist/index.js`
    pub coding_agent_entry: PathBuf,
    /// `…/node_modules/@earendil-works/pi-ai/dist/index.js`
    pub pi_ai_entry: PathBuf,
}

fn pi_not_found(message: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::PiNotFound, message)
}

/// Realpath of `pi` on `PATH`, then `<agentDir>/install/current-version`, then
/// the release's two entry points. `pi_not_found` when any step fails.
pub fn locate_pi() -> Result<PiInstall, CpdError> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let pi = std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("pi"))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .ok_or_else(|| pi_not_found("`pi` is not on PATH.").with("path", path.to_string_lossy().into_owned()))?;
    let real = std::fs::canonicalize(&pi)
        .map_err(|e| pi_not_found(format!("cannot resolve {}: {e}.", pi.display())).with("pi", pi.display().to_string()))?;
    let agent_dir = real
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| pi_not_found(format!("{} has no agent directory above it.", real.display())))?
        .to_path_buf();

    let current = agent_dir.join("install").join("current-version");
    let text = std::fs::read_to_string(&current).map_err(|e| {
        pi_not_found(format!("cannot read {}: {e}.", current.display())).with("current_version", current.display().to_string())
    })?;
    let version = text.lines().next().unwrap_or("").trim().to_owned();
    // The characters pi's launcher accepts for `current-version`.
    let valid = !version.is_empty()
        && version != "."
        && version != ".."
        && version.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    if !valid {
        return Err(pi_not_found(format!("invalid pi version {version:?} in {}.", current.display()))
            .with("current_version", current.display().to_string())
            .with("version", version));
    }

    let scope = agent_dir.join("install").join("releases").join(&version).join("node_modules").join("@earendil-works");
    let coding_agent_entry = scope.join("pi-coding-agent").join("dist").join("index.js");
    let pi_ai_entry = scope.join("pi-ai").join("dist").join("index.js");
    for entry in [&coding_agent_entry, &pi_ai_entry] {
        if !entry.is_file() {
            return Err(pi_not_found(format!("pi release {version} is incomplete: {} is missing.", entry.display()))
                .with("missing", entry.display().to_string()));
        }
    }
    Ok(PiInstall { agent_dir, version, coding_agent_entry, pi_ai_entry })
}

/// pi's Node, the one pi's launcher uses: `${XDG_DATA_HOME:-~/.local/share}/pi-node/current/bin/node`
/// when it exists, else `node` on `PATH`.
pub(crate) fn node() -> PathBuf {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".local").join("share")));
    data_home
        .map(|d| d.join("pi-node").join("current").join("bin").join("node"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("node"))
}

fn catalog_failed(message: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::CatalogFailed, message)
}

/// The last 60 lines of `bytes`, as the other tool-failure errors carry.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(60)..].join("\n")
}

/// The Tier catalog for the configured models, from the installed pi.
///
/// Errors: `pi_not_found`, `catalog_failed` (the module did not run or did not
/// print valid JSON), `tier_model_unknown` (a configured model is not in pi's
/// catalog).
///
/// This is the one place the catalog is obtained. It is cached in `tier_catalog`
/// per pi version (commands.md §14.3): read when the cache holds the configured
/// models under the installed pi's version; otherwise computed from pi and
/// written in one transaction, replacing what was cached.
pub fn load(config: &Config) -> Result<Catalog, CpdError> {
    let mut store = Store::open(&CpdHome::from_env()?, config.timeouts.store_busy_ms.into())?;
    load_with(config, &mut store)
}

/// [`load`] through a `store` the caller already holds, so the catalog does not
/// open a second one (flock, pragmas, schema hash).
pub fn load_with(config: &Config, store: &mut Store) -> Result<Catalog, CpdError> {
    load_from(&locate_pi()?, config, store)
}

fn load_from(pi: &PiInstall, config: &Config, store: &mut Store) -> Result<Catalog, CpdError> {
    let models = [config.tiers.o.as_str(), config.tiers.s.as_str(), config.tiers.h.as_str()];
    if let Some(cached) = cache::read(store, &pi.version, models)? {
        return Ok(cached);
    }
    let catalog = read(pi, config)?;
    cache::write(store, &catalog)?;
    Ok(catalog)
}

/// Runs the embedded module with pi's Node and builds the [`Catalog`] from what it prints.
pub(crate) fn read(pi: &PiInstall, config: &Config) -> Result<Catalog, CpdError> {
    let tiers = [(Letter::O, &config.tiers.o), (Letter::S, &config.tiers.s), (Letter::H, &config.tiers.h)];
    let node = node();
    let mut cmd = Command::new(&node);
    cmd.args(["--input-type=module", "-e", MODULE, "--"])
        .arg(&pi.coding_agent_entry)
        .arg(&pi.pi_ai_entry)
        .args(tiers.iter().map(|(letter, model)| format!("{letter}={model}")))
        .env("PI_MANAGED_INSTALL_ROOT", pi.agent_dir.join("install"))
        .current_dir(&pi.agent_dir)
        .stdin(Stdio::null());
    let out = cmd.output().map_err(|e| {
        catalog_failed(format!("cannot run {}: {e}.", node.display())).with("node", node.display().to_string())
    })?;
    if !out.status.success() {
        return Err(catalog_failed(format!("the catalog module failed under pi {}.", pi.version))
            .with("node", node.display().to_string())
            .with("exit", out.status.code().map_or(Value::Null, Value::from))
            .with("stderr", tail(&out.stderr)));
    }
    let levels: BTreeMap<String, Option<Vec<String>>> = serde_json::from_slice(&out.stdout).map_err(|e| {
        catalog_failed(format!("the catalog module printed invalid JSON: {e}."))
            .with("stdout", tail(&out.stdout))
            .with("stderr", tail(&out.stderr))
    })?;

    let mut missing = Vec::new();
    let mut models = Vec::new();
    for (letter, model) in tiers {
        let Some(entry) = levels.get(&letter.to_string()) else {
            return Err(catalog_failed(format!("the catalog module printed nothing for Tier {letter}.")).with("letter", letter.to_string()));
        };
        let Some(entry) = entry else {
            missing.push(serde_json::json!({"letter": letter.to_string(), "model": model}));
            continue;
        };
        let mut efforts = Vec::new();
        for level in entry {
            match effort_of_level(level) {
                Some(Some(effort)) => efforts.push(effort),
                Some(None) => {}
                None => {
                    return Err(catalog_failed(format!("pi reports the unknown thinking level {level:?} for {model}."))
                        .with("model", model.clone())
                        .with("level", level.clone()));
                }
            }
        }
        efforts.sort_unstable();
        efforts.dedup();
        if efforts.is_empty() {
            return Err(catalog_failed(format!("pi reports no usable thinking level for {model}.")).with("model", model.clone()));
        }
        models.push(TierModel { model: model.clone(), efforts });
    }
    if !missing.is_empty() {
        return Err(CpdError::new(ErrorCode::TierModelUnknown, "A configured model is not in pi's catalog.")
            .with("missing", missing)
            .with("pi_version", pi.version.clone()));
    }
    let [o, s, h]: [TierModel; 3] = models.try_into().expect("one model per letter");
    Ok(Catalog { pi_version: pi.version.clone(), o, s, h })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pi() -> PiInstall {
        PiInstall {
            agent_dir: PathBuf::from("/nonexistent/agent"),
            version: "9.9.9".into(),
            coding_agent_entry: PathBuf::from("/nonexistent/coding-agent.js"),
            pi_ai_entry: PathBuf::from("/nonexistent/pi-ai.js"),
        }
    }

    /// commands-F3: the catalog is read through the Store the caller passes, not
    /// through a second one opened from `$CPD_HOME` (the test sets none).
    #[test]
    fn load_from_uses_the_given_store() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let (config, _) = Config::parse("version = 1\nmachine = \"hs\"\n", &home.config()).unwrap();
        let tier = |model: &str| TierModel { model: model.to_owned(), efforts: vec![0, 1, 2] };
        let cached = Catalog {
            pi_version: "9.9.9".into(),
            o: tier(&config.tiers.o),
            s: tier(&config.tiers.s),
            h: tier(&config.tiers.h),
        };
        cache::write(&mut store, &cached).unwrap();

        // pi's entry points do not exist: a cache miss would fail in `read`.
        assert_eq!(load_from(&pi(), &config, &mut store).unwrap(), cached);
    }
}
