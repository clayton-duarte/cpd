//! The Context store's writes (contract §4.6): creating a Context, appending
//! a record, and importing record files into the index.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use super::record::{Draft, Record};
use super::repo::{self, Manifest};
use super::{Jira, lock, now};
use crate::args::Text;
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::home::CpdHome;
use crate::ids::{ContextId, MachineName, RecordId, RecordKind};
use crate::store::Store;

use super::git::Git;

/// A row of `contexts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRow {
    pub id: String,
    pub title: String,
    pub jira: Option<String>,
    pub created_at: String,
    pub created_on: String,
    pub lead_session: Option<String>,
    pub grounded_seq: Option<i64>,
    pub sync_failing: bool,
}

impl ContextRow {
    pub const COLUMNS: &'static str =
        "id, title, jira, created_at, created_on, lead_session, grounded_seq, sync_failing";

    pub fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ContextRow> {
        Ok(ContextRow {
            id: r.get(0)?,
            title: r.get(1)?,
            jira: r.get(2)?,
            created_at: r.get(3)?,
            created_on: r.get(4)?,
            lead_session: r.get(5)?,
            grounded_seq: r.get(6)?,
            sync_failing: r.get(7)?,
        })
    }

    pub fn find(conn: &Connection, id: &ContextId) -> rusqlite::Result<Option<ContextRow>> {
        conn.query_row(
            &format!("SELECT {} FROM contexts WHERE id = ?1", ContextRow::COLUMNS),
            [id.as_str()],
            ContextRow::from_row,
        )
        .optional()
    }

    /// `context_not_found` when there is no such Context.
    pub fn require(conn: &Connection, id: &ContextId) -> Result<ContextRow, CpdError> {
        ContextRow::find(conn, id)?.ok_or_else(|| {
            CpdError::new(ErrorCode::ContextNotFound, format!("No Context {id}.")).with("context", id.as_str())
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

/// The Context stores of one machine: their files under `$CPD_HOME` and their
/// index in the database. The writes take the Context lock themselves.
pub struct Contexts<'a> {
    pub(super) home: &'a CpdHome,
    pub(super) machine: MachineName,
    pub(super) store: &'a mut Store,
}

impl<'a> Contexts<'a> {
    pub fn new(home: &'a CpdHome, config: &Config, store: &'a mut Store) -> Contexts<'a> {
        Contexts { home, machine: config.machine.clone(), store }
    }

    /// `context new` (commands.md §3.1). Returns the Context's row and whether
    /// it already existed with this title and Jira key.
    pub fn create(
        &mut self,
        id: &ContextId,
        title: &Text,
        jira: Option<&Jira>,
    ) -> Result<(ContextRow, bool), CpdError> {
        let _lock = lock::acquire(self.home, id)?;
        if let Some(row) = self.store.read(|conn| Ok(ContextRow::find(conn, id)?))? {
            if row.title == title.as_str() && row.jira.as_deref() == jira.map(Jira::as_str) {
                return Ok((row, true));
            }
            return Err(CpdError::new(ErrorCode::ContextExists, format!("Context {id} exists with another title or Jira key."))
                .with("context", id.as_str())
                .with("title", row.title)
                .with("jira", row.jira));
        }
        let dir = self.home.context_dir(id);
        if dir.exists() {
            return Err(CpdError::new(
                ErrorCode::ContextExists,
                format!("{} exists but Context {id} is not in the index: sync imports it.", dir.display()),
            )
            .with("context", id.as_str())
            .with("path", dir.display().to_string())
            .with("next", format!("cpd context sync --context {id}")));
        }
        repo::create(self.home, &self.machine, id, title, jira, &now())?;
        let manifest = repo::read_manifest(self.home, id)?;
        self.import_locked(id, &manifest, "context new", false)?;
        let row = self.store.read(|conn| ContextRow::require(conn, id))?;
        Ok((row, false))
    }

    /// Appends one record (§4.6 "A write") under the Context lock: validates it,
    /// appends the line, commits, then indexes it and appends `context-record`.
    /// `command` is the `txns.command` of the index transaction.
    pub fn write(
        &mut self,
        context: &ContextId,
        command: &str,
        by: super::record::By,
        text: &Text,
        draft: Draft,
    ) -> Result<Written, CpdError> {
        let _lock = lock::acquire(self.home, context)?;
        let manifest = repo::read_manifest(self.home, context)?;
        // Repairs a crash between an earlier commit and its index insert, so
        // ids are numbered from every record on disk.
        self.import_locked(context, &manifest, command, false)?;

        let machine = self.machine.clone();
        let kind = draft.kind();
        let (latest, number) = self.store.read(|conn| {
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
            let retry = latest.filter(|(_, r)| r.draft == draft && r.text == text.as_str() && r.by.session == by.session);
            if retry.is_some() {
                return Ok((retry, 0));
            }
            check_links(conn, context, &draft)?;
            let mut stmt =
                conn.prepare("SELECT id FROM context_records WHERE context = ?1 AND machine = ?2 AND kind = ?3")?;
            let ids = stmt.query_map(params![context.as_str(), machine.as_str(), kind.as_str()], |r| r.get::<_, String>(0))?;
            let mut highest = 0;
            for id in ids {
                highest = highest.max(RecordId::parse(&id?)?.number().get());
            }
            Ok((None, highest + 1))
        })?;
        if let Some((seq, record)) = latest {
            return Ok(Written { record, seq, already: true });
        }

        let record = Record {
            id: RecordId::new(machine.clone(), kind, std::num::NonZeroU32::new(number).expect("one more than a count")),
            at: now(),
            by,
            text: text.as_str().to_owned(),
            draft,
        };
        let before = repo::append(self.home, context, &machine, &record)?;
        let dir = self.home.context_dir(context);
        let git = Git::new(&dir, &machine);
        let file = format!("records/{machine}.jsonl");
        let subject: String = record.text.chars().take(60).map(|c| if c.is_control() { ' ' } else { c }).collect();
        let committed = git.run(&["add", "--", &file]).and_then(|_| git.commit(&format!("{} {}: {subject}", kind.as_str(), record.id)));
        if let Err(e) = committed {
            repo::rewind(self.home, context, &machine, before);
            return Err(e);
        }
        let seq = self.store.write(command, |tx| {
            record.insert(tx, context)?;
            let seq = tx.last_insert_rowid();
            events::append(tx, record_event(context, &record, seq, "recorded"))?;
            Ok(seq)
        })?;
        Ok(Written { record, seq, already: false })
    }

    /// Re-imports every record file of every Context directory. Idempotent
    /// (§4.6); for the engine's start-up. Returns how many records were new.
    pub fn import_all(&mut self, command: &str) -> Result<usize, CpdError> {
        let mut imported = 0;
        for name in super::sync::store_names(self.home)? {
            let id = super::sync::store_id(self.home, &name)?;
            let _lock = lock::acquire(self.home, &id)?;
            let manifest = repo::read_manifest(self.home, &id)?;
            imported += self.import_locked(&id, &manifest, command, false)?;
        }
        Ok(imported)
    }

    /// The import of §4.6, with the Context lock held: the Context's row, when
    /// it has none yet (and `context-created`), and every record of the files
    /// whose `(context, id)` is not indexed (and `context-record` for each),
    /// in one transaction. With `sync_ok`, also clears `sync_failing`.
    /// Returns the number of records imported; writes nothing when there is
    /// nothing to do.
    pub(super) fn import_locked(
        &mut self,
        id: &ContextId,
        manifest: &Manifest,
        command: &str,
        sync_ok: bool,
    ) -> Result<usize, CpdError> {
        let records = repo::read_records(self.home, id)?;
        let (row, indexed) = self.store.read(|conn| {
            let row = ContextRow::find(conn, id)?;
            let mut stmt = conn.prepare("SELECT id FROM context_records WHERE context = ?1")?;
            let indexed = stmt.query_map([id.as_str()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<HashSet<_>>>()?;
            Ok((row, indexed))
        })?;
        let missing: Vec<&Record> = records.iter().filter(|r| !indexed.contains(&r.id.to_string())).collect();
        let clears = sync_ok && row.as_ref().is_some_and(|r| r.sync_failing);
        if row.is_some() && missing.is_empty() && !clears {
            return Ok(0);
        }
        self.store.write(command, |tx| {
            if row.is_none() {
                tx.execute(
                    "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id.as_str(), manifest.title, manifest.jira, manifest.created_at, manifest.created_on.as_str()],
                )?;
                events::append(
                    tx,
                    NewEvent::new(
                        Kind::ContextCreated,
                        format!("Context {id} \"{}\" exists. Next: cpd context show --context {id}", manifest.title),
                        json!({"title": manifest.title, "jira": manifest.jira, "created_on": manifest.created_on}),
                    )
                    .context(id.clone()),
                )?;
            }
            let mut imported = 0;
            for record in &missing {
                if record.insert(tx, id)? {
                    let seq = tx.last_insert_rowid();
                    events::append(tx, record_event(id, record, seq, "imported"))?;
                    imported += 1;
                }
            }
            if clears {
                tx.execute("UPDATE contexts SET sync_failing = 0 WHERE id = ?1", [id.as_str()])?;
            }
            Ok(imported)
        })
    }
}

/// §4.6 "A write": `supersedes` names an existing decision, `question` an
/// existing question; and an answer's question has no answer yet (§3.5).
fn check_links(conn: &Connection, context: &ContextId, draft: &Draft) -> Result<(), CpdError> {
    let exists = |id: &RecordId, kind: RecordKind| -> rusqlite::Result<bool> {
        conn.query_row(
            "SELECT 1 FROM context_records WHERE context = ?1 AND id = ?2 AND kind = ?3",
            params![context.as_str(), id.to_string(), kind.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map(|found| found.is_some())
    };
    let not_found = |id: &RecordId, kind: RecordKind| {
        CpdError::new(
            ErrorCode::RecordNotFound,
            format!("Context {context} has no {} {id}.", kind.as_str()),
        )
        .with("context", context.as_str())
        .with("record", id.to_string())
        .with("kind", kind.as_str())
    };
    match draft {
        Draft::Decision { supersedes: Some(id) } if !exists(id, RecordKind::Decision)? => {
            Err(not_found(id, RecordKind::Decision))
        }
        Draft::Answer { question } => {
            if !exists(question, RecordKind::Question)? {
                return Err(not_found(question, RecordKind::Question));
            }
            let answer: Option<String> = conn
                .query_row(
                    "SELECT id FROM context_records WHERE context = ?1 AND kind = 'answer' AND question = ?2 ORDER BY seq LIMIT 1",
                    params![context.as_str(), question.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            match answer {
                Some(answer) => Err(CpdError::new(
                    ErrorCode::QuestionAnswered,
                    format!("Question {question} is already answered by {answer}."),
                )
                .with("question", question.to_string())
                .with("answer", answer)),
                None => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

/// The `context-record` Event of a record (§5.2).
fn record_event(context: &ContextId, record: &Record, seq: i64, verb: &str) -> NewEvent {
    let snippet: String = record.text.chars().take(80).map(|c| if c.is_control() { ' ' } else { c }).collect();
    NewEvent::new(
        Kind::ContextRecord,
        format!(
            "{} {} {verb} in Context {context}: {snippet}. Next: cpd context show --context {context} --since {}",
            record.kind().as_str(),
            record.id,
            seq - 1
        ),
        json!({"record": record.id, "kind": record.kind().as_str(), "machine": record.machine(), "seq": seq}),
    )
    .context(context.clone())
}
