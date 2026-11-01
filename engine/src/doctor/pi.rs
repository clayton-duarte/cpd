//! pi's side of the machine (commands.md §14.1): the catalog, the adapter, the
//! Lead's packages.

use std::path::Path;

use serde_json::{Value, json};

use super::check::{Check, Severity};
use super::deadline;
use cpd_core::config::catalog::read as read_catalog;
use cpd_core::config::{Config, locate_pi};
use cpd_core::host::pi::{PackageId, PiDirs, declared, package_identity, project_settings};
use cpd_core::policy::model::Models;

/// Each model of the machine layer's `[model]` (over cpd's default) is in pi's catalog and `lead_tier` is valid for its model.
///
/// The catalog is asked of pi each time, never read from the cache, so the check
/// proves pi answers and doctor writes nothing.
pub fn tiers(config: &Config) -> Check {
    let home = match cpd_core::home::CpdHome::from_env() {
        Ok(home) => home,
        Err(e) => return Check::from_error("tiers", Severity::Error, &e),
    };
    let models = match cpd_core::policy::resolve_machine(&home) {
        Ok(resolved) => Models::of(&resolved.policy),
        Err(e) => return Check::from_error("tiers", Severity::Error, &e),
    };
    let name = "tiers";
    let pi = match locate_pi() {
        Ok(pi) => pi,
        Err(e) => return Check::from_error(name, Severity::Error, &e),
    };
    let catalog = match read_catalog(&pi, &models, deadline()) {
        Ok(catalog) => catalog,
        Err(e) => return Check::from_error(name, Severity::Error, &e),
    };
    match catalog
        .check(config.lead_tier)
        .and_then(|()| catalog.check(config.review_lead_tier))
    {
        Ok(()) => Check::pass(name, Severity::Error).found(catalog.pi_version),
        Err(e) => Check::from_error(name, Severity::Error, &e),
    }
}

/// The `pi_adapter` directory has a `package.json`.
pub fn adapter(config: &Config) -> Check {
    let name = "pi_adapter";
    let dir = match config.pi_adapter_dir() {
        Ok(dir) => dir,
        Err(e) => return Check::from_error(name, Severity::Error, &e),
    };
    let found = dir.display().to_string();
    if dir.join("package.json").is_file() {
        Check::pass(name, Severity::Error).found(found)
    } else {
        Check::fail(name, Severity::Error, format!("{found} has no package.json: set pi_adapter, or install the pi package beside the binary."))
            .found(found)
    }
}

/// For the adapter and each `lead_pi_packages` entry: whether a Lead gets it by
/// `-e` or from which pi settings file (contract §6.5). Fails when an entry is
/// neither declared in a settings file nor resolvable: pi fetches an `npm:` or
/// git source named by `-e`, but a local path has to exist.
pub fn lead_packages(config: &Config) -> Check {
    let name = "lead_packages";
    let (dirs, cwd) = match (PiDirs::from_env(), std::env::current_dir()) {
        (Ok(dirs), Ok(cwd)) => (dirs, cwd),
        (Err(e), _) => return Check::from_error(name, Severity::Warn, &e),
        (_, Err(e)) => {
            return Check::fail(
                name,
                Severity::Warn,
                format!("The current directory is unreadable: {e}."),
            );
        }
    };
    let adapter = match config.pi_adapter_dir() {
        Ok(dir) => dir.display().to_string(),
        Err(e) => return Check::from_error(name, Severity::Warn, &e),
    };
    // Where pi's global and project settings declare packages; a log nobody reads.
    let mut declarations = declared(&dirs.global_settings(), &dirs.home, &mut std::io::sink());
    declarations.extend(declared(
        &project_settings(&cwd),
        &dirs.home,
        &mut std::io::sink(),
    ));

    let mut report = Vec::new();
    let mut unresolvable = Vec::new();
    for candidate in std::iter::once(&adapter).chain(&config.lead_pi_packages) {
        let id = package_identity(candidate, &cwd, &dirs.home);
        let entry = if let Some(d) = declarations.iter().find(|d| d.id == id) {
            json!({"package": candidate, "by": "settings", "settings": d.settings.display().to_string()})
        } else if matches!(&id, PackageId::Local(path) if !Path::new(path).exists()) {
            unresolvable.push(candidate.as_str());
            json!({"package": candidate, "by": "-e", "resolvable": false})
        } else {
            json!({"package": candidate, "by": "-e"})
        };
        report.push(entry);
    }
    let check =
        Check::new(name, Severity::Warn, unresolvable.is_empty()).found(Value::Array(report));
    if unresolvable.is_empty() {
        return check;
    }
    check.message(format!(
        "Neither declared in pi's settings nor resolvable: {}.",
        unresolvable.join(", ")
    ))
}
