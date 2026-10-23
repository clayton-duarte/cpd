//! `resolve(home, &PolicyKey)` (contract §8.3 "Keys and resolution").

use std::path::PathBuf;
use std::sync::OnceLock;

use super::domain::{merge, Domain, Policy, Scope, Source, Sources};
use super::load::{load_scope_file, parse_scope, policy_invalid};
use super::{owner_unknown, repo_unconfigured, PolicyKey, Resolved};
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

/// What a missing repo file is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RepoFile {
    /// `repo_unconfigured`.
    Required,
    /// No repo scope.
    Optional,
}

/// The repo-only domains (contract §8.3): `Some(domain)` of the first one an owner file has a table of.
fn repo_only(scope: &Scope) -> Option<Domain> {
    [
        (Domain::Hooks, scope.hooks.is_some()),
        (Domain::TestCommands, scope.test_commands.is_some()),
        (Domain::Protected, scope.protected.is_some()),
    ]
    .into_iter()
    .find_map(|(domain, set)| set.then_some(domain))
}

/// The scope layers of `key`, most specific first, or `None` when the owner has no file.
fn layers(
    home: &CpdHome,
    key: &PolicyKey,
    repo_file: RepoFile,
) -> Result<Option<Vec<(Source, Scope)>>, crate::error::CpdError> {
    let Some((owner_real, owner_scope)) = load_scope_file(&owner_file(home, key.owner()))? else {
        return Ok(None);
    };
    if let Some(domain) = repo_only(&owner_scope) {
        return Err(policy_invalid(&owner_real, Some(domain.name()), format!("`{}` is set in a repo file only", domain.name())));
    }
    let mut layers = Vec::new();
    if let PolicyKey::Repo(owner, name) = key {
        let path = repos_dir(home).join(owner.as_str()).join(format!("{name}.toml"));
        match load_scope_file(&path)? {
            Some((real, scope)) => layers.push((Source::Repo(real), scope)),
            None if repo_file == RepoFile::Required => return Err(repo_unconfigured(owner, name, &path)),
            None => {}
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
/// `owner_unknown`, even when its repo file exists. A Repo key whose repo file is missing is
/// `repo_unconfigured` (an empty file is valid), after the owner check.
pub fn resolve(home: &CpdHome, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key, RepoFile::Required)? {
        Some(layers) => Ok(merge_layers(&layers)),
        None => Err(owner_unknown(key.owner())),
    }
}

/// [`resolve`] with the repo file optional: a missing one is no repo scope (owner and default only).
/// For the `pr poll`, which needs only `pr.feedback` and keeps polling an open PR of an unconfigured repo.
pub fn resolve_lenient(home: &CpdHome, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key, RepoFile::Optional)? {
        Some(layers) => Ok(merge_layers(&layers)),
        None => Err(owner_unknown(key.owner())),
    }
}

/// [`resolve`] for a `pull_requests` row no PR owns:
/// an owner without a file is cpd's default, source `default (no owner file)`,
/// not `owner_unknown`.
pub fn resolve_unowned_row(home: &CpdHome, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key, RepoFile::Optional)? {
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
/// owner login or repo name for a name: the first that has not is `policy_invalid` (sorted order).
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
    Ok(())
}
