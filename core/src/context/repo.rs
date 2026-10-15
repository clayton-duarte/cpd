//! A Context's git repository on disk (contract §4.6): `context.json`, the
//! record files, and reading them back. Every file is parsed here, once; what
//! leaves this module is typed.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

use super::git::Git;
use super::record::{Record, VERSION};
use super::{Jira, io_error};
use crate::args::Text;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, MachineName};

/// `context.json`: written once, by `context new`.
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

const GITATTRIBUTES: &str = "records/*.jsonl merge=union\n";
const GITIGNORE: &str = "*.tmp\n";

/// `records/<machine>.jsonl` of this Context.
pub fn records_file(home: &CpdHome, context: &ContextId, machine: &MachineName) -> PathBuf {
    home.context_dir(context).join("records").join(format!("{machine}.jsonl"))
}

fn invalid(context: &ContextId, path: &Path, message: impl Into<String>) -> CpdError {
    CpdError::new(
        ErrorCode::ContextStoreInvalid,
        format!("The store of Context {context} is invalid: {}", message.into()),
    )
    .with("context", context.as_str())
    .with("path", path.display().to_string())
}

/// Creates the store: the repository, its files and its first commit. The
/// directory must not exist; it is removed again when anything fails.
pub fn create(
    home: &CpdHome,
    machine: &MachineName,
    id: &ContextId,
    title: &Text,
    jira: Option<&Jira>,
    created_at: &str,
) -> Result<(), CpdError> {
    let dir = home.context_dir(id);
    let build = || -> Result<(), CpdError> {
        let init = |e| io_error("create", &dir, &e);
        fs::create_dir_all(dir.parent().expect("a Context directory has a parent")).map_err(init)?;
        let git = Git::new(dir.parent().expect("has a parent"), machine);
        git.run(&["init", "-q", "-b", "main", &dir.display().to_string()])?;
        let git = Git::new(&dir, machine);
        let manifest = json!({
            "v": VERSION, "id": id, "title": title.as_str(), "jira": jira.map(Jira::as_str),
            "created_at": created_at, "created_on": machine,
        });
        let write = |name: &str, content: String| {
            let path = dir.join(name);
            fs::write(&path, content).map_err(|e| io_error("write", &path, &e))
        };
        write("context.json", format!("{manifest}\n"))?;
        write(".gitattributes", GITATTRIBUTES.to_owned())?;
        write(".gitignore", GITIGNORE.to_owned())?;
        let records = dir.join("records");
        fs::create_dir(&records).map_err(|e| io_error("create", &records, &e))?;
        write(&format!("records/{machine}.jsonl"), String::new())?;
        git.run(&["add", "-A"])?;
        git.commit(&format!("context {id}: {title}"))
    };
    build().inspect_err(|_| {
        let _ = fs::remove_dir_all(&dir);
    })
}

/// Reads and checks the store's `context.json`. `context_not_found` when there
/// is no store; `context_store_invalid` when it is not a git repository of its
/// own or its manifest is wrong.
pub fn read_manifest(home: &CpdHome, id: &ContextId) -> Result<Manifest, CpdError> {
    let dir = home.context_dir(id);
    if !dir.is_dir() {
        return Err(CpdError::new(ErrorCode::ContextNotFound, format!("No Context store for {id}."))
            .with("context", id.as_str())
            .with("path", dir.display().to_string()));
    }
    // `.git` of its own: a bare `git -C` would find a repository above $CPD_HOME.
    if !dir.join(".git").exists() {
        return Err(invalid(id, &dir, "it is not a git repository."));
    }
    let path = dir.join("context.json");
    let text = fs::read_to_string(&path).map_err(|e| invalid(id, &path, format!("context.json is unreadable: {e}.")))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| invalid(id, &path, format!("context.json: {e}.")))?;
    if manifest.v != VERSION {
        return Err(invalid(id, &path, format!("context.json has version {}.", manifest.v)));
    }
    if manifest.id != *id {
        return Err(invalid(id, &path, format!("context.json names Context {}.", manifest.id)));
    }
    if manifest.title.is_empty() {
        return Err(invalid(id, &path, "context.json has an empty title."));
    }
    if let Some(jira) = &manifest.jira {
        jira.parse::<Jira>().map_err(|e| invalid(id, &path, format!("context.json: {}", e.message())))?;
    }
    Ok(manifest)
}

/// Every record of every `records/*.jsonl`, files in name order and lines in
/// file order. One bad line, file name or duplicate id is `context_store_invalid`
/// naming the file and line.
pub fn read_records(home: &CpdHome, id: &ContextId) -> Result<Vec<Record>, CpdError> {
    let dir = home.context_dir(id).join("records");
    let entries = fs::read_dir(&dir).map_err(|e| invalid(id, &dir, format!("records/ is unreadable: {e}.")))?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| io_error("read", &dir, &e))?.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            files.push(path);
        }
    }
    files.sort();
    let mut records: Vec<Record> = Vec::new();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let machine = MachineName::parse(stem)
            .map_err(|_| invalid(id, &path, format!("{stem:?} is not a machine name; records/ holds <machine>.jsonl.")))?;
        let text = fs::read_to_string(&path).map_err(|e| invalid(id, &path, format!("unreadable: {e}.")))?;
        if !text.is_empty() && !text.ends_with('\n') {
            return Err(invalid(id, &path, "the last line has no newline."));
        }
        for (index, line) in text.lines().enumerate() {
            let at = |message: String| invalid(id, &path, format!("line {}: {message}.", index + 1)).with("line", index + 1);
            let record = Record::parse_line(line, &machine).map_err(&at)?;
            if records.iter().any(|r| r.id == record.id) {
                return Err(at(format!("record {} appears twice", record.id)));
            }
            records.push(record);
        }
    }
    Ok(records)
}

/// Appends `record` to this machine's file and returns the file's length
/// before, for [`rewind`].
pub fn append(home: &CpdHome, context: &ContextId, machine: &MachineName, record: &Record) -> Result<u64, CpdError> {
    let path = records_file(home, context, machine);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| io_error("open", &path, &e))?;
    let before = file.metadata().map_err(|e| io_error("stat", &path, &e))?.len();
    file.write_all(record.to_line().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| io_error("append to", &path, &e))?;
    Ok(before)
}

/// Cuts the file back to `length`: the append whose commit failed.
pub fn rewind(home: &CpdHome, context: &ContextId, machine: &MachineName, length: u64) {
    let path = records_file(home, context, machine);
    if let Ok(file) = OpenOptions::new().write(true).open(path) {
        let _ = file.set_len(length);
    }
}
