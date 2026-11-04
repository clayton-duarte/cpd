//! `cpd context search`: the Context's records ranked by full-text search (contract §4.6).
//!
//! cpd.db is the source of truth. Each search reads a snapshot, builds an in-memory
//! FTS5 table over every record (`fts.rs`), maps the hits back to live rows, and
//! filters by kind, by force and by pending retirement. A relative score floor drops
//! the weak tail; exact ties break by `seq` descending.

use std::collections::{HashMap, HashSet};

use clap::Args;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::curate_run::{DIMS, EMBED_MODEL, bifrost_embedder};
use super::curate_vectors::{SearchLogRow, VectorStore};
use super::fts::{self, Doc, Scored};
use super::hybrid::{rrf, semantic_list};
use super::inforce;
use super::reads::{self, Channel};
use super::render::Indexed;
use super::{ContextRow, Record, Select};
use crate::args::Text;
use crate::embedding::EmbeddingPort;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, RecordId, RecordKind};
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
    /// Only these kinds, comma-separated [default: decision,note]
    #[arg(long, value_name = "k,k", value_delimiter = ',')]
    kind: Vec<String>,
    /// Include the records out of force, labelled
    #[arg(long)]
    all: bool,
}

/// The Context records, curations and live force a search reads once.
struct Snapshot {
    pub row: ContextRow,
    pub records: Vec<Indexed>,
    pub in_force: HashSet<String>,
    pub retired: HashMap<String, String>,
    pub successor: HashMap<String, String>,
    pub curations: HashMap<String, Curation>,
    /// `record_topics`: record to its provisional topic (shown until a curation exists).
    pub provisional: HashMap<String, String>,
    pub hidden: HashSet<String>,
}

/// Reads the Context's row, records (index order), approved retirements, the live
/// in-force set and the curations, in one read transaction.
fn snapshot(store: &mut Store, context: &ContextId) -> Result<Snapshot, CpdError> {
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
            provisional: provisional(conn, context)?,
            hidden: pending_hidden(conn, context)?,
        })
    })
}

/// The records with a pending duplicate or superseded retirement proposal.
fn pending_hidden(conn: &Connection, context: &ContextId) -> Result<HashSet<String>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT record FROM record_retirements WHERE context = ?1 AND status = 'pending' \
         AND reason IN ('duplicate', 'superseded')",
    )?;
    let rows = stmt.query_map([context.as_str()], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The approved retirements of `context`, labelled `<reason> <target>` by record id.
fn retired_labels(
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
    let rows = stmt
        .query_map([context.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .flat_map(|(id, olds)| {
            olds.split(',')
                .map(|old| (old.to_owned(), id.clone()))
                .collect::<Vec<_>>()
        })
        .collect())
}

/// The provisional topic of every record of `context` that has one.
fn provisional(
    conn: &Connection,
    context: &ContextId,
) -> Result<HashMap<String, String>, CpdError> {
    let mut stmt = conn.prepare("SELECT record, topic FROM record_topics WHERE context = ?1")?;
    let rows = stmt.query_map([context.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The topic of a record: its curation's, else its provisional one.
fn topic_of(snap: &Snapshot, id: &str) -> Option<String> {
    match snap.curations.get(id) {
        Some((topic, _, _)) => Some(topic.clone()),
        None => snap.provisional.get(id).cloned(),
    }
}

/// The curation (topic, short and clock) of every curated record of `context`.
fn curations(
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

/// `cpd context search`.
pub(super) fn search(args: SearchArgs) -> Result<Output, CpdError> {
    let query = args.query.as_str().trim().to_owned();
    if query.is_empty() {
        return Err(
            CpdError::new(ErrorCode::Usage, "A search query must not be empty.")
                .with("query", args.query.as_str()),
        );
    }
    let kinds = kinds(&args.kind)?;
    let Env {
        home,
        config,
        mut store,
        ports,
        ..
    } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let snap = snapshot(&mut store, &context)?;

    let docs: Vec<Doc> = snap
        .records
        .iter()
        .map(|(_, record)| {
            let id = record.id.to_string();
            let topic = topic_of(&snap, &id).unwrap_or_default();
            let short = snap
                .curations
                .get(&id)
                .map(|(_, short, _)| short.clone())
                .unwrap_or_default();
            Doc {
                id,
                topic,
                short,
                text: record.text.clone(),
            }
        })
        .collect();
    let fts_start = ports.clock.now();
    let ranked = fts::rank(&docs, &query)?;
    let named = snap
        .records
        .iter()
        .map(|(_, record)| record.id.to_string())
        .find(|id| id.eq_ignore_ascii_case(&query));
    let visible: Vec<String> = filter(
        docs.iter()
            .map(|doc| Scored {
                id: doc.id.clone(),
                score: 0.0,
            })
            .collect(),
        &snap,
        &kinds,
        args.all,
        named.as_deref(),
    )
    .into_iter()
    .map(|hit| hit.id)
    .collect();
    let pending_hidden = if args.all {
        0
    } else {
        filter(ranked.clone(), &snap, &kinds, true, named.as_deref())
            .iter()
            .filter(|hit| {
                snap.hidden.contains(&hit.id) && named.as_deref() != Some(hit.id.as_str())
            })
            .count() as u64
    };
    let unfloored = filter(ranked, &snap, &kinds, args.all, named.as_deref());
    let candidates = unfloored.len() as u64;
    let lexical = fts::floor(unfloored, named.as_deref());
    let floor_dropped = candidates - lexical.len() as u64;
    let fts_ms = ports
        .clock
        .now()
        .saturating_duration_since(fts_start)
        .as_millis() as u64;
    let (kept, mode, semantic_error, embed_ms, embed_tokens) = match semantic(
        &SemanticInput {
            home: &home,
            config: &config,
            runner: &ports.runner,
            context: &context,
            query: &query,
            clock: &ports.clock,
        },
        &visible,
    )? {
        Semantic::None => (lexical, "fts", None, None, None),
        Semantic::Failed(code) => (lexical, "fts-only", Some(code), None, None),
        Semantic::List(list, ms, tokens) => (
            rrf(&lexical, &list, named.as_deref()),
            "hybrid",
            None,
            Some(ms),
            tokens,
        ),
    };
    let hits = hits_of(kept, &snap);

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
    let session = reads::env_session(&mut store).map(|id| id.to_string());
    let at = crate::clock::timestamp(ports.clock.utc());
    // Best-effort: a vectors.db failure never fails the search.
    let _ = VectorStore::at(&home).log_search(&SearchLogRow {
        at: &at,
        context: &context,
        session: session.as_deref(),
        query: &query,
        hybrid: mode == "hybrid",
        candidates,
        floor_dropped,
        pending_hidden,
        hits: page.len() as u64,
        fts_ms,
        embed_ms,
        embed_tokens,
    });
    let mut out = json!({
        "context": {"id": snap.row.id, "title": snap.row.title, "jira": snap.row.jira},
        "query": query,
        "mode": mode,
        "records": records,
        "markdown": markdown,
    });
    if let Some(code) = semantic_error {
        out["semantic_error"] = json!(code);
    }
    Ok(Output::success(out))
}

/// What the vector leg of a search found.
enum Semantic {
    /// No vectors for this Context and model: the embedder is never called.
    None,
    /// Vectors exist but could not be used; the error code.
    Failed(String),
    /// The semantic list over the visible records.
    /// With the embed time in ms and the embed tokens.
    List(Vec<Scored>, u64, Option<u64>),
}

/// What the vector leg reads.
struct SemanticInput<'a> {
    home: &'a crate::home::CpdHome,
    config: &'a crate::config::Config,
    runner: &'a std::sync::Arc<dyn crate::host::runner::Runner>,
    context: &'a ContextId,
    query: &'a str,
    clock: &'a std::sync::Arc<dyn crate::clock::Clock>,
}

/// The vector leg: the visible records' vectors, then the embedded query against them.
/// A foreign or newer vectors.db is an error; any other store or embedder failure degrades to FTS-only.
fn semantic(input: &SemanticInput<'_>, visible: &[String]) -> Result<Semantic, CpdError> {
    let ids: Vec<RecordId> = visible.iter().filter_map(|id| id.parse().ok()).collect();
    let vectors = match VectorStore::at(input.home).get(input.context, EMBED_MODEL, &ids) {
        Ok(vectors) => vectors,
        Err(e)
            if e.details().contains_key("user_version")
                || e.details().contains_key("application_id") =>
        {
            return Err(e);
        }
        Err(e) => return Ok(Semantic::Failed(e.code().as_str().to_owned())),
    };
    if vectors.is_empty() {
        return Ok(Semantic::None);
    }
    let started = input.clock.now();
    let embedded = bifrost_embedder(input.config, input.runner)
        .and_then(|embedder| embedder.embed(input.query, DIMS));
    let embed_ms = input
        .clock
        .now()
        .saturating_duration_since(started)
        .as_millis() as u64;
    let (query, tokens) = match embedded {
        Ok(embedded) => (embedded.vector, embedded.tokens),
        Err(e) => return Ok(Semantic::Failed(e.code().as_str().to_owned())),
    };
    let vectors: Vec<(String, Vec<f32>)> = vectors
        .into_iter()
        .map(|(id, v)| (id.to_string(), v))
        .collect();
    Ok(Semantic::List(
        semantic_list(&query, &vectors),
        embed_ms,
        tokens,
    ))
}

/// The kinds to keep: the four searchable kinds by default, else the named ones.
fn kinds(names: &[String]) -> Result<HashSet<&'static str>, CpdError> {
    if names.is_empty() {
        return Ok([RecordKind::Decision, RecordKind::Note]
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

/// One mapped, filtered hit, in the engine's order (before the tie-break sort).
struct Hit {
    id: String,
    short: String,
    kind: &'static str,
    at: String,
    seq: i64,
    score: f64,
    in_force: bool,
    topic: Option<String>,
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
            "topic": self.topic,
        })
    }

    /// The `- [<id>] <short>` line, with the out-of-force labels of `context show`.
    fn line(&self, snap: &Snapshot) -> String {
        let mut line = match &self.topic {
            Some(topic) => format!("- [{}] [{topic}] {}", self.id, self.short),
            None => format!("- [{}] {}", self.id, self.short),
        };
        if let Some(successor) = snap.successor.get(&self.id) {
            line.push_str(&format!(" (superseded by {successor})"));
        }
        if let Some(label) = snap.retired.get(&self.id) {
            line.push_str(&format!(" [retired ({label})]"));
        }
        line
    }
}

/// Keeps the ranked hits that map to a live row of a selected kind, in force (unless `all`)
/// and not hidden by a pending duplicate or superseded proposal (unless `all`, or named by id).
fn filter(
    ranked: Vec<Scored>,
    snap: &Snapshot,
    kinds: &HashSet<&'static str>,
    all: bool,
    named: Option<&str>,
) -> Vec<Scored> {
    ranked
        .into_iter()
        .filter(|hit| {
            let Some((_, record)) = snap
                .records
                .iter()
                .find(|(_, record)| record.id.to_string() == hit.id)
            else {
                return false;
            };
            // `--all` governs supersession and retirement, which only the force
            // kinds have; a `plan`/`brief` is never in force and is shown
            // whenever its kind is selected.
            if !all && !snap.in_force.contains(&hit.id) && force_kind(record.kind()) {
                return false;
            }
            if !all && snap.hidden.contains(&hit.id) && named != Some(hit.id.as_str()) {
                return false;
            }
            kinds.contains(record.kind().as_str())
        })
        .collect()
}

/// Maps the kept hits to rows and breaks exact ties by `seq` descending.
fn hits_of(kept: Vec<Scored>, snap: &Snapshot) -> Vec<Hit> {
    let mut hits: Vec<Hit> = kept
        .into_iter()
        .filter_map(|hit| {
            let (seq, record) = snap
                .records
                .iter()
                .find(|(_, record)| record.id.to_string() == hit.id)?;
            Some(Hit {
                short: short(&hit.id, record, &snap.curations),
                kind: record.kind().as_str(),
                at: record.at.clone(),
                seq: *seq,
                score: hit.score,
                in_force: snap.in_force.contains(&hit.id),
                topic: topic_of(snap, &hit.id),
                id: hit.id,
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
    matches!(kind, RecordKind::Decision | RecordKind::Note)
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
