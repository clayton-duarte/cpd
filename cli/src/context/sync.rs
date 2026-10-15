//! `context sync` (contract §4.6, commands.md §3.6): fetch, merge, push with
//! the private remote, then import.

use std::fs;

use serde_json::{Value, json};

use super::contexts::{ContextRow, Contexts};
use super::git::Git;
use super::{io_error, lock, repo};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::home::CpdHome;
use crate::ids::ContextId;

/// The rounds of fetch, merge and push before the remote counts as busy (§4.6 step 3).
const ROUNDS: usize = 3;

/// Who runs the sync. A failure from the engine (E6) is recorded in the Context
/// and reported once; a failure run by hand only reports it (§3.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Hand,
    Engine,
}

/// What one Context's sync did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Synced {
    pub id: ContextId,
    pub synced: bool,
    /// `"no_remote"` when there was nothing to sync with.
    pub reason: Option<&'static str>,
    pub imported: usize,
    pub pushed: bool,
}

impl Synced {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"id": self.id, "synced": self.synced, "imported": self.imported, "pushed": self.pushed});
        if let Some(reason) = self.reason {
            v["reason"] = json!(reason);
        }
        v
    }
}

/// The names of the directories under `contexts/`, in name order.
pub fn store_names(home: &CpdHome) -> Result<Vec<String>, CpdError> {
    let dir = home.contexts_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error("read", &dir, &e)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error("read", &dir, &e))?;
        if entry.path().is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// The Context id a directory of `contexts/` stands for; `context_store_invalid`
/// when its name is not one.
pub fn store_id(home: &CpdHome, name: &str) -> Result<ContextId, CpdError> {
    ContextId::parse(name).map_err(|_| {
        CpdError::new(
            ErrorCode::ContextStoreInvalid,
            format!("contexts/{name} is not a Context id: a Context store is contexts/<id>/."),
        )
        .with("path", home.contexts_dir().join(name).display().to_string())
    })
}

impl Contexts<'_> {
    /// Syncs one Context under its lock (§4.6). `context_store_invalid`,
    /// `context_sync_conflict` or `context_sync_failed` on failure; with
    /// [`Trigger::Engine`] the failure is also recorded in the Context.
    pub fn sync(&mut self, id: &ContextId, trigger: Trigger) -> Result<Synced, CpdError> {
        let _lock = lock::acquire(self.home, id)?;
        let result = self.sync_locked(id);
        if let (Err(error), Trigger::Engine) = (&result, trigger) {
            // A failure to record the failure must not hide the failure.
            let _ = self.record_failure(id, error);
        }
        result
    }

    fn sync_locked(&mut self, id: &ContextId) -> Result<Synced, CpdError> {
        let manifest = repo::read_manifest(self.home, id)?;
        let dir = self.home.context_dir(id);
        let git = Git::new(&dir, &self.machine);
        let remote = git.probe(&["remote", "get-url", "origin"]);
        let has_remote = match remote {
            Ok(url) => url.is_some(),
            // `git remote get-url` exits 2 for an unknown remote.
            Err(e) if e.details().get("exit") == Some(&json!(2)) => false,
            Err(e) => return Err(e),
        };
        let (pushed, reason) = if has_remote {
            (exchange(&git, id)?, None)
        } else {
            (false, Some("no_remote"))
        };
        let imported = self.import_locked(id, &manifest, "context sync", has_remote)?;
        Ok(Synced { id: id.clone(), synced: has_remote, reason, imported, pushed })
    }

    /// Sets `sync_failing` and appends one `error` Event, only when it was not
    /// set (§4.6). A Context with no row yet has nowhere to record it.
    fn record_failure(&mut self, id: &ContextId, error: &CpdError) -> Result<(), CpdError> {
        self.store.write("context sync", |tx| {
            if ContextRow::find(tx, id)?.is_none() {
                return Ok(());
            }
            let changed = tx.execute("UPDATE contexts SET sync_failing = 1 WHERE id = ?1 AND sync_failing = 0", [id.as_str()])?;
            if changed == 1 {
                let data = json!({
                    "source": "context sync", "code": error.code(), "message": error.message(), "details": error.details(),
                });
                events::append(
                    tx,
                    NewEvent::new(
                        Kind::Error,
                        format!("Context {id} failed to sync: {} Next: cpd2 context sync --context {id}", error.message()),
                        data,
                    )
                    .context(id.clone()),
                )?;
            }
            Ok(())
        })
    }
}

/// Fetch, merge and push, at most [`ROUNDS`] times (§4.6 steps 2–3). Returns
/// whether anything was pushed. A push that fails while the remote has not
/// moved is not "rejected because the remote moved": it fails at once.
fn exchange(git: &Git<'_>, id: &ContextId) -> Result<bool, CpdError> {
    let sync_failed = |e: CpdError| {
        CpdError::new(ErrorCode::ContextSyncFailed, format!("Context {id} did not sync: {}", e.message()))
            .with("context", id.as_str())
            .with("argv", e.details().get("argv").cloned().unwrap_or(Value::Null))
            .with("exit", e.details().get("exit").cloned().unwrap_or(Value::Null))
            .with("stderr", e.details().get("stderr").cloned().unwrap_or(Value::Null))
    };
    let mut previous: Option<(Option<String>, CpdError)> = None;
    for _ in 0..ROUNDS {
        git.run(&["fetch", "-q", "origin"]).map_err(&sync_failed)?;
        let tip = git
            .probe(&["rev-parse", "--verify", "-q", "refs/remotes/origin/main"])
            .map_err(&sync_failed)?
            .map(|s| s.trim().to_owned());
        if let Some((moved_from, failure)) = previous.take()
            && moved_from == tip
        {
            return Err(failure);
        }
        if tip.is_some() {
            merge(git, id).map_err(|e| if e.code() == ErrorCode::ContextSyncConflict { e } else { sync_failed(e) })?;
        }
        let head = git.run(&["rev-parse", "HEAD"]).map_err(&sync_failed)?;
        if tip.as_deref() == Some(head.trim()) {
            return Ok(false);
        }
        match git.run(&["push", "-q", "origin", "main"]) {
            Ok(_) => return Ok(true),
            Err(e) => previous = Some((tip, sync_failed(e))),
        }
    }
    Err(previous.map(|(_, failure)| failure).expect("a round ran"))
}

/// `git merge --no-edit origin/main`; a conflict is aborted, leaving nothing
/// changed, and named (§4.6 step 2).
///
/// `--allow-unrelated-histories`: two machines that created the same Context
/// id have unrelated histories, and §4.6 says that case is a conflict in
/// `context.json` for the User to resolve, not a merge git refuses to try.
fn merge(git: &Git<'_>, id: &ContextId) -> Result<(), CpdError> {
    let args = ["merge", "--no-edit", "--allow-unrelated-histories", "origin/main"];
    let output = git.attempt(&args)?;
    if output.status.success() {
        return Ok(());
    }
    let files: Vec<String> = git
        .run(&["diff", "--name-only", "--diff-filter=U"])?
        .lines()
        .map(str::to_owned)
        .collect();
    if files.is_empty() {
        return Err(git.failure(&args, &output));
    }
    git.run(&["merge", "--abort"])?;
    Err(CpdError::new(
        ErrorCode::ContextSyncConflict,
        format!("Merging origin/main into Context {id} conflicts in {}: resolve it by hand.", files.join(", ")),
    )
    .with("context", id.as_str())
    .with("files", files))
}
