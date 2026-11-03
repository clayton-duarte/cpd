//! Read log of Context records (decision pro-d10 item 5): stubs, the implementation is another Job's.

use crate::error::CpdError;
use crate::ids::{ContextId, RecordId, SessionId};
use crate::store::Store;
use super::{ContextRow, Select};
use crate::env::Env;
use crate::output::Output;
use clap::Args;
use serde_json::{Value, json};

#[derive(Args, Debug)]
pub struct ReadsArgs {
    #[command(flatten)]
    select: Select,
    /// Only reads at or after this ISO-8601 instant
    #[arg(long, value_name = "iso")]
    since: Option<String>,
    /// The channels to count: pushed, search, show
    #[arg(long, value_name = "c,c", value_delimiter = ',')]
    channel: Option<Vec<String>>,
    /// Only records read by at least this many Sessions
    #[arg(long, value_name = "n")]
    min_sessions: Option<u32>,
}


use rusqlite::params;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::clock;
use crate::error::ErrorCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    Pushed,
    Search,
    Show,
    Shadow,
}

impl Channel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Channel::Pushed => "pushed",
            Channel::Search => "search",
            Channel::Show => "show",
            Channel::Shadow => "shadow",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Read {
    pub(crate) record: RecordId,
    pub(crate) rank: Option<u32>,
    pub(crate) score: Option<f64>,
}

/// The reads of a search page: 1-based rank in page order, the hit's score.
#[allow(dead_code)]
pub(crate) fn search_reads(hits: &[(RecordId, f64)]) -> Vec<Read> {
    hits.iter()
        .zip(1u32..)
        .map(|((record, score), rank)| Read {
            record: record.clone(),
            rank: Some(rank),
            score: Some(*score),
        })
        .collect()
}

/// Insert the reads in one transaction; a repeat of a key changes nothing. Returns the rows inserted.
pub(crate) fn log_reads_for(
    store: &mut Store,
    context: &ContextId,
    session: &SessionId,
    channel: Channel,
    reads: &[Read],
    at: &str,
) -> Result<usize, CpdError> {
    store.write("context-reads-log", |tx| {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO context_reads (context, record, session, channel, read_at, rank, score)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        let mut inserted = 0;
        for read in reads {
            inserted += stmt.execute(params![
                context.as_str(),
                read.record.to_string(),
                session.as_str(),
                channel.as_str(),
                at,
                read.rank,
                read.score
            ])?;
        }
        Ok(inserted)
    })
}

/// Mark used the records `text` cites (not in `exclude`) that `session` read. Returns the rows updated.
pub(crate) fn mark_used_for(
    store: &mut Store,
    context: &ContextId,
    session: &SessionId,
    text: &str,
    exclude: &[RecordId],
) -> Result<usize, CpdError> {
    let cited: Vec<RecordId> = RecordId::scan(text)
        .into_iter()
        .filter(|id| !exclude.contains(id))
        .collect();
    if cited.is_empty() {
        return Ok(0);
    }
    store.write("context-reads-used", |tx| {
        let mut stmt = tx.prepare(
            "UPDATE context_reads SET used = 1 WHERE context = ?1 AND session = ?2 AND record = ?3",
        )?;
        let mut updated = 0;
        for id in &cited {
            updated += stmt.execute(params![context.as_str(), session.as_str(), id.to_string()])?;
        }
        Ok(updated)
    })
}

/// The Session of this command: `CPD_SESSION`, else `PI_SESSION_ID`, when it is a known Session.
pub(crate) fn env_session(store: &mut Store) -> Option<SessionId> {
    let raw = ["CPD_SESSION", "PI_SESSION_ID"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))?;
    let id = SessionId::parse(&raw).ok()?;
    let known = store
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT count(*) FROM sessions WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get::<_, i64>(0),
                )?
                > 0)
        })
        .ok()?;
    known.then_some(id)
}

/// Best-effort: log the reads of the environment's Session; every error is swallowed.
#[allow(dead_code)]
pub(crate) fn log_reads(
    store: &mut Store,
    context: &ContextId,
    channel: Channel,
    reads: &[Read],
) {
    if let Some(session) = env_session(store) {
        let _ = log_reads_for(store, context, &session, channel, reads, &clock::now());
    }
}

/// Best-effort: mark used what `text` cites; every error is swallowed.
#[allow(dead_code)]
pub(crate) fn mark_used(
    store: &mut Store,
    context: &ContextId,
    text: &str,
    exclude: &[RecordId],
) {
    if let Some(session) = env_session(store) {
        let _ = mark_used_for(store, context, &session, text, exclude);
    }
}

/// The `--since` instant as `read_at` is written; `usage` unless an ISO-8601 instant with a zone.
pub(super) fn parse_since(text: &str) -> Result<String, CpdError> {
    OffsetDateTime::parse(text, &Rfc3339)
        .map(clock::timestamp)
        .map_err(|_| {
            CpdError::new(
                ErrorCode::Usage,
                format!("--since {text:?} is not an ISO-8601 instant with a zone (Z or ±hh:mm)."),
            )
            .with("value", text)
        })
}

/// The chosen channels in the order pushed, search, show; `usage` for any other name.
pub(super) fn parse_channels(names: &[String]) -> Result<Vec<Channel>, CpdError> {
    for name in names {
        if !["pushed", "search", "show"].contains(&name.as_str()) {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("--channel {name:?} is not one of pushed, search, show."),
            )
            .with("value", name.as_str()));
        }
    }
    Ok([Channel::Pushed, Channel::Search, Channel::Show]
        .into_iter()
        .filter(|c| names.iter().any(|n| n == c.as_str()))
        .collect())
}

/// `context reads` (§3.18).
pub(super) fn reads(args: ReadsArgs) -> Result<Output, CpdError> {
    let since = args.since.as_deref().map(parse_since).transpose()?;
    let channels = match &args.channel {
        Some(names) => parse_channels(names)?,
        None => vec![Channel::Pushed, Channel::Search, Channel::Show],
    };
    let min_sessions = args.min_sessions.unwrap_or(1);
    if min_sessions < 1 {
        return Err(CpdError::new(ErrorCode::Usage, "--min-sessions must be at least 1.")
            .with("value", min_sessions));
    }
    let Env { mut store, .. } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let names: Vec<&str> = channels.iter().map(|c| c.as_str()).collect();
    let (row, records) = store.read(|conn| {
        let row = ContextRow::require(conn, &context)?;
        let mut stmt = conn.prepare(
            "SELECT r.record, k.kind, count(*), count(DISTINCT r.session),
                    count(DISTINCT CASE WHEN r.used = 1 THEN r.session END)
               FROM context_reads r JOIN context_records k ON k.context = r.context AND k.id = r.record
              WHERE r.context = ?1 AND r.channel <> 'shadow'
                AND EXISTS (SELECT 1 FROM json_each(?2) WHERE value = r.channel)
                AND (?3 IS NULL OR r.read_at >= ?3)
              GROUP BY r.record, k.kind
             HAVING count(DISTINCT r.session) >= ?4
              ORDER BY count(*) DESC, count(DISTINCT r.session) DESC, r.record ASC",
        )?;
        let records = stmt
            .query_map(
                params![
                    context.as_str(),
                    json!(names).to_string(),
                    since,
                    min_sessions
                ],
                |r| {
                    Ok(json!({"id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?,
                              "reads": r.get::<_, i64>(2)?, "sessions": r.get::<_, i64>(3)?,
                              "used": r.get::<_, i64>(4)?}))
                },
            )?
            .collect::<rusqlite::Result<Vec<Value>>>()?;
        Ok((row, records))
    })?;
    let markdown = records
        .iter()
        .map(|r| {
            format!(
                "{} {} reads={} sessions={} used={}",
                r["id"].as_str().unwrap_or_default(),
                r["kind"].as_str().unwrap_or_default(),
                r["reads"],
                r["sessions"],
                r["used"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "since": since, "channels": names, "min_sessions": min_sessions,
        "records": records, "markdown": markdown,
    })))
}
