//! The Context writes (contract §4.6): creating a Context and writing a
//! record, each one database transaction.

use rusqlite::{Connection, OptionalExtension, params};

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
            store,
        }
    }

    /// `context new` (commands.md §3.1): the Context's row and `context-created`.
    /// Returns the row and whether it already existed with this title, Jira key and owner.
    /// The owner's policy file must resolve (`owner_unconfirmed`, `policy_invalid`).
    pub fn create(
        &mut self,
        home: &CpdHome,
        id: &ContextId,
        title: &Text,
        jira: Option<&Jira>,
        owner: &OwnerLogin,
        confirm_fallback: bool,
    ) -> Result<(ContextRow, bool), CpdError> {
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
        if let Some(row) = self.store.read(|conn| existing(conn))? {
            return Ok((row, true));
        }
        let link = match resolve(home, &PolicyKey::Owner(owner.clone())) {
            Err(e) if e.code() == ErrorCode::OwnerUnconfirmed && confirm_fallback => true,
            other => other.map(|_| false)?,
        };
        if link {
            crate::policy::confirm_fallback(home, owner)?;
        }
        let machine = self.machine.clone();
        self.store.write("context new", |tx| {
            // Another process may have created it since the read.
            if let Some(row) = existing(tx)? {
                return Ok((row, true));
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
            Ok((ContextRow::require(tx, id)?, false))
        })
    }

    /// Writes one record (§4.6 "A write"): in one transaction, validates it
    /// (`record_not_found`, `question_answered`), inserts the row and appends
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
/// record may supersede (`supersedes_invalid` otherwise), `question` an existing question; and an answer's question has no answer yet (§3.5).
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
    match draft {
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
