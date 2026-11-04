//! `cpd context stats` (commands.md §3.20; Plan `ctx-observability`, pro-d13). STUB: a zeroed report, nothing computed.

use clap::Args;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::Select;
use crate::env::Env;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::ContextId;
use crate::output::Output;

#[derive(Args, Debug)]
pub struct StatsArgs {
    #[command(flatten)]
    select: Select,
    /// Every live Context
    #[arg(long)]
    all: bool,
    /// Count only what happened at or after this ISO 8601 UTC time
    #[arg(long, value_name = "iso")]
    since: Option<String>,
}

fn kinds(keys: &[&str], leaf: &Value) -> Value {
    Value::Object(keys.iter().map(|k| (k.to_string(), leaf.clone())).collect())
}

/// The report of one Context over the store `conn`, `home`'s run files and `vectors.db`, from `since` (an ISO
/// 8601 UTC time) on; `sizes` is a snapshot. STUB: zeroed.
pub fn compute(
    _conn: &Connection,
    _home: &CpdHome,
    context: &ContextId,
    _since: Option<&str>,
) -> Result<Value, CpdError> {
    let sized = json!({"count": 0, "bytes": 0});
    let outcome = json!({"approved": 0, "pending": 0, "rejected": 0});
    Ok(json!({
        "context": context.as_str(),
        "sizes": {
            "records": {
                "by_kind": kinds(&["decision", "note", "plan", "brief"], &sized),
                "by_status": kinds(&["in_force", "superseded", "retired_approved", "retired_pending"], &sized),
            },
            "curated": {"count": 0, "short_bytes": 0},
            "provisional_topics": 0,
            "vectors": {"count": 0, "checked": 0, "blob_bytes": 0},
        },
        "writing_and_curation": {
            "records_written": 0,
            "distill": {"runs": 0, "curations": 0, "retire": 0, "rejected": 0, "failed": 0},
            "curate": {"runs": 0, "free": 0, "judged": 0, "retire": 0, "contradicts": 0, "disagreed": 0,
                       "quote_failed": 0, "kev_unreachable": 0},
            "retirements": kinds(&["duplicate", "superseded", "documented"], &outcome),
        },
        "cost": {
            "distill": {"runs": 0, "tokens_in": 0, "tokens_out": 0, "cost": 0, "ms": 0},
            "curate": {"runs": 0, "kev_tokens_in": 0, "kev_tokens_out": 0, "judge_tokens_in": 0,
                       "judge_tokens_out": 0, "cost": 0, "ms": 0},
            "embed": {"runs": 0, "embed_tokens": 0, "ms": 0},
            "search": {"searches": 0, "embed_tokens": 0, "embed_ms": 0, "fts_ms": 0},
            "total": {"tokens_in": 0, "tokens_out": 0, "embed_tokens": 0, "cost": 0, "ms": 0},
        },
        "retrieval": {
            "reads": {"pushed": 0, "search": 0, "show": 0},
            "reading_sessions": 0,
            "cited": {"used": 0, "conversion": 0},
            "unread_in_force": 0,
            "searches": {"total": 0, "hybrid": 0, "fts": 0, "zero_hit": 0, "recent_zero_hit_queries": [],
                         "fts_ms": {"mean": null, "max": 0}, "embed_ms": {"mean": null, "max": 0}},
        },
    }))
}

pub(super) fn stats(args: StatsArgs) -> Result<Output, CpdError> {
    let Env {
        home, mut store, ..
    } = Env::open()?;
    let id = args.select.resolve(&mut store)?;
    let report = store.read(|conn| compute(conn, &home, &id, args.since.as_deref()))?;
    Ok(Output::success(json!({
        "since": args.since,
        "contexts": [report],
        "markdown": "",
    })))
}
