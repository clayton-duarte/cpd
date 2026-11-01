//! Loading and validating a scope file (contract §8.3 "Precedence and replace",
//! "Validation").
//!
//! A file is parsed once into a [`Scope`]: per domain, `None` when the file has
//! no table, `Some` with every key an `Option` otherwise, so an absent table,
//! a set table and an empty table (`[pr]`, cpd's default for the whole domain)
//! stay distinguishable.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use toml::de::{DeTable, DeValue};
use toml::{Table, Value};

use super::domain::{GatesSet, HooksSet, Scope};
use super::exposure::{check_exposed, ScopeKind};
use crate::git::hooks::parse_value;
use crate::error::{CpdError, ErrorCode};

/// `policy_invalid` (exit 1): `file`, and `key` (null when the file does not parse).
pub fn policy_invalid(file: &Path, key: Option<&str>, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::PolicyInvalid, format!("Invalid policy file {}: {why}.", file.display()))
        .with("file", file.display().to_string())
        .with("key", key.map_or(serde_json::Value::Null, |k| serde_json::Value::String(k.to_owned())))
}

/// The machine file's table of MCP servers: read by `machine::parse_machine` (contract §8.3 "MCP servers"), not a domain of [`Scope`].
const MCP: &str = "mcp";

/// Parses the text of the scope file `file`.
pub fn parse_scope(file: &Path, text: &str) -> Result<Scope, CpdError> {
    parse_scope_of(file, text, None)
}

/// [`parse_scope`] for a file of `kind`: the `[mcp]` table of a machine file is left to `machine::parse_machine`; in any other file it is refused.
fn parse_scope_of(file: &Path, text: &str, kind: Option<ScopeKind>) -> Result<Scope, CpdError> {
    let mut table: Table = text.parse().map_err(|e: toml::de::Error| {
        policy_invalid(file, None, format!("not valid TOML ({})", e.message()))
    })?;
    if kind == Some(ScopeKind::Machine) {
        table.remove(MCP);
    }
    // `[hooks]` is read from the text, which keeps the file order of a block's commands (a `Table` sorts keys).
    let hooks = table.remove(HOOKS).map(|_| hooks_set(file, text)).transpose()?;
    let mut scope = match Value::Table(table.clone()).try_into::<Scope>() {
        Ok(scope) => scope,
        Err(e) => {
            let path = bad_key::<Scope>(&table);
            let why = if path.first().is_some_and(|k| k == "mcp") {
                "`mcp` is set in a machine file (policy/host.toml), not in an owner or repo file".to_owned()
            } else {
                e.message().to_owned()
            };
            return Err(policy_invalid(file, Some(&path.join(".")), why));
        }
    };
    scope.hooks = hooks;
    // The deprecated `[pr] gate` alias: `local` -> both gates `local`, `ci`/`none` -> both `none`.
    // An explicit `[gate]` table in the same file wins over it.
    if scope.gate.is_none()
        && let Some(alias) = scope.pr.as_ref().and_then(|pr| pr.gate) {
            let gate = alias.gate();
            scope.gate = Some(GatesSet { merge: Some(gate), pr: Some(gate), text: None });
        }
    if let Some(set) = &scope.resources {
        for (name, decl) in &set.0 {
            if name.as_str() == crate::resource::WORKERS {
                return Err(policy_invalid(file, Some("resources.workers"), "`workers` is the built-in resource and cannot be declared"));
            }
            if decl.slots.is_some_and(|n| !(1..=64).contains(&n)) {
                return Err(policy_invalid(file, Some(&format!("resources.{}.slots", name.as_str())), "`slots` is a whole number from 1 to 64"));
            }
        }
    }
    if let Some(jira) = scope.pr.as_ref().and_then(|pr| pr.jira.as_ref()) {
        super::jira::check(jira).map_err(|(name, why)| policy_invalid(file, Some(&format!("pr.jira.{name}")), why))?;
    }
    if let Some(post) = scope.pr.as_ref().and_then(|pr| pr.post.as_ref()) {
        super::post::check(post).map_err(|(name, why)| policy_invalid(file, Some(&format!("pr.post.{name}")), why))?;
    }
    if let Some(set) = &scope.test_commands {
        set.commands().map_err(|why| policy_invalid(file, Some("test-commands"), why))?;
    }
    if let Some(set) = &scope.deploys {
        set.check().map_err(|why| policy_invalid(file, Some("deploys.workflows"), why))?;
    }
    Ok(scope)
}

const HOOKS: &str = "hooks";

/// The `[hooks]` table of the scope file text: each key one of the three forms of contract §7.3.
fn hooks_set(file: &Path, text: &str) -> Result<HooksSet, CpdError> {
    let doc = DeTable::parse(text)
        .map_err(|e| policy_invalid(file, None, format!("not valid TOML ({})", e.message())))?;
    let (_, value) = doc
        .get_ref()
        .iter()
        .find(|(k, _)| k.get_ref().as_ref() == HOOKS)
        .expect("the caller found a `hooks` key");
    let DeValue::Table(entries) = value.get_ref() else {
        return Err(policy_invalid(file, Some(HOOKS), "`hooks` must be a table"));
    };
    let mut set = HooksSet::default();
    for (key, value) in entries.iter() {
        let name = key.get_ref().as_ref();
        let slot = match name {
            "pre-start" => &mut set.pre_start,
            "pre-commit" => &mut set.pre_commit,
            "pre-merge" => &mut set.pre_merge,
            _ => {
                return Err(policy_invalid(
                    file,
                    Some(&format!("{HOOKS}.{name}")),
                    format!("unknown hook `{name}` in [hooks] (expected pre-start, pre-commit or pre-merge)"),
                ));
            }
        };
        let pipeline = parse_value(value, &format!("`{name}` in [hooks]"))
            .map_err(|why| policy_invalid(file, Some(&format!("{HOOKS}.{name}")), why.trim_end_matches('.')))?;
        *slot = Some(pipeline);
    }
    Ok(set)
}

/// The deepest key path that makes `table` fail to read as `T`, found by
/// reading one entry at a time (a table entry is descended into only when it
/// is acceptable empty: an unknown table is itself the bad key).
pub(super) fn bad_key<T: DeserializeOwned>(table: &Table) -> Vec<String> {
    fn nest(path: &[String], leaf: Value) -> Table {
        path.iter().rev().fold(leaf, |acc, k| Value::Table(Table::from_iter([(k.clone(), acc)]))).try_into().unwrap_or_default()
    }
    fn fails<T: DeserializeOwned>(path: &[String], leaf: Value) -> bool {
        Value::Table(nest(path, leaf)).try_into::<T>().is_err()
    }
    fn walk<T: DeserializeOwned>(table: &Table, path: &mut Vec<String>) -> bool {
        for (k, v) in table {
            path.push(k.clone());
            if fails::<T>(path, v.clone()) {
                if let Value::Table(sub) = v
                    && !fails::<T>(path, Value::Table(Table::new())) && walk::<T>(sub, path) {
                        return true;
                    }
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    walk::<T>(table, &mut path);
    path
}

/// Reads and parses the scope file at `path`: `Ok(None)` when it does not
/// exist (a dangling symlink included), else its realpath and its [`Scope`], which sets only
/// domains exposed to `kind`.
pub fn load_scope_file(path: &Path, kind: ScopeKind) -> Result<Option<(PathBuf, Scope)>, CpdError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::FileUnreadable,
                format!("Cannot read the policy file {}: {e}.", path.display()),
            )
            .with("file", path.display().to_string()));
        }
    };
    let real = std::fs::canonicalize(path).map_err(|e| {
        CpdError::new(ErrorCode::FileUnreadable, format!("Cannot resolve {}: {e}.", path.display()))
            .with("file", path.display().to_string())
    })?;
    let scope = parse_scope_of(&real, &text, Some(kind))?;
    check_exposed(&real, kind, &scope)?;
    Ok(Some((real, scope)))
}
