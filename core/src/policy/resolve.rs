//! `resolve(home, &PolicyKey)` (contract §8.3 "Keys and resolution").

use std::path::PathBuf;
use std::sync::OnceLock;

use super::domain::{Domain, Policy, Scope, Source, Sources, merge};
use super::exposure::ScopeKind;
use super::load::{load_scope_file, parse_scope, policy_invalid};
use super::model::check_one_provider;
use super::{PolicyKey, Resolved, owner_unconfirmed};
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, RepoName};

const DEFAULT_FILE: &str = "core/src/policy/default.toml";
const DEFAULT_TOML: &str = include_str!("default.toml");

/// cpd's compiled default, parsed by the same loader as every scope file.
pub fn default_policy() -> &'static Policy {
    static DEFAULT: OnceLock<Policy> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        let file = std::path::Path::new(DEFAULT_FILE);
        parse_scope(file, DEFAULT_TOML)
            .ok()
            .and_then(|scope| scope.complete())
            .expect("core/src/policy/default.toml sets every domain and every typed key (pinned by a test)")
    })
}

/// `$CPD_HOME/policy/`: the one tree.
pub fn policy_dir(home: &CpdHome) -> PathBuf {
    home.root().join("policy")
}

/// `policy/host.toml`: the machine scope.
pub(super) fn machine_path(home: &CpdHome) -> PathBuf {
    policy_dir(home).join(MACHINE_FILE)
}

/// `policy/<owner>/owner.toml`: the owner scope, and the one thing that makes an owner known.
pub(super) fn owner_path(home: &CpdHome, owner: &OwnerLogin) -> PathBuf {
    policy_dir(home).join(owner.as_str()).join(OWNER_FILE)
}

/// `policy/<owner>/<repo>.toml`: the repo scope.
pub fn repo_path(home: &CpdHome, owner: &OwnerLogin, name: &RepoName) -> PathBuf {
    policy_dir(home)
        .join(owner.as_str())
        .join(format!("{name}.toml"))
}

const OLD_LAYOUT: &str = "the old layout owners/, repos/, machines/ is no longer read";
const MACHINE_FILE: &str = "host.toml";
pub(super) const OWNER_FILE: &str = "owner.toml";

/// The scope layers of `key`, most specific first (repo, owner, machine), or `None` when the owner
/// has no file. A missing repo or machine file is no layer.
fn layers(
    home: &CpdHome,
    key: &PolicyKey,
) -> Result<Option<Vec<(Source, Scope)>>, crate::error::CpdError> {
    let Some((owner_real, owner_scope)) =
        load_scope_file(&owner_path(home, key.owner()), ScopeKind::Owner)?
    else {
        return Ok(None);
    };
    let mut layers = Vec::new();
    if let PolicyKey::Repo(owner, name) = key {
        let path = repo_path(home, owner, name);
        if let Some((real, scope)) = load_scope_file(&path, ScopeKind::Repo)? {
            layers.push((Source::Repo(real), scope));
        }
    }
    layers.push((Source::Owner(owner_real), owner_scope));
    if let Some((real, scope)) = load_scope_file(&machine_path(home), ScopeKind::Machine)? {
        layers.push((Source::Machine(real), scope));
    }
    Ok(Some(layers))
}

/// The merge of `layers` over the default; a `[model]` table of two providers is `policy_invalid`,
/// naming the file that set it.
fn merge_layers(layers: &[(Source, Scope)]) -> Result<Resolved, crate::error::CpdError> {
    let refs: Vec<(Source, &Scope)> = layers.iter().map(|(s, sc)| (s.clone(), sc)).collect();
    let resolved = merge(default_policy(), &refs);
    let file = match &resolved.sources.get(Domain::Model).source {
        Source::Machine(p) | Source::Owner(p) | Source::Repo(p) => p.clone(),
        Source::Default | Source::DefaultNoOwnerFile => PathBuf::from(DEFAULT_FILE),
    };
    check_one_provider(&resolved.policy.model, &file)?;
    Ok(resolved)
}

/// The policy of `key`: cpd's default, then `host.toml`, the owner file and the repo file,
/// per domain, the whole domain replaced. A key whose owner has no file is
/// `owner_unconfirmed`, even when its repo file exists. A missing repo file is no repo scope
/// (the same as an empty one).
pub fn resolve(home: &CpdHome, key: &PolicyKey) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key)? {
        Some(layers) => merge_layers(&layers),
        None => Err(owner_unconfirmed(home, key.owner())),
    }
}

/// [`resolve`] for a `pull_requests` row no PR owns:
/// an owner without a file is cpd's default, source `default (no owner file)`,
/// not `owner_unconfirmed`.
pub fn resolve_unowned_row(
    home: &CpdHome,
    key: &PolicyKey,
) -> Result<Resolved, crate::error::CpdError> {
    match layers(home, key)? {
        Some(layers) => merge_layers(&layers),
        None => Ok(Resolved {
            policy: default_policy().clone(),
            sources: Sources::all(Source::DefaultNoOwnerFile),
            warnings: Vec::new(),
        }),
    }
}

/// The machine layer over cpd's default alone: `host.toml` when it exists.
pub fn resolve_machine(home: &CpdHome) -> Result<Resolved, crate::error::CpdError> {
    match load_scope_file(&machine_path(home), ScopeKind::Machine)? {
        Some((real, scope)) => merge_layers(&[(Source::Machine(real), scope)]),
        None => merge_layers(&[]),
    }
}

/// The key of a scope file by its path under `$CPD_HOME/policy/`: `<owner>/owner.toml` or
/// `<owner>/<repo>.toml`. A name that is not a valid login or name is `policy_invalid`.
pub fn key_of_scope_file(
    home: &CpdHome,
    path: &std::path::Path,
) -> Result<PolicyKey, crate::error::CpdError> {
    let bad = |why: &str| policy_invalid(path, None, why);
    let rel = path
        .strip_prefix(policy_dir(home))
        .map_err(|_| bad("not under the policy directory"))?;
    let parts: Vec<&str> = rel.iter().map(|p| p.to_str().unwrap_or("")).collect();
    match parts.as_slice() {
        [owner, file] => {
            let owner = owner_dir_name(owner)
                .map_err(|_| bad("the directory name is not a valid owner login"))?;
            if *file == OWNER_FILE {
                return Ok(PolicyKey::Owner(owner));
            }
            let name = file
                .strip_suffix(".toml")
                .ok_or_else(|| bad("a scope file ends in .toml"))?;
            // `parse` lowercases; a file name is the lowercase form itself, or the key never finds it.
            match RepoName::parse(name) {
                Ok(n) if n.as_str() == name => Ok(PolicyKey::Repo(owner, n)),
                _ => Err(bad("the file name is not a valid repo name")),
            }
        }
        _ => Err(bad("expected <owner>/owner.toml or <owner>/<repo>.toml")),
    }
}

/// The owner a directory name is: a valid login that is its own lowercase form.
fn owner_dir_name(name: &str) -> Result<OwnerLogin, ()> {
    match OwnerLogin::parse(name) {
        Ok(o) if o.as_str() == name => Ok(o),
        _ => Err(()),
    }
}

/// The names of the visible entries of `dir` (not starting with `.`), sorted; a missing `dir` has none.
fn entries(dir: &std::path::Path) -> Result<Vec<PathBuf>, crate::error::CpdError> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(crate::error::CpdError::new(
                crate::error::ErrorCode::FileUnreadable,
                format!("Cannot read the policy directory {}: {e}.", dir.display()),
            )
            .with("file", dir.display().to_string()));
        }
    };
    let mut paths: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."))
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// Every entry of `$CPD_HOME/policy/` is checked by name, in one walk (sorted, the first bad one is
/// `policy_invalid`): `host.toml`; a directory with a valid owner login for a name that holds only
/// `owner.toml` and `<repo>.toml` files with valid repo names. A directory with any of those and no
/// `owner.toml` is the old layout's. Names starting with `.` are ignored.
pub fn check_file_names(home: &CpdHome) -> Result<(), crate::error::CpdError> {
    let is_dir = |p: &std::path::Path| std::fs::metadata(p).is_ok_and(|m| m.is_dir());
    for entry in entries(&policy_dir(home))? {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !is_dir(&entry) {
            if name != MACHINE_FILE {
                return Err(policy_invalid(
                    &entry,
                    None,
                    "expected host.toml or an <owner>/ directory",
                ));
            }
            continue;
        }
        if owner_dir_name(name).is_err() {
            return Err(policy_invalid(
                &entry,
                None,
                "the directory name is not a valid owner login (expected host.toml or an <owner>/ directory)",
            ));
        }
        let files = entries(&entry)?;
        for file in &files {
            if is_dir(file) {
                return Err(policy_invalid(
                    &entry,
                    None,
                    format!("{name}/ holds a directory; {OLD_LAYOUT}"),
                ));
            }
            key_of_scope_file(home, file)?;
        }
        if !files.is_empty()
            && !files
                .iter()
                .any(|f| f.file_name().is_some_and(|n| n == OWNER_FILE))
        {
            return Err(policy_invalid(
                &entry,
                None,
                format!("{name}/ has files but no owner.toml; {OLD_LAYOUT}"),
            ));
        }
    }
    Ok(())
}
