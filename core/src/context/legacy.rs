//! The git stores of earlier versions (contract §4.5 migration 0007, §4.6):
//! `$CPD_HOME/contexts/<id>/` with `context.json` and `records/<machine>.jsonl`.
//! Only migration 0007 reads them, once, and every file is parsed here, so
//! what leaves this module is typed. It depends on no database code, so the
//! store's migration can call it.
//!
//! Errors are messages: a store that is not valid fails the migration, which
//! reports it as `migration_failed`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Jira;
use super::record::{Record, VERSION};
use crate::ids::{ContextId, MachineName};

/// `context.json`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    v: u32,
    pub id: ContextId,
    pub title: String,
    pub jira: Option<String>,
    pub created_at: String,
    pub created_on: MachineName,
}

/// One old store, read.
#[derive(Clone, Debug)]
pub struct OldStore {
    pub manifest: Manifest,
    /// Every record of every file, files in name order and lines in file order.
    pub records: Vec<Record>,
}

/// The folders of `contexts/`, in name order; none when there is no such
/// directory. Files beside them are not stores and are left alone.
pub fn folders(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut folders = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?.path();
        if path.is_dir() {
            folders.push(path);
        }
    }
    folders.sort();
    Ok(folders)
}

/// Reads and checks one store folder.
pub fn read(folder: &Path) -> Result<OldStore, String> {
    let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let id = ContextId::parse(name)
        .map_err(|_| format!("{} is not a Context id: a Context store is contexts/<id>/", folder.display()))?;
    // Messages end without a period: the caller's sentence adds one.
    let invalid = |path: &Path, message: String| {
        format!("the store of Context {id} is invalid: {} {}", path.display(), message.trim_end_matches('.'))
    };

    let path = folder.join("context.json");
    let text = fs::read_to_string(&path).map_err(|e| invalid(&path, format!("is unreadable: {e}.")))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| invalid(&path, format!("is not valid: {e}.")))?;
    if manifest.v != VERSION {
        return Err(invalid(&path, format!("has version {}.", manifest.v)));
    }
    if manifest.id != id {
        return Err(invalid(&path, format!("names Context {}.", manifest.id)));
    }
    if manifest.title.is_empty() {
        return Err(invalid(&path, "has an empty title.".into()));
    }
    if let Some(jira) = &manifest.jira {
        jira.parse::<Jira>().map_err(|e| invalid(&path, format!("has a bad Jira key: {}", e.message())))?;
    }

    let records = read_records(folder, &invalid)?;
    Ok(OldStore { manifest, records })
}

fn read_records(folder: &Path, invalid: &dyn Fn(&Path, String) -> String) -> Result<Vec<Record>, String> {
    let dir = folder.join("records");
    let entries = fs::read_dir(&dir).map_err(|e| invalid(&dir, format!("is unreadable: {e}.")))?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| invalid(&dir, format!("is unreadable: {e}.")))?.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            files.push(path);
        }
    }
    files.sort();
    let mut records: Vec<Record> = Vec::new();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let machine = MachineName::parse(stem)
            .map_err(|_| invalid(&path, format!("is not <machine>.jsonl: {stem:?} is not a machine name.")))?;
        let text = fs::read_to_string(&path).map_err(|e| invalid(&path, format!("is unreadable: {e}.")))?;
        if !text.is_empty() && !text.ends_with('\n') {
            return Err(invalid(&path, "has no newline after its last line.".into()));
        }
        for (index, line) in text.lines().enumerate() {
            let at = |message: String| invalid(&path, format!("line {}: {message}.", index + 1));
            let record = Record::parse_line(line, &machine).map_err(at)?;
            if records.iter().any(|r| r.id == record.id) {
                return Err(at(format!("record {} appears twice", record.id)));
            }
            records.push(record);
        }
    }
    Ok(records)
}
