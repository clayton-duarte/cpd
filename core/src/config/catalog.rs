//! pi's model catalog (contract §9): which efforts each configured Tier model
//! supports, asked of pi itself and never kept in a table here.
//!
//! cpd 2 has no Node of its own, so a small embedded ES module (`catalog.mjs`)
//! runs with pi's Node and prints pi's thinking levels per model as JSON.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::cache;
use super::file::Config;
use super::tier::{Catalog, Letter, TierModel, effort_of_level};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::Runner;
use crate::policy::model::Models;
use crate::ports::Ports;
use crate::store::Store;

/// The embedded module; its header says how it is run.
const MODULE: &str = include_str!("catalog.mjs");

/// The module's deadline (contract §9): it loads pi's packages and reads no
/// network (`allowModelNetwork: false`); a cold catalog measured 0.68 s (median, commands-F3).
const NODE_DEADLINE: Duration = Duration::from_secs(30);

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
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .ok_or_else(|| {
            pi_not_found("`pi` is not on PATH.").with("path", path.to_string_lossy().into_owned())
        })?;
    let real = std::fs::canonicalize(&pi).map_err(|e| {
        pi_not_found(format!("cannot resolve {}: {e}.", pi.display()))
            .with("pi", pi.display().to_string())
    })?;
    let agent_dir = real
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            pi_not_found(format!(
                "{} has no agent directory above it.",
                real.display()
            ))
        })?
        .to_path_buf();

    let current = agent_dir.join("install").join("current-version");
    let text = std::fs::read_to_string(&current).map_err(|e| {
        pi_not_found(format!("cannot read {}: {e}.", current.display()))
            .with("current_version", current.display().to_string())
    })?;
    let version = text.lines().next().unwrap_or("").trim().to_owned();
    // The characters pi's launcher accepts for `current-version`.
    let valid = !version.is_empty()
        && version != "."
        && version != ".."
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    if !valid {
        return Err(pi_not_found(format!(
            "invalid pi version {version:?} in {}.",
            current.display()
        ))
        .with("current_version", current.display().to_string())
        .with("version", version));
    }

    let scope = agent_dir
        .join("install")
        .join("releases")
        .join(&version)
        .join("node_modules")
        .join("@earendil-works");
    let coding_agent_entry = scope.join("pi-coding-agent").join("dist").join("index.js");
    let pi_ai_entry = scope.join("pi-ai").join("dist").join("index.js");
    for entry in [&coding_agent_entry, &pi_ai_entry] {
        if !entry.is_file() {
            return Err(pi_not_found(format!(
                "pi release {version} is incomplete: {} is missing.",
                entry.display()
            ))
            .with("missing", entry.display().to_string()));
        }
    }
    Ok(PiInstall {
        agent_dir,
        version,
        coding_agent_entry,
        pi_ai_entry,
    })
}

/// pi's Node, the one pi's launcher uses: `${XDG_DATA_HOME:-~/.local/share}/pi-node/current/bin/node`
/// when it exists, else `node` on `PATH`.
pub fn node() -> PathBuf {
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

/// The Tier catalog for the `models` of a policy key, from the installed pi.
///
/// Errors: `pi_not_found`, `catalog_failed` (the module did not run or did not
/// print valid JSON), `tier_model_unknown` (a configured model is not in pi's
/// catalog).
///
/// This is the one place the catalog is obtained. It is cached in `tier_catalog`
/// per pi version and `models.json` fingerprint (commands.md §14.3): read when the cache holds the configured
/// models under the installed pi's version and the file's fingerprint; otherwise computed from pi and
/// written in one transaction (append-only: old rows stay).
pub fn load(config: &Config, models: &Models) -> Result<Catalog, CpdError> {
    // The open waits out a migration on the ambient clock, so a test passes a `ManualClock`.
    let mut store = Store::open_with(
        &*Ports::current().clock,
        &CpdHome::from_env()?,
        config.timeouts.store_busy_ms.into(),
        None,
    )?;
    load_with(models, &mut store)
}

/// [`load`] through a `store` the caller already holds, so the catalog does not
/// open a second one (flock, pragmas, schema hash).
pub fn load_with(models: &Models, store: &mut Store) -> Result<Catalog, CpdError> {
    load_from(&locate_pi()?, models, store)
}

fn load_from(pi: &PiInstall, models: &Models, store: &mut Store) -> Result<Catalog, CpdError> {
    let refs = [
        models.o.as_str(),
        models.s.as_str(),
        models.h.as_str(),
        models.g.as_str(),
        models.j.as_str(),
    ];
    let sha = models_sha(&pi.agent_dir)?;
    if let Some(cached) = cache::read_refs(store, &pi.version, &sha, refs)? {
        return Ok(cached);
    }
    let catalog = read(pi, models, NODE_DEADLINE)?;
    cache::write(store, &sha, &catalog)?;
    Ok(catalog)
}

/// The cache key's second part (commands.md §14.3): the lowercase hex SHA-256 of the bytes of `<agentDir>/models.json`, which pi
/// reads for custom models and their `thinkingLevelMap`; `""` when the file does not exist. Any other read error is `catalog_failed`.
fn models_sha(agent_dir: &Path) -> Result<String, CpdError> {
    let path = agent_dir.join("models.json");
    match std::fs::read(&path) {
        Ok(bytes) => Ok(format!("{:x}", Sha256::digest(bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(
            catalog_failed(format!("cannot read {}: {e}.", path.display()))
                .with("models_json", path.display().to_string()),
        ),
    }
}

/// Runs the embedded module with pi's Node (killed past `deadline`) and builds the [`Catalog`] from what it prints.
pub fn read(pi: &PiInstall, models: &Models, deadline: Duration) -> Result<Catalog, CpdError> {
    read_with(&*Ports::current().runner, pi, models, deadline)
}

/// [`read`] on `runner`.
pub fn read_with(
    runner: &dyn Runner,
    pi: &PiInstall,
    models: &Models,
    deadline: Duration,
) -> Result<Catalog, CpdError> {
    let tiers = [
        (Letter::O, &models.o),
        (Letter::S, &models.s),
        (Letter::H, &models.h),
        (Letter::G, &models.g),
        (Letter::J, &models.j),
    ];
    let node = node();
    let mut cmd = Command::new(&node);
    cmd.args(["--input-type=module", "-e", MODULE, "--"])
        .arg(&pi.coding_agent_entry)
        .arg(&pi.pi_ai_entry)
        .args(
            tiers
                .iter()
                .map(|(letter, model)| format!("{letter}={model}")),
        )
        .env("PI_MANAGED_INSTALL_ROOT", pi.agent_dir.join("install"))
        .current_dir(&pi.agent_dir)
        .stdin(Stdio::null());
    let out = runner
        .output(&mut cmd, "node", deadline)
        .map_err(|e| match e {
            Failed::Io(e) => catalog_failed(format!("cannot run {}: {e}.", node.display()))
                .with("node", node.display().to_string()),
            Failed::Timeout(e) => e,
        })?;
    if !out.status.success() {
        return Err(catalog_failed(format!(
            "the catalog module failed under pi {}.",
            pi.version
        ))
        .with("node", node.display().to_string())
        .with("exit", out.status.code().map_or(Value::Null, Value::from))
        .with("stderr", tail(&out.stderr)));
    }
    let levels: BTreeMap<String, Option<Vec<String>>> = serde_json::from_slice(&out.stdout)
        .map_err(|e| {
            catalog_failed(format!("the catalog module printed invalid JSON: {e}."))
                .with("stdout", tail(&out.stdout))
                .with("stderr", tail(&out.stderr))
        })?;

    let mut missing = Vec::new();
    let mut models = Vec::new();
    for (letter, model) in tiers {
        let Some(entry) = levels.get(&letter.to_string()) else {
            // `J` is the decide role's letter: a catalog computed before it existed has no row for it, so
            // the letter is left empty rather than refused (a scripted catalog in the tests does the same).
            if letter == Letter::J {
                models.push(TierModel {
                    model: String::new(),
                    efforts: Vec::new(),
                });
                continue;
            }
            return Err(catalog_failed(format!(
                "the catalog module printed nothing for Tier {letter}."
            ))
            .with("letter", letter.to_string()));
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
                    return Err(catalog_failed(format!(
                        "pi reports the unknown thinking level {level:?} for {model}."
                    ))
                    .with("model", model.clone())
                    .with("level", level.clone()));
                }
            }
        }
        efforts.sort_unstable();
        efforts.dedup();
        // Only `off`, or no level at all: no efforts, only `<letter>X` is valid (Tier model v2).
        if efforts == [0] {
            efforts.clear();
        }
        models.push(TierModel {
            model: model.clone(),
            efforts,
        });
    }
    if !missing.is_empty() {
        return Err(CpdError::new(
            ErrorCode::TierModelUnknown,
            "A configured model is not in pi's catalog.",
        )
        .with("missing", missing)
        .with("pi_version", pi.version.clone()));
    }
    let [o, s, h, g, j]: [TierModel; 5] = models.try_into().expect("one model per letter");
    Ok(Catalog {
        pi_version: pi.version.clone(),
        o,
        s,
        h,
        g,
        j,
    })
}

#[cfg(test)]
#[path = "catalog_cache_tests.rs"]
mod cache_tests;

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
        let models = crate::policy::default_policy().model.clone();
        let tier = |model: &str| TierModel {
            model: model.to_owned(),
            efforts: vec![0, 1, 2],
        };
        let cached = Catalog {
            pi_version: "9.9.9".into(),
            o: tier(&models.o),
            s: tier(&models.s),
            h: tier(&models.h),
            g: TierModel {
                model: models.g.clone(),
                efforts: vec![],
            },
            j: tier(&models.j),
        };
        cache::write(&mut store, "", &cached).unwrap();

        // pi's entry points do not exist: a cache miss would fail in `read`.
        assert_eq!(load_from(&pi(), &models, &mut store).unwrap(), cached);
    }
}
