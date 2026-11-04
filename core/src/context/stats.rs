//! `cpd context stats` (commands.md §3.20; Plan `ctx-observability`, pro-d13): sizes, writing and curation, and
//! cost of a Context. The `retrieval` section is zeroed here (filled by `obs-impl-ret`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use clap::Args;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::Select;
use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::ContextId;
use crate::output::Output;

#[derive(Args, Debug)]
pub struct StatsArgs {
    #[command(flatten)]
    select: Select,
    /// Every live Context
    #[arg(long, conflicts_with = "context")]
    all: bool,
    /// Count only what happened at or after this ISO 8601 UTC time
    #[arg(long, value_name = "iso")]
    since: Option<String>,
}

fn io_failed(path: &Path, e: &io::Error) -> CpdError {
    CpdError::new(
        ErrorCode::FileUnreadable,
        format!("Cannot read {}: {e}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// The JSON lines of `path`; an absent file is no lines and a line that is not JSON is skipped (commands.md §3.20).
fn read_lines(path: &Path) -> Result<Vec<Value>, CpdError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(Value::is_object)
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(io_failed(path, &e)),
    }
}

/// Whether the line's time (`key`, RFC 3339) is at or after `since`; a line without a readable time is outside a window.
fn in_window(line: &Value, key: &str, since: Option<OffsetDateTime>) -> bool {
    let Some(since) = since else { return true };
    line[key]
        .as_str()
        .and_then(|t| OffsetDateTime::parse(t, &Rfc3339).ok())
        .is_some_and(|t| t >= since)
}

fn num(line: &Value, key: &str) -> i64 {
    line[key].as_i64().unwrap_or(0)
}

fn sum(lines: &[&Value], key: &str) -> i64 {
    lines.iter().map(|l| num(l, key)).sum()
}

fn sum_cost(lines: &[&Value]) -> f64 {
    lines.iter().filter_map(|l| l["cost"].as_f64()).sum()
}

fn sized(count: i64, bytes: i64) -> Value {
    json!({"count": count, "bytes": bytes})
}

fn sizes_of(conn: &Connection, home: &CpdHome, context: &ContextId) -> Result<Value, CpdError> {
    let ctx = context.as_str();
    let mut stmt =
        conn.prepare("SELECT id, kind, text, supersedes FROM context_records WHERE context = ?1")?;
    let rows = stmt
        .query_map([ctx], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let retirements = |status: &str| -> Result<HashSet<String>, CpdError> {
        let mut stmt = conn
            .prepare("SELECT record FROM record_retirements WHERE context = ?1 AND status = ?2")?;
        Ok(stmt
            .query_map(params![ctx, status], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?)
    };
    let approved = retirements("approved")?;
    let pending = retirements("pending")?;
    let superseded: HashSet<&str> = rows
        .iter()
        .filter_map(|(_, _, _, s)| s.as_deref())
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .collect();
    let mut by_kind: HashMap<&str, (i64, i64)> = HashMap::new();
    let mut by_status: HashMap<&str, (i64, i64)> = HashMap::new();
    for (id, kind, text, _) in &rows {
        let bytes = text.len() as i64;
        let status = if approved.contains(id) {
            "retired_approved"
        } else if superseded.contains(id.as_str()) {
            "superseded"
        } else if pending.contains(id) {
            "retired_pending"
        } else {
            "in_force"
        };
        for (map, key) in [(&mut by_kind, kind.as_str()), (&mut by_status, status)] {
            let e = map.entry(key).or_default();
            e.0 += 1;
            e.1 += bytes;
        }
    }
    let pick = |keys: &[&str], map: &HashMap<&str, (i64, i64)>| -> Value {
        Value::Object(
            keys.iter()
                .map(|k| {
                    let (c, b) = map.get(k).copied().unwrap_or_default();
                    (k.to_string(), sized(c, b))
                })
                .collect(),
        )
    };
    let (curated, short_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(CAST(short AS BLOB))), 0) FROM record_curations WHERE context = ?1",
        [ctx],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let provisional: i64 = conn.query_row(
        "SELECT COUNT(*) FROM record_topics t WHERE t.context = ?1 AND NOT EXISTS
           (SELECT 1 FROM record_curations c WHERE c.context = t.context AND c.record = t.record)",
        [ctx],
        |r| r.get(0),
    )?;
    Ok(json!({
        "records": {
            "by_kind": pick(&["decision", "note", "plan", "brief"], &by_kind),
            "by_status": pick(&["in_force", "superseded", "retired_approved", "retired_pending"], &by_status),
        },
        "curated": {"count": curated, "short_bytes": short_bytes},
        "provisional_topics": provisional,
        "vectors": vectors_of(home, ctx)?,
    }))
}

/// `vectors.db` opened read-only; `None` when the file is absent.
fn open_vectors(home: &CpdHome) -> Result<Option<Connection>, CpdError> {
    let path = home.vectors_db();
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?))
}

fn has_table(conn: &Connection, name: &str) -> Result<bool, CpdError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn vectors_of(home: &CpdHome, ctx: &str) -> Result<Value, CpdError> {
    let zero = json!({"count": 0, "checked": 0, "blob_bytes": 0});
    let Some(conn) = open_vectors(home)? else {
        return Ok(zero);
    };
    if !has_table(&conn, "context_embeddings")? || !has_table(&conn, "context_checked")? {
        return Ok(zero);
    }
    let (count, blob): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(vector)), 0) FROM context_embeddings WHERE context = ?1",
        [ctx],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let checked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM context_checked WHERE context = ?1",
        [ctx],
        |r| r.get(0),
    )?;
    Ok(json!({"count": count, "checked": checked, "blob_bytes": blob}))
}

/// `{searches, embed_tokens, embed_ms, fts_ms}` from `search_log` from `since` on; zeros without the table.
fn search_cost(home: &CpdHome, ctx: &str, since: Option<&str>) -> Result<Value, CpdError> {
    let zero = json!({"searches": 0, "embed_tokens": 0, "embed_ms": 0, "fts_ms": 0});
    let Some(conn) = open_vectors(home)? else {
        return Ok(zero);
    };
    if !has_table(&conn, "search_log")? {
        return Ok(zero);
    }
    let (n, tokens, embed_ms, fts_ms): (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(embed_tokens), 0), COALESCE(SUM(embed_ms), 0), COALESCE(SUM(fts_ms), 0)
           FROM search_log WHERE context = ?1 AND (?2 IS NULL OR at >= ?2)",
        params![ctx, since],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok(json!({"searches": n, "embed_tokens": tokens, "embed_ms": embed_ms, "fts_ms": fts_ms}))
}

fn writing_of(
    conn: &Connection,
    ctx: &str,
    since: Option<&str>,
    distill: &[&Value],
    curate: &[&Value],
) -> Result<Value, CpdError> {
    let records_written: i64 = conn.query_row(
        "SELECT COUNT(*) FROM context_records WHERE context = ?1 AND (?2 IS NULL OR at >= ?2)",
        params![ctx, since],
        |r| r.get(0),
    )?;
    let rejected: usize = distill
        .iter()
        .filter_map(|l| l["rejected"].as_array())
        .map(Vec::len)
        .sum();
    let failed = distill.iter().filter(|l| l["result"] == "failed").count();
    let mut retirements = serde_json::Map::new();
    for reason in ["duplicate", "superseded", "documented"] {
        let mut outcome = json!({"approved": 0, "pending": 0, "rejected": 0});
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM record_retirements
              WHERE context = ?1 AND reason = ?2 AND (?3 IS NULL OR proposed_at >= ?3) GROUP BY status",
        )?;
        let rows = stmt
            .query_map(params![ctx, reason, since], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (status, n) in rows {
            outcome[status.as_str()] = json!(n);
        }
        retirements.insert(reason.to_owned(), outcome);
    }
    Ok(json!({
        "records_written": records_written,
        "distill": {"runs": distill.len(), "curations": sum(distill, "curations"), "retire": sum(distill, "retire"),
                    "rejected": rejected, "failed": failed},
        "curate": {"runs": curate.len(), "free": sum(curate, "free"), "judged": sum(curate, "judged"),
                   "retire": sum(curate, "retire"), "contradicts": sum(curate, "contradicts"),
                   "disagreed": sum(curate, "disagreed"), "quote_failed": sum(curate, "quote_failed"),
                   "kev_unreachable": sum(curate, "kev_unreachable")},
        "retirements": retirements,
    }))
}

/// The report of one Context over the store `conn`, `home`'s run files and `vectors.db`, from `since` (an ISO
/// 8601 UTC time) on; `sizes` is a snapshot.
pub fn compute(
    conn: &Connection,
    home: &CpdHome,
    context: &ContextId,
    since: Option<&str>,
) -> Result<Value, CpdError> {
    let ctx = context.as_str();
    let since_at = since
        .map(|s| OffsetDateTime::parse(s, &Rfc3339))
        .transpose()
        .map_err(|_| bad_since(since.unwrap_or_default()))?;
    let since_ts = since_at.map(clock::timestamp);
    let since_ts = since_ts.as_deref();

    let runs: Vec<Value> = read_lines(&super::distill::runs_path(home, context))?;
    let distill: Vec<&Value> = runs
        .iter()
        .filter(|l| in_window(l, "started_at", since_at))
        .collect();
    let curate_all: Vec<Value> =
        read_lines(&home.context_digest_dir(context).join("curate-runs.jsonl"))?;
    let in_win: Vec<&Value> = curate_all
        .iter()
        .filter(|l| in_window(l, "at", since_at))
        .collect();
    let of_kind = |kind: &str| -> Vec<&Value> {
        in_win
            .iter()
            .copied()
            .filter(|l| l["kind"] == kind)
            .collect()
    };
    let (curate, embed) = (of_kind("curate"), of_kind("embed"));

    let distill_ms: i64 = distill
        .iter()
        .map(|l| (l["seconds"].as_f64().unwrap_or(0.0) * 1000.0).round() as i64)
        .sum();
    let (distill_cost, curate_cost) = (sum_cost(&distill), sum_cost(&curate));
    let search = search_cost(home, ctx, since_ts)?;
    let (distill_in, distill_out) = (sum(&distill, "tokens_in"), sum(&distill, "tokens_out"));
    let (curate_ms, embed_ms) = (sum(&curate, "ms"), sum(&embed, "ms"));
    let embed_tokens = sum(&curate, "embed_tokens")
        + sum(&embed, "embed_tokens")
        + search["embed_tokens"].as_i64().unwrap_or(0);
    Ok(json!({
        "context": ctx,
        "sizes": sizes_of(conn, home, context)?,
        "writing_and_curation": writing_of(conn, ctx, since_ts, &distill, &curate)?,
        "cost": {
            "distill": {"runs": distill.len(), "tokens_in": distill_in, "tokens_out": distill_out,
                        "cost": distill_cost, "ms": distill_ms},
            "curate": {"runs": curate.len(), "kev_tokens_in": sum(&curate, "kev_tokens_in"),
                       "kev_tokens_out": sum(&curate, "kev_tokens_out"),
                       "judge_tokens_in": sum(&curate, "judge_tokens_in"),
                       "judge_tokens_out": sum(&curate, "judge_tokens_out"),
                       "cost": curate_cost, "ms": curate_ms},
            "embed": {"runs": embed.len(), "embed_tokens": sum(&embed, "embed_tokens"), "ms": embed_ms},
            "search": search,
            "total": {
                "tokens_in": distill_in + sum(&curate, "kev_tokens_in") + sum(&curate, "judge_tokens_in"),
                "tokens_out": distill_out + sum(&curate, "kev_tokens_out") + sum(&curate, "judge_tokens_out"),
                "embed_tokens": embed_tokens,
                "cost": distill_cost + curate_cost,
                "ms": distill_ms + curate_ms + embed_ms,
            },
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

fn bad_since(text: &str) -> CpdError {
    CpdError::new(
        ErrorCode::Usage,
        format!("--since {text:?} is not an ISO-8601 instant with a zone (Z or ±hh:mm)."),
    )
    .with("since", text)
}

fn markdown(reports: &[Value]) -> String {
    let mut out = String::new();
    for r in reports {
        let (s, w, c) = (&r["sizes"], &r["writing_and_curation"], &r["cost"]);
        out.push_str(&format!("## {}\n\n", r["context"].as_str().unwrap_or("")));
        out.push_str("### Sizes\n\n| group | count | bytes |\n|---|---|---|\n");
        for group in ["by_kind", "by_status"] {
            if let Some(map) = s["records"][group].as_object() {
                for (k, v) in map {
                    out.push_str(&format!("| {k} | {} | {} |\n", v["count"], v["bytes"]));
                }
            }
        }
        out.push_str(&format!(
            "\n- curated: {} ({} short bytes); provisional topics: {}\n- vectors: {} ({} checked, {} bytes)\n\n",
            s["curated"]["count"], s["curated"]["short_bytes"], s["provisional_topics"],
            s["vectors"]["count"], s["vectors"]["checked"], s["vectors"]["blob_bytes"],
        ));
        out.push_str(&format!(
            "### Writing and curation\n\n- records written: {}\n- distill: {}\n- curate: {}\n- retirements: {}\n\n",
            w["records_written"], w["distill"], w["curate"], w["retirements"],
        ));
        out.push_str("### Cost\n\n| step | figures |\n|---|---|\n");
        if let Some(map) = c.as_object() {
            for (k, v) in map {
                out.push_str(&format!("| {k} | {v} |\n"));
            }
        }
        out.push_str(&format!("\n### Retrieval\n\n{}\n\n", r["retrieval"]));
    }
    out
}

pub(super) fn stats(args: StatsArgs) -> Result<Output, CpdError> {
    let Env {
        home, mut store, ..
    } = Env::open()?;
    if let Some(since) = &args.since {
        OffsetDateTime::parse(since, &Rfc3339).map_err(|_| bad_since(since))?;
    }
    let ids = if args.all {
        store.read(|conn| {
            let mut stmt =
                conn.prepare("SELECT id FROM contexts WHERE deleted_at IS NULL ORDER BY id")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .map(|id| ContextId::parse(&id?))
                .collect::<Result<Vec<_>, CpdError>>()
        })?
    } else {
        vec![args.select.resolve(&mut store)?]
    };
    let reports = store.read(|conn| {
        ids.iter()
            .map(|id| compute(conn, &home, id, args.since.as_deref()))
            .collect::<Result<Vec<_>, _>>()
    })?;
    let markdown = markdown(&reports);
    Ok(Output::success(json!({
        "since": args.since,
        "contexts": reports,
        "markdown": markdown,
    })))
}
