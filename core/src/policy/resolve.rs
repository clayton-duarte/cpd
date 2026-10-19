//! `resolve(home, &PolicyKey)` (contract §8.3 "Keys and resolution").

use std::path::PathBuf;
use std::sync::OnceLock;

use super::domain::{merge, Policy, Scope, Source, Sources};
use super::load::{load_scope_file, parse_scope, policy_invalid};
use super::{owner_unknown, PolicyKey, Resolved};
use crate::home::CpdHome;
use crate::ids::OwnerLogin;

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

fn owner_file(home: &CpdHome, owner: &OwnerLogin) -> PathBuf {
    owners_dir(home).join(format!("{owner}.toml"))
}

/// The scope layers of `key`, most specific first, or `None` when the owner has no file.
fn layers(home: &CpdHome, key: &PolicyKey) -> Result<Option<Vec<(Source, Scope)>>, crate::error::CpdError> {
    let Some((owner_real, owner_scope)) = load_scope_file(&owner_file(home, key.owner()))? else {
        return Ok(None);
    };
    let mut layers = Vec::new();
    if let PolicyKey::Repo(owner, name) = key {
        let path = repos_dir(home).join(owner.as_str()).join(format!("{name}.toml"));
        if let Some((real, scope)) = load_scope_file(&path)? {
            layers.push((Source::Repo(real), scope));
        }
    }
    layers.push((Source::Owner(owner_real), owner_scope));
    Ok(Some(layers))
}

fn merge_layers(layers: &[(Source, Scope)]) -> Resolved {
    let refs: Vec<(Source, &Scope)> = layers.iter().map(|(s, sc)| (s.clone(), sc)).collect();
    merge(default_policy(), &refs)
}

/// The policy of `key`: cpd's default, then the owner file, then the repo file,
/// per domain, the whole domain replaced. A key whose owner has no file is
/// `owner_unknown`, even when its repo file exists.
pub fn resolve(home: &CpdHome, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key)? {
        Some(layers) => Ok(merge_layers(&layers)),
        None => Err(owner_unknown(key.owner())),
    }
}

/// [`resolve`] for a `pull_requests` row no PR owns (the Inbox's `pr-failing`):
/// an owner without a file is cpd's default, source `default (no owner file)`,
/// not `owner_unknown`.
pub fn resolve_unowned_row(home: &CpdHome, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key)? {
        Some(layers) => Ok(merge_layers(&layers)),
        None => Ok(Resolved {
            policy: default_policy().clone(),
            sources: Sources::all(Source::DefaultNoOwnerFile),
        }),
    }
}

/// The key of a scope file by its path under `$CPD_HOME/policy/`: `owners/<owner>.toml`
/// or `repos/<owner>/<name>.toml`. A name that is not a valid login or name is `policy_invalid`.
pub fn key_of_scope_file(home: &CpdHome, path: &std::path::Path) -> Result<PolicyKey, crate::error::CpdError> {
    let bad = |why: &str| policy_invalid(path, None, why);
    let rel = path.strip_prefix(home.root().join("policy")).map_err(|_| bad("not under the policy directory"))?;
    let parts: Vec<&str> = rel.iter().map(|p| p.to_str().unwrap_or("")).collect();
    let stem = |f: &str| f.strip_suffix(".toml").map(str::to_owned);
    match parts.as_slice() {
        ["owners", file] => {
            let owner = stem(file).ok_or_else(|| bad("a scope file ends in .toml"))?;
            Ok(PolicyKey::Owner(OwnerLogin::parse(&owner).map_err(|_| bad("the file name is not a valid owner login"))?))
        }
        ["repos", owner, file] => {
            let name = stem(file).ok_or_else(|| bad("a scope file ends in .toml"))?;
            Ok(PolicyKey::Repo(
                OwnerLogin::parse(owner).map_err(|_| bad("the directory name is not a valid owner login"))?,
                crate::ids::RepoName::parse(&name).map_err(|_| bad("the file name is not a valid repo name"))?,
            ))
        }
        _ => Err(bad("expected owners/<owner>.toml or repos/<owner>/<name>.toml")),
    }
}
