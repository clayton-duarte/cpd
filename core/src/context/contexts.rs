//! The Context writes (contract §4.6): creating a Context and writing a
//! record, each one database transaction.

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::Jira;
use super::distill::State;
use super::prose_supersedes::prose_supersession_targets;
use super::record::{Draft, Record, may_supersede, superseding};
use crate::args::Text;
use crate::clock::now;
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::home::CpdHome;
use crate::ids::{ContextId, MachineName, OwnerLogin, RecordId, RecordKind, SessionId};
use crate::policy::{PolicyKey, resolve};
use crate::store::Store;

/// A local lease also wakes same-process waiters when the file lock is released.
struct LocalLease {
    path: std::path::PathBuf,
}

fn local_registry() -> &'static (
    std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
    std::sync::Condvar,
) {
    static REGISTRY: std::sync::OnceLock<(
        std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
        std::sync::Condvar,
    )> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        (
            std::sync::Mutex::new(std::collections::HashSet::new()),
            std::sync::Condvar::new(),
        )
    })
}

impl Drop for LocalLease {
    fn drop(&mut self) {
        let (mutex, wake) = local_registry();
        let mut held = mutex.lock().unwrap_or_else(|e| e.into_inner());
        held.remove(&self.path);
        wake.notify_all();
    }
}

/// File drops before the local lease wakes its waiters.
pub(crate) struct CreationLock {
    _file: File,
    _local: LocalLease,
}

/// Serialize one creation and its external provisioning without holding a store transaction.
/// Lock files are persistent: unlinking a held inode would allow a second holder.
pub(crate) fn creation_lock(
    home: &CpdHome,
    kind: &str,
    id: &str,
) -> Result<CreationLock, CpdError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    creation_lock_with_remaining(home, kind, id, || {
        deadline.saturating_duration_since(std::time::Instant::now())
    })
}

fn creation_lock_with_remaining(
    home: &CpdHome,
    kind: &str,
    id: &str,
    remaining: impl Fn() -> Duration,
) -> Result<CreationLock, CpdError> {
    let dir = home.locks_dir();
    let path = dir.join(format!("{kind}-new-{id}.lock"));
    let failed = |e: &dyn std::fmt::Display| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot use creation lock {}: {e}.", path.display()),
        )
        .with("lock", path.display().to_string())
    };
    let timed_out = || {
        CpdError::new(
            ErrorCode::LockTimeout,
            format!(
                "Another creation held {} for 60 seconds. Retry with the same flags.",
                path.display()
            ),
        )
        .with("lock", path.display().to_string())
        .with("waited_seconds", 60)
    };
    fs::create_dir_all(&dir).map_err(|e| failed(&e))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| failed(&e))?;
    if !file.metadata().map_err(|e| failed(&e))?.is_file() {
        return Err(failed(&"not a regular file"));
    }
    let (mutex, wake) = local_registry();
    let mut held = mutex.lock().unwrap_or_else(|e| e.into_inner());
    while held.contains(&path) {
        let remaining = remaining();
        if remaining.is_zero() {
            return Err(timed_out());
        }
        held = wake
            .wait_timeout(held, remaining)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    held.insert(path.clone());
    drop(held);
    let local = LocalLease { path: path.clone() };
    loop {
        if crate::lock::try_lock(&file).map_err(|e| failed(&e))? {
            return Ok(CreationLock {
                _file: file,
                _local: local,
            });
        }
        let remaining = remaining();
        if remaining.is_zero() {
            return Err(timed_out());
        }
        let held = mutex.lock().unwrap_or_else(|e| e.into_inner());
        drop(
            wake.wait_timeout(held, remaining.min(Duration::from_millis(100)))
                .unwrap_or_else(|e| e.into_inner()),
        );
    }
}

/// A row of `contexts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRow {
    pub id: String,
    pub title: String,
    pub jira: Option<String>,
    pub created_at: String,
    pub created_on: String,
    /// When it was archived (contract §2.2 "Archived"), else none.
    pub archived_at: Option<String>,
    /// The GitHub owner, the policy key (contract §8.3); none only for a Context made before the policy keys.
    pub owner: Option<String>,
}

impl ContextRow {
    pub const COLUMNS: &'static str = "id, title, jira, created_at, created_on, archived_at, owner";

    pub fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ContextRow> {
        Ok(ContextRow {
            id: r.get(0)?,
            title: r.get(1)?,
            jira: r.get(2)?,
            created_at: r.get(3)?,
            created_on: r.get(4)?,
            archived_at: r.get(5)?,
            owner: r.get(6)?,
        })
    }

    pub fn find(conn: &Connection, id: &ContextId) -> rusqlite::Result<Option<ContextRow>> {
        conn.query_row(
            &format!(
                "SELECT {} FROM contexts WHERE id = ?1 AND deleted_at IS NULL",
                ContextRow::COLUMNS
            ),
            [id.as_str()],
            ContextRow::from_row,
        )
        .optional()
    }

    /// `context_not_found` when there is no such Context.
    pub fn require(conn: &Connection, id: &ContextId) -> Result<ContextRow, CpdError> {
        ContextRow::find(conn, id)?.ok_or_else(|| {
            CpdError::new(ErrorCode::ContextNotFound, format!("No Context {id}."))
                .with("context", id.as_str())
        })
    }
}

/// What [`Contexts::write`] did.
#[derive(Clone, Debug)]
pub struct Written {
    pub record: Record,
    /// The record's index position.
    pub seq: i64,
    /// A retry of this machine's latest record: nothing was written.
    pub already: bool,
}

/// The Contexts of one machine: their rows and records in the database, the
/// only place they live (contract §4.6). Every write is one transaction.
pub struct Contexts<'a> {
    machine: MachineName,
    config: Config,
    store: &'a mut Store,
}

/// What a record write finds: this machine's latest record again (a retry), or
/// the number its new record takes.
// The variant carries the one value this enum exists to hand back; boxing is a type change out of scope.
#[allow(clippy::large_enum_variant)]
enum Next {
    Retry(i64, Record),
    Number(u32),
}

impl<'a> Contexts<'a> {
    pub fn new(config: &Config, store: &'a mut Store) -> Contexts<'a> {
        Contexts {
            machine: config.machine.clone(),
            config: config.clone(),
            store,
        }
    }

    /// `context new` (commands.md §3.1): the Context's row and `context-created`.
    /// Returns the row, whether it already existed, and the provisioned Manager.
    /// The owner's policy file must resolve (`owner_unconfirmed`, `policy_invalid`).
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &mut self,
        home: &CpdHome,
        id: &ContextId,
        title: &Text,
        jira: Option<&Jira>,
        owner: &OwnerLogin,
        confirm_fallback: bool,
        by_user: bool,
    ) -> Result<(ContextRow, bool, Value), CpdError> {
        let _creation_lock = creation_lock(home, "context", id.as_str())?;
        let existing = |conn: &Connection| -> Result<Option<ContextRow>, CpdError> {
            // A deleted Context keeps its row, and so its id: never reused (contract §2.2 "Deleted").
            let deleted: bool = conn
                .query_row(
                    "SELECT deleted_at IS NOT NULL FROM contexts WHERE id = ?1",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(false);
            if deleted {
                return Err(CpdError::new(
                    ErrorCode::ContextExists,
                    format!("Context {id} was deleted: its id is not reused."),
                )
                .with("context", id.as_str()));
            }
            let Some(row) = ContextRow::find(conn, id)? else {
                return Ok(None);
            };
            if row.title == title.as_str()
                && row.jira.as_deref() == jira.map(Jira::as_str)
                && row.owner.as_deref() == Some(owner.as_str())
            {
                return Ok(Some(row));
            }
            Err(CpdError::new(
                ErrorCode::ContextExists,
                format!("Context {id} exists with another title, Jira key or owner."),
            )
            .with("context", id.as_str())
            .with("title", row.title)
            .with("jira", row.jira)
            .with("owner", row.owner))
        };
        let found = self.store.read(|conn| existing(conn))?;
        let manager = self.store.read(|conn| super::manager::live(conn, id))?;
        if found.is_none() {
            let link = match resolve(home, &PolicyKey::Owner(owner.clone())) {
                Err(e) if e.code() == ErrorCode::OwnerUnconfirmed && confirm_fallback => true,
                other => other.map(|_| false)?,
            };
            if link {
                crate::policy::confirm_fallback(home, owner)?;
            }
        }
        let prepared = if manager.is_none() {
            Some(super::manager::prepare_pair(&self.config, id)?)
        } else {
            None
        };
        let machine = self.machine.clone();
        let result = self.store.write("context new", |tx| {
            // Another process may have created it since the read.
            if let Some(row) = existing(tx)? {
                let registered = if super::manager::live(tx, id)?.is_none() {
                    let prepared = prepared.as_ref().ok_or_else(|| CpdError::bad_row(format!("Manager of Context {id} disappeared during creation")))?;
                    super::manager::insert(tx, id, prepared, by_user, self.config.manager_runner)?;
                    true
                } else {
                    let stored: bool = tx.query_row(
                        "SELECT by_user FROM sessions WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL",
                        [id.as_str()],
                        |r| r.get(0),
                    )?;
                    if stored != by_user {
                        return Err(CpdError::new(ErrorCode::ContextExists, format!("Context {id} exists with a different Manager by-user mode."))
                            .with("context", id.as_str())
                            .with("by_user", stored));
                    }
                    false
                };
                return Ok((row, true, registered));
            }
            let taken: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM plans WHERE id = ?1)",
                [id.as_str()],
                |r| r.get(0),
            )?;
            if taken {
                return Err(CpdError::new(
                    ErrorCode::IdTaken,
                    format!("Context id {id} is taken by a Plan."),
                )
                .with("context", id.as_str()));
            }
            tx.execute(
                "INSERT INTO contexts (id, title, jira, created_at, created_on, owner) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id.as_str(), title.as_str(), jira.map(Jira::as_str), now(), machine.as_str(), owner.as_str()],
            )?;
            events::append(
                tx,
                NewEvent::context_created(
                    format!("Context {id} \"{title}\" exists. Next: cpd context show --context {id}", title = title.as_str()),
                    title.as_str(),
                    jira.map(Jira::as_str),
                    machine.as_str(),
                )
                .context(id.clone()),
            )?;
            let prepared = prepared.as_ref().ok_or_else(|| CpdError::bad_row(format!("Manager exists without Context {id}")))?;
            super::manager::insert(tx, id, prepared, by_user, self.config.manager_runner)?;
            Ok((ContextRow::require(tx, id)?, false, true))
        })?;
        let mut provision = || -> Result<Value, CpdError> {
            let by_user: bool = self.store.read(|conn| {
                Ok(conn.query_row(
                    "SELECT by_user FROM sessions WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL",
                    [id.as_str()],
                    |r| r.get(0),
                )?)
            })?;
            if !by_user && (result.2 || !home.manager_message(id).is_file()) {
                let topics = self
                    .store
                    .read(|conn| super::curated::topic_counts(conn, id))?;
                super::manager::write_first_message(home, id, &result.0.title, &topics)?;
            }
            let crate::output::Output::Success(mut output) = super::manager::start_for_new(id)?
            else {
                return Err(CpdError::bad_row("Manager provisioning returned no output"));
            };
            output
                .remove("manager")
                .ok_or_else(|| CpdError::bad_row("Manager provisioning returned no Manager"))
        };
        let manager = provision().map_err(|e| {
            let message = format!("Context {id} and its Manager are registered, but Manager provisioning failed: {}. Retry context new with the same flags.", e.message());
            CpdError::new(e.code(), message).with("context", id.as_str()).with("cause", e.to_json())
        })?;
        Ok((result.0, result.1, manager))
    }

    /// Writes one record (§4.6 "A write"): in one transaction, validates it
    /// (`record_not_found`), inserts the row and appends
    /// `context-record`. `command` is the transaction's `txns.command`. A retry
    /// of this machine's latest record writes nothing.
    pub fn write(
        &mut self,
        context: &ContextId,
        command: &str,
        by: super::record::By,
        text: &Text,
        draft: Draft,
    ) -> Result<Written, CpdError> {
        self.write_confirmed(context, command, by, text, draft, false)
    }

    /// [`Contexts::write`]; a decision or note whose text names an in-force record after `supersedes`, `overrules` or
    /// `replaces` that `draft` does not list is refused (`supersedes_unlinked`), unless `no_supersedes`.
    pub fn write_confirmed(
        &mut self,
        context: &ContextId,
        command: &str,
        by: super::record::By,
        text: &Text,
        draft: Draft,
        no_supersedes: bool,
    ) -> Result<Written, CpdError> {
        let machine = self.machine.clone();
        let next = |conn: &Connection| -> Result<Next, CpdError> {
            ContextRow::require(conn, context)?;
            let latest = conn
                .query_row(
                    &format!(
                        "SELECT {} FROM context_records WHERE context = ?1 AND machine = ?2 ORDER BY seq DESC LIMIT 1",
                        Record::COLUMNS
                    ),
                    params![context.as_str(), machine.as_str()],
                    Record::from_row,
                )
                .optional()?;
            if let Some((seq, record)) = latest.filter(|(_, r)| {
                r.draft == draft && r.text == text.as_str() && r.by.session == by.session
            }) {
                return Ok(Next::Retry(seq, record));
            }
            if let Some((seq, record)) = in_force_duplicate(conn, context, &draft, text.as_str())? {
                return Ok(Next::Retry(seq, record));
            }
            check_links(conn, context, &draft)?;
            if !no_supersedes {
                check_prose(conn, context, &draft, text.as_str())?;
            }
            let mut stmt = conn.prepare(
                "SELECT id FROM context_records WHERE context = ?1 AND machine = ?2 AND kind = ?3",
            )?;
            let ids = stmt.query_map(
                params![context.as_str(), machine.as_str(), draft.kind().as_str()],
                |r| r.get::<_, String>(0),
            )?;
            let mut highest = 0;
            for id in ids {
                highest = highest.max(RecordId::parse(&id?)?.number().get());
            }
            Ok(Next::Number(highest + 1))
        };
        if let Next::Retry(seq, record) = self.store.read(|conn| next(conn))? {
            return Ok(Written {
                record,
                seq,
                already: true,
            });
        }
        let written = self.store.write(command, |tx| {
            // The number is taken again inside the write transaction: only one writer at a time.
            let number = match next(tx)? {
                Next::Retry(seq, record) => {
                    return Ok(Written {
                        record,
                        seq,
                        already: true,
                    });
                }
                Next::Number(number) => number,
            };
            let record = Record {
                id: RecordId::new(
                    machine.clone(),
                    draft.kind(),
                    std::num::NonZeroU32::new(number).expect("one more than a count"),
                ),
                at: now(),
                by: by.clone(),
                text: text.as_str().to_owned(),
                draft: draft.clone(),
            };
            record.insert(tx, context)?;
            let seq = tx.last_insert_rowid();
            events::append(tx, record_event(context, &record, seq, "recorded"))?;
            Ok(Written {
                record,
                seq,
                already: false,
            })
        })?;
        // Best-effort telemetry: a failed mark never fails the write.
        if let (false, Some(session)) = (written.already, &by.session) {
            let mut exclude = vec![written.record.id.clone()];
            exclude.extend(draft.supersedes().iter().cloned());
            let _ =
                super::reads::mark_used_for(self.store, context, session, text.as_str(), &exclude);
        }
        Ok(written)
    }

    /// The Session of the environment (`CPD_SESSION`, else `PI_SESSION_ID`), when it is a known Session.
    pub(crate) fn env_session(store: &mut Store) -> Option<SessionId> {
        super::reads::env_session(store)
    }

    /// Best-effort: mark the records `text` cites that `session` read in `context` as used.
    pub(crate) fn mark_cited(
        store: &mut Store,
        context: &ContextId,
        session: &SessionId,
        text: &str,
    ) {
        let _ = super::reads::mark_used_for(store, context, session, text, &[]);
    }
}

/// An in-force record of the same kind as `draft` with the same trimmed text and the same supersedes list (§4.6 "A write").
fn in_force_duplicate(
    conn: &Connection,
    context: &ContextId,
    draft: &Draft,
    text: &str,
) -> Result<Option<(i64, Record)>, CpdError> {
    if !matches!(draft.kind(), RecordKind::Decision | RecordKind::Note) {
        return Ok(None);
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq",
        Record::COLUMNS
    ))?;
    let records = stmt
        .query_map([context.as_str()], Record::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let Some(highest) = records.last().map(|(seq, _)| *seq) else {
        return Ok(None);
    };
    let retired = super::inforce::retired(conn, context)?;
    let found = super::inforce::in_force_without(&records, highest, &retired)
        .into_iter()
        .find(|(_, r)| {
            r.kind() == draft.kind()
                && r.text.trim() == text.trim()
                && r.supersedes() == draft.supersedes()
        })
        .cloned();
    Ok(found)
}

/// §4.6 "A write": `supersedes` names an existing record of a kind the writing
/// record may supersede (`supersedes_invalid` otherwise).
fn check_links(conn: &Connection, context: &ContextId, draft: &Draft) -> Result<(), CpdError> {
    let mut seen = Vec::new();
    for id in draft.supersedes() {
        if seen.contains(&id) {
            return Err(CpdError::new(
                ErrorCode::SupersedesInvalid,
                format!("{id} is named twice in --supersedes."),
            )
            .with("record", id.to_string()));
        }
        seen.push(id);
        let found: Option<String> = conn
            .query_row(
                "SELECT kind FROM context_records WHERE context = ?1 AND id = ?2",
                params![context.as_str(), id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(found) = found else {
            return Err(CpdError::new(
                ErrorCode::RecordNotFound,
                format!("Context {context} has no record {id}."),
            )
            .with("context", context.as_str())
            .with("record", id.to_string()));
        };
        let writer = draft.kind();
        if !superseding(writer).iter().any(|k| k.as_str() == found) {
            let allowed: Vec<&str> = superseding(writer).iter().map(|k| k.as_str()).collect();
            return Err(CpdError::new(
                ErrorCode::SupersedesInvalid,
                format!(
                    "A {} cannot supersede {id}, a {found}: it may supersede a {}.",
                    writer.as_str(),
                    allowed.join(" or a ")
                ),
            )
            .with("record", id.to_string())
            .with("kind", found)
            .with("allowed", allowed));
        }
    }
    Ok(())
}

/// The in-force records of the Context that the text of a decision or note names after a supersession verb, that the
/// draft may supersede and does not list: `supersedes_unlinked`.
fn check_prose(
    conn: &Connection,
    context: &ContextId,
    draft: &Draft,
    text: &str,
) -> Result<(), CpdError> {
    let writer = draft.kind();
    if superseding(writer).is_empty() {
        return Ok(());
    }
    let named = prose_supersession_targets(text);
    if named.is_empty() {
        return Ok(());
    }
    let state = State::read(conn, context)?;
    let in_force = state.in_force_ids();
    let unlisted: Vec<String> = named
        .iter()
        .filter(|id| {
            !draft.supersedes().contains(id)
                && may_supersede(writer, id.kind())
                && in_force.contains(&id.to_string())
        })
        .map(ToString::to_string)
        .collect();
    if unlisted.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::SupersedesUnlinked,
        format!(
            "The text names {} after a supersession verb, but --supersedes does not list it. Add --supersedes, or --no-supersedes to confirm the text is no supersession.",
            unlisted.join(", ")
        ),
    )
    .with("ids", unlisted))
}

/// The `context-record` Event of a record (§5.2).
fn record_event(context: &ContextId, record: &Record, seq: i64, verb: &str) -> NewEvent {
    let snippet: String = record
        .text
        .chars()
        .take(80)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    NewEvent::context_record(
        format!(
            "{} {} {verb} in Context {context}: {snippet}. Next: cpd context show --context {context} --since {}",
            record.kind().as_str(),
            record.id,
            seq - 1
        ),
        &record.id,
        record.kind().as_str(),
        record.machine(),
        seq,
    )
    .context(context.clone())
}

#[cfg(test)]
mod creation_lock_tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn expired_lock(home: &CpdHome, kind: &str, id: &str) -> Result<CreationLock, CpdError> {
        creation_lock_with_remaining(home, kind, id, || Duration::ZERO)
    }

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
    }

    #[test]
    fn local_and_file_contention_use_the_same_deadline_and_release() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let home = home(&dir);
        let held = expired_lock(&home, "context", "ctx").unwrap();
        let error = match expired_lock(&home, "context", "ctx") {
            Err(error) => error,
            Ok(_) => panic!("local contention must time out"),
        };
        assert_eq!(error.code(), ErrorCode::LockTimeout);
        let unrelated = expired_lock(&home, "context", "other").unwrap();
        drop(unrelated);
        drop(held);
        let path = home.locks_dir().join("context-new-ctx.lock");
        let external = OpenOptions::new().write(true).open(path).unwrap();
        assert!(crate::lock::try_lock(&external).unwrap());
        let error = match expired_lock(&home, "context", "ctx") {
            Err(error) => error,
            Ok(_) => panic!("file contention must time out"),
        };
        assert_eq!(error.code(), ErrorCode::LockTimeout);
        assert!(error.message().contains("Retry"));
        drop(external);
        drop(expired_lock(&home, "context", "ctx").unwrap());
    }

    #[test]
    fn panic_releases_both_creation_guards() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let home = home(&dir);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _held = expired_lock(&home, "plan", "fresh").unwrap();
            panic!("injected failure");
        }));
        assert!(result.is_err());
        drop(expired_lock(&home, "plan", "fresh").unwrap());
    }

    #[test]
    fn lock_file_symlink_is_refused_without_leaving_a_lease() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let home = home(&dir);
        fs::create_dir_all(home.locks_dir()).unwrap();
        let path = home.locks_dir().join("context-new-ctx.lock");
        let target = dir.path().join("target");
        fs::write(&target, "unchanged").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(expired_lock(&home, "context", "ctx").is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "unchanged");
        fs::remove_file(path).unwrap();
        drop(expired_lock(&home, "context", "ctx").unwrap());
    }
}
