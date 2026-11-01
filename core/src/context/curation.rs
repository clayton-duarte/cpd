//! Record curation (hs-d549, hs-d551): the curations and retirement proposals the distiller writes, and the User's
//! decisions on them (contract §4.6 "Retirement"; commands.md §3.15 to §3.17).

use clap::Args;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::distill::State;
use super::{ContextRow, Select};
use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{ContextId, RecordId};
use crate::output::Output;
use crate::store::Store;

/// Why a Context is not curable (hs-d592 R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotCurable {
    Archived,
    Deleted,
    Young { records: u32 },
}

/// Whether the Context is curable: not archived, not deleted and holding at least `floor` records (every kind and
/// state). `Ok(None)` is curable. An unknown Context counts as deleted.
pub fn not_curable(
    conn: &Connection,
    context: &ContextId,
    floor: u32,
) -> Result<Option<NotCurable>, CpdError> {
    let row: Option<(bool, bool)> = conn
        .query_row(
            "SELECT archived_at IS NOT NULL, deleted_at IS NOT NULL FROM contexts WHERE id = ?1",
            [context.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((archived, deleted)) = row else {
        return Ok(Some(NotCurable::Deleted));
    };
    if deleted {
        return Ok(Some(NotCurable::Deleted));
    }
    if archived {
        return Ok(Some(NotCurable::Archived));
    }
    let records: i64 = conn.query_row(
        "SELECT count(*) FROM context_records WHERE context = ?1",
        [context.as_str()],
        |r| r.get(0),
    )?;
    let records = u32::try_from(records).unwrap_or(u32::MAX);
    Ok((records < floor).then_some(NotCurable::Young { records }))
}

/// Why a record is proposed for retirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Duplicate,
    Superseded,
    Documented,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Duplicate => "duplicate",
            Reason::Superseded => "superseded",
            Reason::Documented => "documented",
        }
    }
}

/// One record's curation: its topic and short form, and whether it is a documentation candidate (with the paths where
/// its id was found).
#[derive(Clone, Debug)]
pub struct Curation {
    pub record: RecordId,
    pub topic: String,
    pub short: String,
    pub doc_paths: Option<Vec<String>>,
}

/// A proposal to retire `record`; `target` is a record id or a doc path.
#[derive(Clone, Debug)]
pub struct Proposal {
    pub record: RecordId,
    pub reason: Reason,
    pub target: String,
}

/// Writes `curations` and pending retirements for `proposals` and appends `context-curated`, in one transaction. A record that already has a
/// retirement row (any status) gets no new proposal; a record whose doc_candidate is already set keeps it. Every
/// proposed record has a curation (written now or before): the commands list a proposal by its short form.
///
/// A proposal whose record, or whose `duplicate`/`superseded` target, is no longer in force when this transaction
/// reads it (the distiller runs for minutes; the User may have approved a retirement meanwhile) is dropped, not an
/// error. Returns the number of proposals considered written (the `retire` count of the Event).
pub fn propose(
    store: &mut Store,
    context: &ContextId,
    curations: &[Curation],
    proposals: &[Proposal],
) -> Result<usize, CpdError> {
    propose_rejecting(store, context, curations, proposals, 0)
}

/// [`propose`] for a run that rejected `rejected` lines of the distiller's output (GXD1, GXD2): the Event carries the count.
pub fn propose_rejecting(
    store: &mut Store,
    context: &ContextId,
    curations: &[Curation],
    proposals: &[Proposal],
    rejected: usize,
) -> Result<usize, CpdError> {
    store.write("context curate", |tx| {
        let at = clock::now();
        for c in curations {
            let paths = c.doc_paths.as_ref().map(|p| json!(p).to_string());
            let candidate = paths.as_ref().map(|_| "pending");
            tx.execute(
                "INSERT INTO record_curations (context, record, topic, short, doc_candidate, doc_paths, curated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (context, record) DO UPDATE SET
                   topic = excluded.topic, short = excluded.short, curated_at = excluded.curated_at,
                   doc_paths = CASE WHEN doc_candidate IS NULL THEN excluded.doc_paths ELSE doc_paths END,
                   doc_candidate = coalesce(doc_candidate, excluded.doc_candidate)",
                rusqlite::params![context.as_str(), c.record.to_string(), c.topic, c.short, candidate, paths, at],
            )?;
        }
        let force = State::read(tx, context)?.in_force_ids();
        let proposals: Vec<&Proposal> = proposals
            .iter()
            .filter(|p| {
                force.contains(&p.record.to_string())
                    && (p.reason == Reason::Documented || force.contains(&p.target))
            })
            .collect();
        for p in &proposals {
            tx.execute(
                "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending', ?5) ON CONFLICT (context, record) DO NOTHING",
                rusqlite::params![context.as_str(), p.record.to_string(), p.reason.as_str(), p.target, at],
            )?;
            let curated: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM record_curations WHERE context = ?1 AND record = ?2)",
                [context.as_str(), p.record.to_string().as_str()],
                |r| r.get(0),
            )?;
            if !curated {
                return Err(CpdError::new(ErrorCode::Internal, format!("Record {} is proposed for retirement but not curated.", p.record))
                    .with("record", p.record.to_string()));
            }
        }
        let docs = curations.iter().filter(|c| c.doc_paths.is_some()).count();
        let text = format!(
            "Context {context} curated: {} records, {rejected} lines rejected, {} retirements proposed, {docs} doc candidates.",
            curations.len(),
            proposals.len()
        );
        append(
            tx,
            NewEvent::context_curated(text, context.as_str(), curations.len() as u64, rejected as u64, proposals.len() as u64, docs as u64).context(context.clone()),
        )?;
        Ok(proposals.len())
    })
}

#[derive(Args, Debug)]
pub struct CurationArgs {
    #[command(flatten)]
    select: Select,
    /// At most this many entries per list (default: all)
    #[arg(long, value_name = "n")]
    limit: Option<u32>,
}

#[derive(Args, Debug)]
pub struct RetireArgs {
    #[command(flatten)]
    select: Select,
    /// The pending retirements to approve
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    approve: Vec<RecordId>,
    /// The pending retirements to reject
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    reject: Vec<RecordId>,
}

#[derive(Args, Debug)]
pub struct DocsArgs {
    #[command(flatten)]
    select: Select,
    /// The pending doc candidates to send to the Manager
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    send: Vec<RecordId>,
    /// The pending doc candidates to dismiss
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    dismiss: Vec<RecordId>,
}

/// Only the User decides (hs-d551): no Session and no Attempt runs these commands.
fn user_only(command: &str) -> Result<(), CpdError> {
    for var in ["CPD_SESSION", "CPD_ATTEMPT"] {
        if std::env::var_os(var).is_some() {
            return Err(CpdError::new(
                ErrorCode::UserOnly,
                format!("{command} is for the User: {var} is set, so an agent's Session runs it."),
            )
            .with("env", var));
        }
    }
    Ok(())
}

/// The ids of both lists, each without repeats, in the order given; `args_invalid` when one is in both, `usage` when
/// both are empty.
fn decided(
    command: &str,
    first: (&str, &[RecordId]),
    second: (&str, &[RecordId]),
) -> Result<(Vec<String>, Vec<String>), CpdError> {
    if first.1.is_empty() && second.1.is_empty() {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("{command} needs {} or {}.", first.0, second.0),
        ));
    }
    let list = |ids: &[RecordId]| {
        let mut out: Vec<String> = Vec::new();
        for id in ids.iter().map(ToString::to_string) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    };
    let (a, b) = (list(first.1), list(second.1));
    if let Some(both) = a.iter().find(|id| b.contains(id)) {
        return Err(CpdError::new(
            ErrorCode::ArgsInvalid,
            format!("{both} is in both {} and {}.", first.0, second.0),
        )
        .with("record", both.as_str()));
    }
    Ok((a, b))
}

fn not_pending(context: &ContextId, record: &str) -> CpdError {
    CpdError::new(
        ErrorCode::CurationNotPending,
        format!("Record {record} of Context {context} has nothing pending."),
    )
    .with("record", record)
    .with("context", context.as_str())
}

/// Refuses the first of `ids` that has no pending row of `table`'s kind (`column` is the status column).
fn all_pending(
    conn: &Connection,
    context: &ContextId,
    table: &str,
    column: &str,
    ids: &[&String],
) -> Result<(), CpdError> {
    let sql =
        format!("SELECT {column} = 'pending' FROM {table} WHERE context = ?1 AND record = ?2");
    for id in ids {
        let pending: Option<Option<bool>> = conn
            .query_row(&sql, [context.as_str(), id.as_str()], |r| r.get(0))
            .optional()?;
        if pending != Some(Some(true)) {
            return Err(not_pending(context, id));
        }
    }
    Ok(())
}

/// `context curation` (§3.15).
pub(super) fn curation(args: CurationArgs) -> Result<Output, CpdError> {
    user_only("context curation")?;
    let Env { mut store, .. } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    if args.limit == Some(0) {
        return Err(CpdError::new(
            ErrorCode::ArgsInvalid,
            "--limit must be an integer of 1 or more.",
        )
        .with("limit", 0));
    }
    let limit = args.limit.map_or(-1, i64::from);
    let (retire, docs, retire_total, docs_total) = store.read(|conn| {
        ContextRow::require(conn, &context)?;
        let mut stmt = conn.prepare(
            "SELECT r.record, c.kind, c.text, coalesce(k.short, ''), r.reason, r.target
               FROM record_retirements r
               JOIN context_records c ON c.context = r.context AND c.id = r.record
               LEFT JOIN record_curations k ON k.context = r.context AND k.record = r.record
              WHERE r.context = ?1 AND r.status = 'pending' ORDER BY r.proposed_at, c.seq LIMIT ?2",
        )?;
        let retire = stmt
            .query_map(rusqlite::params![context.as_str(), limit], |r| {
                Ok(json!({"record": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "text": r.get::<_, String>(2)?,
                          "short": r.get::<_, String>(3)?, "reason": r.get::<_, String>(4)?, "target": r.get::<_, String>(5)?}))
            })?
            .collect::<rusqlite::Result<Vec<Value>>>()?;
        let mut stmt = conn.prepare(
            "SELECT k.record, c.kind, c.text, k.short, coalesce(k.doc_paths, '[]')
               FROM record_curations k JOIN context_records c ON c.context = k.context AND c.id = k.record
              WHERE k.context = ?1 AND k.doc_candidate = 'pending' ORDER BY k.curated_at, c.seq LIMIT ?2",
        )?;
        let docs = stmt
            .query_map(rusqlite::params![context.as_str(), limit], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let docs = docs
            .into_iter()
            .map(|(record, kind, text, short, paths)| {
                let paths: Value = serde_json::from_str(&paths)
                    .map_err(|e| CpdError::new(ErrorCode::StoreFailed, format!("The doc paths of {record} are not JSON: {e}.")))?;
                Ok(json!({"record": record, "kind": kind, "text": text, "short": short, "paths": paths}))
            })
            .collect::<Result<Vec<Value>, CpdError>>()?;
        let total = |sql: &str| conn.query_row(sql, [context.as_str()], |r| r.get::<_, i64>(0));
        let retire_total = total(
            "SELECT count(*) FROM record_retirements r JOIN context_records c ON c.context = r.context AND c.id = r.record
              WHERE r.context = ?1 AND r.status = 'pending'",
        )?;
        let docs_total = total(
            "SELECT count(*) FROM record_curations k JOIN context_records c ON c.context = k.context AND c.id = k.record
              WHERE k.context = ?1 AND k.doc_candidate = 'pending'",
        )?;
        Ok((retire, docs, retire_total, docs_total))
    })?;
    Ok(Output::success(
        json!({"context": context, "retire": retire, "docs": docs, "retire_total": retire_total, "docs_total": docs_total}),
    ))
}

/// `context retire` (§3.16).
pub(super) fn retire(args: RetireArgs) -> Result<Output, CpdError> {
    user_only("context retire")?;
    let (approve, reject) = decided(
        "context retire",
        ("--approve", &args.approve),
        ("--reject", &args.reject),
    )?;
    let Env { mut store, .. } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    store.write("context retire", |tx| {
        ContextRow::require(tx, &context)?;
        all_pending(tx, &context, "record_retirements", "status", &approve.iter().chain(&reject).collect::<Vec<_>>())?;
        let at = clock::now();
        for (status, ids) in [("approved", &approve), ("rejected", &reject)] {
            for id in ids {
                tx.execute(
                    "UPDATE record_retirements SET status = ?3, decided_at = ?4, decided_by = 'user' WHERE context = ?1 AND record = ?2",
                    rusqlite::params![context.as_str(), id, status, at],
                )?;
            }
        }
        let text = format!("Retirements of {context} decided: {} approved, {} rejected.", approve.len(), reject.len());
        append(tx, NewEvent::records_retired(text, context.as_str(), &approve, &reject).context(context.clone()))?;
        Ok(())
    })?;
    Ok(Output::success(
        json!({"context": context, "approved": approve, "rejected": reject}),
    ))
}

/// The board's limit of a post's characters (`board post`, commands.md §10a.1).
const POST_MAX_CHARS: usize = 2000;

/// The one board post that sends `sent` (id, short form, paths) to the Manager: a record whose line does not fit
/// goes by id alone, and the text is cut at the limit, so that it is always written.
fn docs_post(context: &ContextId, sent: &[(String, String, Vec<String>)]) -> String {
    let mut text = format!("Documentation candidates of Context {context}, sent by the User: plan a docs Workflow that documents these records.\n");
    let mut room = POST_MAX_CHARS.saturating_sub(text.chars().count());
    for (id, short, paths) in sent {
        let full = format!("- {id}: {short} ({})\n", paths.join(", "));
        let line = if full.chars().count() <= room {
            full
        } else {
            format!("- {id}\n")
        };
        room = room.saturating_sub(line.chars().count());
        text.push_str(&line);
    }
    text.chars().take(POST_MAX_CHARS).collect()
}

/// `context docs` (§3.17).
pub(super) fn docs(args: DocsArgs) -> Result<Output, CpdError> {
    user_only("context docs")?;
    let (send, dismiss) = decided(
        "context docs",
        ("--send", &args.send),
        ("--dismiss", &args.dismiss),
    )?;
    let Env { mut store, .. } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let manager = store.write("context docs", |tx| {
        ContextRow::require(tx, &context)?;
        all_pending(tx, &context, "record_curations", "doc_candidate", &send.iter().chain(&dismiss).collect::<Vec<_>>())?;
        let manager: Option<String> = tx
            .query_row(
                "SELECT id FROM sessions WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL ORDER BY created_at, id LIMIT 1",
                [context.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if manager.is_none() && !send.is_empty() {
            return Err(CpdError::new(ErrorCode::ManagerMissing, format!("Context {context} has no live Manager to send the candidates to."))
                .with("context", context.as_str()));
        }
        let at = clock::now();
        let mut sent = Vec::new();
        for (status, ids) in [("sent", &send), ("dismissed", &dismiss)] {
            for id in ids {
                if status == "sent" {
                    let (short, paths): (String, String) = tx.query_row(
                        "SELECT short, coalesce(doc_paths, '[]') FROM record_curations WHERE context = ?1 AND record = ?2",
                        [context.as_str(), id.as_str()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?;
                    let paths: Vec<String> = serde_json::from_str(&paths)
                        .map_err(|e| CpdError::new(ErrorCode::StoreFailed, format!("The doc paths of {id} are not a JSON list: {e}.")))?;
                    sent.push((id.clone(), short, paths));
                }
                tx.execute(
                    "UPDATE record_curations SET doc_candidate = ?3 WHERE context = ?1 AND record = ?2",
                    rusqlite::params![context.as_str(), id, status],
                )?;
            }
        }
        if let Some(manager) = manager.as_ref().filter(|_| !send.is_empty()) {
            tx.execute(
                "INSERT INTO board_posts (at, context, from_session, to_session, text) VALUES (?1, ?2, NULL, ?3, ?4)",
                rusqlite::params![at, context.as_str(), manager, docs_post(&context, &sent)],
            )?;
        }
        let text = format!("Doc candidates of {context} decided: {} sent, {} dismissed.", send.len(), dismiss.len());
        append(tx, NewEvent::docs_decided(text, context.as_str(), &send, &dismiss).context(context.clone()))?;
        Ok(manager.filter(|_| !send.is_empty()))
    })?;
    let mut out = json!({"context": context, "sent": send, "dismissed": dismiss});
    if let Some(manager) = manager {
        out["manager"] = json!(manager);
    }
    Ok(Output::success(out))
}
