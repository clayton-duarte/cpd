//! `cpd context search`: the Context's records ranked by the holographic engine
//! (contract §4.6; docs/research/context-holographic.md, `## Search contract` and
//! `## Index and staleness`).
//!
//! cpd.db is the source of truth: the engine is a derived per-Context index. A
//! search first syncs that index (from scratch on first use, else the records added
//! or curated since the watermark), then asks the engine, maps every hit back to its
//! live row, and filters by kind and by force. The engine's order is kept, exact
//! ties break by `seq` descending, and no hit is re-ranked.

use std::collections::{HashMap, HashSet};

use clap::Args;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::engine::{Bridge, ContextEngine};
use super::inforce;
use super::query;
use super::reads::{self, Channel};
use super::render::Indexed;
use super::{ContextRow, Record, Select};
use crate::args::Text;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::RecordId;
use crate::ids::{ContextId, RecordKind};
use crate::output::Output;
use crate::store::Store;

/// A curated record's topic, short form and curation clock.
type Curation = (String, String, Option<String>);

/// `context search <query> [--context <c>] [--limit <n>] [--kind <k,k>] [--all]`.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// The query: words are text (an id like hs-d655, a path or don't is quoted for you); "a phrase", AND, OR, NOT, NEAR(..), parentheses and a trailing `*` (prefix match) are kept
    query: Text,
    #[command(flatten)]
    select: Select,
    /// At most this many records; 0 is every match
    #[arg(long, value_name = "n", default_value_t = 20)]
    limit: u32,
    /// Only these kinds, comma-separated [default: decision,note,question,answer]
    #[arg(long, value_name = "k,k", value_delimiter = ',')]
    kind: Vec<String>,
    /// Include the records out of force, labelled
    #[arg(long)]
    all: bool,
}

/// The Context records, curations and live force a search reads once.
pub(super) struct Snapshot {
    pub row: ContextRow,
    pub records: Vec<Indexed>,
    pub in_force: HashSet<String>,
    pub retired: HashMap<String, String>,
    pub successor: HashMap<String, String>,
    pub curations: HashMap<String, Curation>,
    pub max_seq: i64,
    pub max_curated: Option<String>,
}

/// Reads the Context's row, records (index order), approved retirements, the live
/// in-force set and the curations, in one read transaction.
pub(super) fn snapshot(store: &mut Store, context: &ContextId) -> Result<Snapshot, CpdError> {
    store.read(|conn| {
        let row = ContextRow::require(conn, context)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq",
            Record::COLUMNS
        ))?;
        let records = stmt
            .query_map([context.as_str()], Record::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let as_of = records.last().map(|(seq, _)| *seq).unwrap_or(0);
        let curated = curations(conn, context)?;
        let max_curated = curated
            .values()
            .filter_map(|(_, _, at)| at.as_deref())
            .max()
            .map(str::to_owned);
        let retired_ids = inforce::retired(conn, context)?;
        let in_force = inforce::in_force_without(&records, as_of, &retired_ids)
            .into_iter()
            .map(|(_, record)| record.id.to_string())
            .collect();
        Ok(Snapshot {
            row,
            records,
            in_force,
            retired: retired_labels(conn, context)?,
            successor: successors(conn, context)?,
            curations: curated,
            max_seq: as_of,
            max_curated,
        })
    })
}

/// The approved retirements of `context`, labelled `<reason> <target>` by record id.
pub(super) fn retired_labels(
    conn: &Connection,
    context: &ContextId,
) -> Result<HashMap<String, String>, CpdError> {
    let mut stmt =
        conn.prepare("SELECT record, reason, target FROM record_retirements WHERE context = ?1 AND status = 'approved'")?;
    let rows = stmt.query_map([context.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            format!("{} {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The record id each superseded record is named by, from `context_records`.
fn successors(conn: &Connection, context: &ContextId) -> Result<HashMap<String, String>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, supersedes FROM context_records WHERE context = ?1 AND supersedes IS NOT NULL",
    )?;
    let rows = stmt.query_map([context.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows
        .map(|row| row.map(|(id, old)| (old, id)))
        .collect::<rusqlite::Result<_>>()?)
}

/// The curation (topic, short and clock) of every curated record of `context`.
pub(super) fn curations(
    conn: &Connection,
    context: &ContextId,
) -> Result<HashMap<String, Curation>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT record, topic, short, curated_at FROM record_curations WHERE context = ?1",
    )?;
    let rows = stmt.query_map([context.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A projected fact: the record id, its index position, its text and its curation (topic, short).
pub(super) fn fact(seq: i64, record: &Record, curations: &HashMap<String, Curation>) -> Value {
    let id = record.id.to_string();
    let (topic, short): (&str, &str) = curations
        .get(&id)
        .map(|(topic, short, _)| (topic.as_str(), short.as_str()))
        .unwrap_or(("", ""));
    json!({"record": id, "seq": seq, "topic": topic, "short": short, "text": record.text})
}

/// `cpd context search` (docs/research/context-holographic.md, `## Search contract`).
pub(super) fn search(args: SearchArgs) -> Result<Output, CpdError> {
    let query = args.query.as_str().trim().to_owned();
    if query.is_empty() {
        return Err(
            CpdError::new(ErrorCode::Usage, "A search query must not be empty.")
                .with("query", args.query.as_str()),
        );
    }
    let sent = query::sanitise(&query);
    let kinds = kinds(&args.kind)?;
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let snap = snapshot(&mut store, &context)?;
    let bridge = Bridge::new(
        config.context_engine_enabled,
        config.context_engine_bridge_path()?,
        config.context_engine_python_path()?,
        config.context_engine_timeout(),
    )?;
    let db = config
        .context_engine_dir(&home)
        .join(format!("{}.db", context.as_str()));

    let stats = bridge.run(&db, &[json!({"cmd": "stats"})])?.remove(0);
    check_ok(&stats, &query)?;
    let total = stats["facts"].as_u64().unwrap_or(0);
    let indexed_seq = stats["indexed_seq"].as_i64().unwrap_or(0);
    let indexed_curated = stats["curated_at"].as_str().map(str::to_owned);

    let mut requests: Vec<Value> = Vec::new();
    if indexed_seq == 0 {
        requests.push(json!({"cmd": "reset"}));
        requests.push(index(&snap, &snap.records));
    } else {
        let changed: Vec<Indexed> = snap
            .records
            .iter()
            .filter(|(seq, record)| {
                *seq > indexed_seq
                    || curation_is_newer(&snap, &record.id.to_string(), indexed_curated.as_deref())
            })
            .cloned()
            .collect();
        if !changed.is_empty() {
            requests.push(index(&snap, &changed));
        }
    }

    let n = over_fetch(total, args.limit);
    requests.push(search_request(&sent, n));
    let responses = bridge.run(&db, &requests)?;
    for response in &responses {
        check_ok(response, &query)?;
    }
    let mut hits = hits_of(&responses[responses.len() - 1], &snap, &kinds, args.all);

    // One retry with a larger N when the filtered page is short and the engine truncated.
    if args.limit > 0
        && (hits.len() as u64) < u64::from(args.limit)
        && response_len(&responses[responses.len() - 1]) == n
        && n < total
    {
        let m = n.saturating_mul(5).min(total);
        let again = bridge.run(&db, &[search_request(&sent, m)])?;
        check_ok(&again[0], &query)?;
        hits = hits_of(&again[0], &snap, &kinds, args.all);
    }

    let take = if args.limit == 0 {
        usize::MAX
    } else {
        args.limit as usize
    };
    let page: Vec<&Hit> = hits.iter().take(take).collect();
    let records: Vec<Value> = page.iter().map(|hit| hit.json()).collect();
    let markdown = page
        .iter()
        .map(|hit| hit.line(&snap))
        .collect::<Vec<_>>()
        .join("\n");
    let page_hits: Vec<(RecordId, f64)> = page
        .iter()
        .filter_map(|hit| Some((hit.id.parse().ok()?, hit.score)))
        .collect();
    reads::log_reads(
        &mut store,
        &context,
        Channel::Search,
        &reads::search_reads(&page_hits),
    );
    Ok(Output::success(json!({
        "context": {"id": snap.row.id, "title": snap.row.title, "jira": snap.row.jira},
        "query": query,
        "records": records,
        "markdown": markdown,
    })))
}

/// The kinds to keep: the four searchable kinds by default, else the named ones.
fn kinds(names: &[String]) -> Result<HashSet<&'static str>, CpdError> {
    if names.is_empty() {
        return Ok([
            RecordKind::Decision,
            RecordKind::Note,
            RecordKind::Question,
            RecordKind::Answer,
        ]
        .into_iter()
        .map(RecordKind::as_str)
        .collect());
    }
    let mut kinds = HashSet::new();
    for name in names {
        let kind = RecordKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == name)
            .ok_or_else(|| {
                CpdError::new(ErrorCode::Usage, format!("Unknown record kind {name:?}."))
                    .with("kind", name.as_str())
            })?;
        kinds.insert(kind.as_str());
    }
    Ok(kinds)
}

/// Whether `id` was curated after the watermark `indexed_curated`.
fn curation_is_newer(snap: &Snapshot, id: &str, indexed_curated: Option<&str>) -> bool {
    snap.curations
        .get(id)
        .and_then(|(_, _, at)| at.as_deref())
        .is_some_and(|at| indexed_curated.is_none_or(|before| at > before))
}

/// `{"cmd":"index","facts":[…],"indexed_seq":…,"curated_at":…}` for `records`,
/// carrying the Context's watermark so the bridge advances it even when only an
/// older record's curation changed.
pub(super) fn index(snap: &Snapshot, records: &[Indexed]) -> Value {
    let facts: Vec<Value> = records
        .iter()
        .map(|(seq, record)| fact(*seq, record, &snap.curations))
        .collect();
    json!({
        "cmd": "index",
        "facts": facts,
        "indexed_seq": snap.max_seq,
        "curated_at": snap.max_curated,
    })
}

/// `{"cmd":"search","query":…,"limit":…}`.
fn search_request(query: &str, limit: u64) -> Value {
    json!({"cmd": "search", "query": query, "limit": limit})
}

/// The over-fetch `N = min(total_facts, max(limit·5, 80))`; `--limit 0` is all facts.
fn over_fetch(total: u64, limit: u32) -> u64 {
    if limit == 0 {
        return total;
    }
    u64::from(limit).saturating_mul(5).max(80).min(total)
}

/// The number of hits in a `search` response.
fn response_len(response: &Value) -> u64 {
    response["hits"]
        .as_array()
        .map_or(0, |hits| hits.len() as u64)
}

/// A response that is not `ok` is a refusal: `bad_query` is `usage`, anything else `engine_failed`.
pub(super) fn check_ok(response: &Value, query: &str) -> Result<(), CpdError> {
    if response["ok"] == true {
        return Ok(());
    }
    let error = response["error"].as_str().unwrap_or("engine_error");
    let message = response["message"]
        .as_str()
        .unwrap_or("the engine failed")
        .to_owned();
    if error == "bad_query" {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("The search query is malformed: {message}"),
        )
        .with("query", query)
        .with("message", message));
    }
    Err(CpdError::new(
        ErrorCode::EngineFailed,
        format!("The holographic engine failed: {message}"),
    )
    .with("message", message))
}

/// One mapped, filtered hit, in the engine's order (before the tie-break sort).
struct Hit {
    id: String,
    short: String,
    kind: &'static str,
    at: String,
    seq: i64,
    score: f64,
    in_force: bool,
}

impl Hit {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "short": self.short,
            "kind": self.kind,
            "at": self.at,
            "seq": self.seq,
            "score": self.score,
            "in_force": self.in_force,
        })
    }

    /// The `- [<id>] <short>` line, with the out-of-force labels of `context show`.
    fn line(&self, snap: &Snapshot) -> String {
        let mut line = format!("- [{}] {}", self.id, self.short);
        if let Some(successor) = snap.successor.get(&self.id) {
            line.push_str(&format!(" (superseded by {successor})"));
        }
        if let Some(label) = snap.retired.get(&self.id) {
            line.push_str(&format!(" [retired ({label})]"));
        }
        line
    }
}

/// Maps the response's hits back to live rows, filters by kind and (unless `all`)
/// force, and breaks exact ties by `seq` descending, keeping the engine's order.
fn hits_of(
    response: &Value,
    snap: &Snapshot,
    kinds: &HashSet<&'static str>,
    all: bool,
) -> Vec<Hit> {
    let mut hits: Vec<Hit> = response["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|hit| {
            let id = hit["record"].as_str()?;
            let (seq, record) = snap
                .records
                .iter()
                .find(|(_, record)| record.id.to_string() == id)?;
            let in_force = snap.in_force.contains(id);
            // `--all` governs supersession and retirement, which only the four
            // force kinds have; a `plan`/`brief` is never in force and is shown
            // whenever its kind is selected.
            if !all && !in_force && force_kind(record.kind()) {
                return None;
            }
            if !kinds.contains(record.kind().as_str()) {
                return None;
            }
            Some(Hit {
                id: id.to_owned(),
                short: short(&record.id.to_string(), record, &snap.curations),
                kind: record.kind().as_str(),
                at: record.at.clone(),
                seq: *seq,
                score: hit["score"].as_f64().unwrap_or(0.0),
                in_force,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.seq.cmp(&a.seq))
    });
    hits
}

/// Whether a record kind is ever in force (`inforce.rs`).
fn force_kind(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::Decision | RecordKind::Note | RecordKind::Question | RecordKind::Answer
    )
}

/// The curated short form, else the record's first line at most 120 bytes.
fn short(id: &str, record: &Record, curations: &HashMap<String, Curation>) -> String {
    if let Some((_, short, _)) = curations.get(id) {
        return short.clone();
    }
    let first = record.first_line();
    let mut end = first.len().min(120);
    while !first.is_char_boundary(end) {
        end -= 1;
    }
    first[..end].to_owned()
}
