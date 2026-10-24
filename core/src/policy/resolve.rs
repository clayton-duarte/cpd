//! `resolve(home, &PolicyKey)` (contract §8.3 "Keys and resolution").

use std::path::PathBuf;
use std::sync::OnceLock;

use super::domain::{merge, Policy, Scope, Source, Sources};
use super::exposure::ScopeKind;
use super::load::{load_scope_file, parse_scope, policy_invalid};
use super::{owner_unconfirmed, PolicyKey, Resolved};
use crate::home::CpdHome;
use crate::ids::{HostName, OwnerLogin};

const DEFAULT_TOML: &str = include_str!("default.toml");

/// cpd's compiled default, parsed by the same loader as every scope file.
pub fn default_policy() -> &'static Policy {
    static DEFAULT: OnceLock<Policy> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        let file = std::path::Path::new("core/src/policy/default.toml");
        parse_scope(file, DEFAULT_TOML)
            .ok()
            .and_then(|scope| scope.complete())
            .expect("core/src/policy/default.toml sets every domain and every typed key (pinned by a test)")
    })
}

pub(super) fn owners_dir(home: &CpdHome) -> PathBuf {
    home.root().join("policy").join("owners")
}

pub(super) fn repos_dir(home: &CpdHome) -> PathBuf {
    home.root().join("policy").join("repos")
}

pub(super) fn machines_dir(home: &CpdHome) -> PathBuf {
    home.root().join("policy").join("machines")
}

pub(super) fn machine_file(home: &CpdHome, host: &HostName) -> PathBuf {
    machines_dir(home).join(format!("{host}.toml"))
}

pub(super) fn owner_file(home: &CpdHome, owner: &OwnerLogin) -> PathBuf {
    owners_dir(home).join(format!("{owner}.toml"))
}

/// The scope layers of `key`, most specific first (repo, owner, machine), or `None` when the owner
/// has no file. A missing repo or machine file is no layer.
fn layers(
    home: &CpdHome,
    host: &HostName,
    key: &PolicyKey,
) -> Result<Option<Vec<(Source, Scope)>>, crate::error::CpdError> {
    let Some((owner_real, owner_scope)) = load_scope_file(&owner_file(home, key.owner()), ScopeKind::Owner)? else {
        return Ok(None);
    };
    let mut layers = Vec::new();
    if let PolicyKey::Repo(owner, name) = key {
        let path = repos_dir(home).join(owner.as_str()).join(format!("{name}.toml"));
        if let Some((real, scope)) = load_scope_file(&path, ScopeKind::Repo)? {
            layers.push((Source::Repo(real), scope));
        }
    }
    layers.push((Source::Owner(owner_real), owner_scope));
    if let Some((real, scope)) = load_scope_file(&machine_file(home, host), ScopeKind::Machine)? {
        layers.push((Source::Machine(real), scope));
    }
    Ok(Some(layers))
}

fn merge_layers(layers: &[(Source, Scope)]) -> Resolved {
    let refs: Vec<(Source, &Scope)> = layers.iter().map(|(s, sc)| (s.clone(), sc)).collect();
    merge(default_policy(), &refs)
}

/// The policy of `key`: cpd's default, then the machine file, the owner file and the repo file,
/// per domain, the whole domain replaced. A key whose owner has no file is
/// `owner_unconfirmed`, even when its repo file exists. A missing repo file is no repo scope
/// (the same as an empty one).
pub fn resolve(home: &CpdHome, host: &HostName, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, host, key)? {
        Some(layers) => Ok(merge_layers(&layers)),
        None => Err(owner_unconfirmed(home, key.owner())),
    }
}

/// [`resolve`] for a `pull_requests` row no PR owns:
/// an owner without a file is cpd's default, source `default (no owner file)`,
/// not `owner_unconfirmed`.
pub fn resolve_unowned_row(home: &CpdHome, host: &HostName, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, host, key)? {
        Some(layers) => Ok(merge_layers(&layers)),
        None => Ok(Resolved {
            policy: default_policy().clone(),
            sources: Sources::all(Source::DefaultNoOwnerFile),
        }),
    }
}

/// The machine layer over cpd's default alone: the host's file when it exists.
pub fn resolve_machine(home: &CpdHome, host: &HostName) -> Result<Resolved, crate::error::CpdError> {
    match load_scope_file(&machine_file(home, host), ScopeKind::Machine)? {
        Some((real, scope)) => Ok(merge_layers(&[(Source::Machine(real), scope)])),
        None => Ok(merge_layers(&[])),
    }
}

/// The key of a scope file by its path under `$CPD_HOME/policy/`: `owners/<owner>.toml`
/// or `repos/<owner>/<name>.toml`. A name that is not a valid login or name is `policy_invalid`.
pub fn key_of_scope_file(home: &CpdHome, path: &std::path::Path) -> Result<PolicyKey, crate::error::CpdError> {
    let bad = |why: &str| policy_invalid(path, None, why);
    let rel = path.strip_prefix(home.root().join("policy")).map_err(|_| bad("not under the policy directory"))?;
    let parts: Vec<&str> = rel.iter().map(|p| p.to_str().unwrap_or("")).collect();
    let stem = |f: &str| f.strip_suffix(".toml").map(str::to_owned);
    // `parse` lowercases; a file name is the lowercase form itself, or the key never finds it.
    let owner_of = |s: &str, what: &str| match OwnerLogin::parse(s) {
        Ok(o) if o.as_str() == s => Ok(o),
        _ => Err(bad(what)),
    };
    match parts.as_slice() {
        ["owners", file] => {
            let owner = stem(file).ok_or_else(|| bad("a scope file ends in .toml"))?;
            Ok(PolicyKey::Owner(owner_of(&owner, "the file name is not a valid owner login")?))
        }
        ["repos", owner, file] => {
            let name = stem(file).ok_or_else(|| bad("a scope file ends in .toml"))?;
            let name = match crate::ids::RepoName::parse(&name) {
                Ok(n) if n.as_str() == name => n,
                _ => return Err(bad("the file name is not a valid repo name")),
            };
            Ok(PolicyKey::Repo(owner_of(owner, "the directory name is not a valid owner login")?, name))
        }
        _ => Err(bad("expected owners/<owner>.toml or repos/<owner>/<name>.toml")),
    }
}

/// Every `*.toml` file under `owners/` and `repos/<owner>/` of `$CPD_HOME/policy/` has a valid
/// owner login or repo name for a name, and every entry of `machines/` is `<host>.toml` with a valid
/// host name: the first that has not is `policy_invalid` (sorted order).
pub fn check_file_names(home: &CpdHome) -> Result<(), crate::error::CpdError> {
    fn toml_files(dir: &std::path::Path) -> Result<Vec<PathBuf>, crate::error::CpdError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(crate::error::CpdError::new(
                    crate::error::ErrorCode::FileUnreadable,
                    format!("Cannot read the policy directory {}: {e}.", dir.display()),
                )
                .with("file", dir.display().to_string()));
            }
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        Ok(paths)
    }
    let is_toml = |p: &PathBuf| p.extension().is_some_and(|e| e == "toml") && p.is_file();
    for file in toml_files(&owners_dir(home))?.into_iter().filter(is_toml) {
        key_of_scope_file(home, &file)?;
    }
    for dir in toml_files(&repos_dir(home))?.into_iter().filter(|p| p.is_dir()) {
        for file in toml_files(&dir)?.into_iter().filter(is_toml) {
            key_of_scope_file(home, &file)?;
        }
    }
    for entry in toml_files(&machines_dir(home))? {
        let valid = entry
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".toml"))
            .is_some_and(|host| HostName::parse(host).is_ok());
        if !valid {
            return Err(policy_invalid(&entry, None, "expected machines/<host>.toml with a valid host name"));
        }
    }
    Ok(())
}
