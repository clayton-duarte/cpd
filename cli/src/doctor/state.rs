//! What the machine holds in `$CPD_HOME` (commands.md §14.1): the store, the
//! tamper report, the Context stores and the engine.

use std::fs::File;
use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};

use super::check::{Check, Severity};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::ContextId;
use crate::store::Store;

const NO_STORE: &str = "the store did not open.";

/// The database opens, migrates and passes the schema check. A store that
/// opened is returned for the checks that read it, even when the schema check
/// failed: reads still work (contract §4.3).
pub fn store(opened: Result<Store, CpdError>) -> (Check, Option<Store>) {
    match opened {
        Err(e) => (Check::from_error("store", Severity::Error, &e), None),
        Ok(store) => {
            let check = match store.schema_mismatch() {
                None => Check::pass("store", Severity::Error),
                Some(m) => Check::fail("store", Severity::Error, "The schema is not this cpd2's: a table or trigger was changed outside cpd.")
                    .found(m.found.clone())
                    .expected(m.expected.clone()),
            };
            (check, Some(store))
        }
    }
}

/// The tamper report (contract §4.3) is empty; `found` lists it.
pub fn tamper(store: Option<&mut Store>) -> Check {
    let name = "tamper";
    let Some(store) = store else { return Check::not_checked(name, Severity::Error, NO_STORE) };
    match store.tamper() {
        Err(e) => Check::from_error(name, Severity::Error, &e),
        Ok(report) if report.is_empty() => Check::pass(name, Severity::Error),
        Ok(report) => Check::fail(name, Severity::Error, "The Ledger was changed outside cpd.").found(Value::Array(report)),
    }
}

/// Every directory under `contexts/` is a valid store and none is `sync_failing`.
pub fn contexts(home: &CpdHome, store: Option<&mut Store>) -> Check {
    let name = "contexts";
    let Some(store) = store else { return Check::not_checked(name, Severity::Warn, NO_STORE) };
    let mut problems = match context_store_problems(&home.contexts_dir()) {
        Ok(problems) => problems,
        Err(message) => return Check::fail(name, Severity::Warn, message),
    };
    let failing = store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM contexts WHERE sync_failing = 1 ORDER BY id")?;
        Ok(stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    });
    match failing {
        Ok(ids) => problems.extend(ids.into_iter().map(|id| json!({"context": id, "problem": "sync_failing"}))),
        Err(e) => return Check::from_error(name, Severity::Warn, &e),
    }
    if problems.is_empty() {
        return Check::pass(name, Severity::Warn);
    }
    Check::fail(name, Severity::Warn, format!("{} Context problem(s): see found.", problems.len())).found(Value::Array(problems))
}

/// One `{context, problem}` per directory of `contexts/` that is not a valid
/// store (contract §4.6): a git repository whose `context.json` is `v` 1 with
/// this directory's id and a title. No `contexts/` is no Contexts.
fn context_store_problems(dir: &Path) -> Result<Vec<Value>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}.", dir.display())),
    };
    let mut names: Vec<_> = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot read {}: {e}.", dir.display()))?
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names
        .into_iter()
        .filter_map(|name| context_store_problem(&dir.join(&name), &name).map(|p| json!({"context": name, "problem": p})))
        .collect())
}

fn context_store_problem(path: &Path, name: &str) -> Option<String> {
    if ContextId::parse(name).is_err() {
        return Some("the name is not a Context id".into());
    }
    if !path.is_dir() {
        return Some("not a directory".into());
    }
    if !path.join(".git").exists() {
        return Some("not a git repository".into());
    }
    let text = match std::fs::read_to_string(path.join("context.json")) {
        Ok(text) => text,
        Err(e) => return Some(format!("context.json is unreadable: {e}")),
    };
    let json: Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(e) => return Some(format!("context.json is not JSON: {e}")),
    };
    let valid = json["v"] == 1 && json["id"] == name && json["title"].as_str().is_some_and(|t| !t.is_empty());
    (!valid).then(|| "context.json is not {\"v\": 1, \"id\": <this directory>, \"title\": …}".into())
}

/// The engine runs (it holds `locks/watch.lock`) when any Plan is in `dispatch` or `pr`.
pub fn watch(home: &CpdHome, store: Option<&mut Store>) -> Check {
    let name = "watch";
    let Some(store) = store else { return Check::not_checked(name, Severity::Warn, NO_STORE) };
    let plans = store.read(|conn| {
        Ok(conn.query_row("SELECT count(*) FROM plans WHERE phase IN ('dispatch', 'pr')", [], |r| r.get::<_, i64>(0))?)
    });
    let plans = match plans {
        Ok(plans) => plans,
        Err(e) => return Check::from_error(name, Severity::Warn, &e),
    };
    if plans == 0 {
        return Check::pass(name, Severity::Warn).message("No Plan is in dispatch or pr.");
    }
    match engine_runs(&home.watch_lock()) {
        Ok(true) => Check::pass(name, Severity::Warn).found(format!("{plans} Plan(s) in dispatch or pr; the engine runs")),
        Ok(false) => Check::fail(name, Severity::Warn, format!("{plans} Plan(s) are in dispatch or pr and no engine runs: start it with `cpd2 watch --ensure`.")),
        Err(message) => Check::fail(name, Severity::Warn, message),
    }
}

/// Whether another process holds `lock` (the engine's flock, contract §6.6).
/// Takes nothing: a missing file is no engine, and the probe's shared lock is
/// let go at once.
fn engine_runs(lock: &Path) -> Result<bool, String> {
    let file = match File::open(lock) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("cannot open {}: {e}.", lock.display())),
    };
    match Flock::lock(file, FlockArg::LockSharedNonblock) {
        Ok(_free) => Ok(false),
        Err((_, Errno::EWOULDBLOCK)) => Ok(true),
        Err((_, e)) => Err(format!("cannot probe {}: {e}.", lock.display())),
    }
}
