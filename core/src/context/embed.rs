//! `cpd context embed`: backfills one Context's in-force decisions and notes into `vectors.db`.

use clap::Args;
use serde_json::json;

use super::curate_run::{DIMS, EMBED_MODEL, append_run, bifrost_embedder};
use super::curate_vectors::{VectorStore, content_hash};
use super::distill::State;
use super::{ContextRow, Select};
use crate::clock::Clock;
use crate::embedding::EmbeddingPort;
use crate::env::Env;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::output::Output;

#[derive(Args, Debug)]
pub struct EmbedArgs {
    #[command(flatten)]
    select: Select,
    /// Embed at most this many records in one run (0 = all)
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

/// What one backfill call did.
#[derive(Debug, PartialEq)]
pub(super) struct EmbedReport {
    pub embedded: usize,
    pub skipped: usize,
    pub remaining: usize,
    pub embed_tokens: u64,
    pub ms: u64,
}

/// Embeds the `records` (newest first, with their text) whose stored content hash differs, at most `limit`.
/// Each row is written as soon as it is embedded; the first embedder error stops the call.
pub(super) fn embed_records(
    store: &VectorStore,
    embedder: &dyn EmbeddingPort,
    context: &ContextId,
    records: &[(RecordId, String)],
    limit: usize,
) -> Result<EmbedReport, CpdError> {
    let ids: Vec<RecordId> = records.iter().map(|(id, _)| id.clone()).collect();
    let have = store.hashes(context, EMBED_MODEL, &ids)?;
    let mut report = EmbedReport {
        embedded: 0,
        skipped: 0,
        remaining: 0,
        embed_tokens: 0,
        ms: 0,
    };
    for (id, text) in records {
        let hash = content_hash(text);
        if have.get(id) == Some(&hash) {
            report.skipped += 1;
        } else if limit != 0 && report.embedded >= limit {
            report.remaining += 1;
        } else {
            let e = embedder.embed(text, DIMS)?;
            store.put_hashed(context, &e.model, id, &e.vector, &hash)?;
            report.embed_tokens += e.tokens.unwrap_or(0);
            report.embedded += 1;
        }
    }
    Ok(report)
}

/// [`embed_records`] timed by `clock`, with its `curate-runs.jsonl` line appended (best effort).
pub(super) fn embed_run(
    home: &CpdHome,
    clock: &dyn Clock,
    store: &VectorStore,
    embedder: &dyn EmbeddingPort,
    context: &ContextId,
    records: &[(RecordId, String)],
    limit: usize,
) -> Result<EmbedReport, CpdError> {
    let started = clock.now();
    let mut report = embed_records(store, embedder, context, records, limit)?;
    report.ms = clock.now().saturating_duration_since(started).as_millis() as u64;
    append_run(
        home,
        context,
        json!({
            "kind": "embed",
            "at": crate::clock::timestamp(clock.utc()),
            "context": context.as_str(),
            "ms": report.ms,
            "embed_tokens": report.embed_tokens,
            "kev_tokens_in": 0,
            "kev_tokens_out": 0,
            "judge_tokens_in": 0,
            "judge_tokens_out": 0,
            "cost": null,
            "embedded": report.embedded,
            "skipped": report.skipped,
            "remaining": report.remaining,
        }),
    );
    Ok(report)
}

/// `context embed`.
pub(super) fn embed(args: EmbedArgs) -> Result<Output, CpdError> {
    let Env {
        home,
        config,
        mut store,
        ports,
        ..
    } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    store.read(|conn| ContextRow::require(conn, &context))?;
    let state = store.read(|conn| State::read(conn, &context))?;
    let records: Vec<(RecordId, String)> = state
        .in_force_records()
        .into_iter()
        .rev()
        .map(|(_, r)| (r.id.clone(), r.text.clone()))
        .collect();
    let embedder = bifrost_embedder(&home, &config, &ports.runner)?;
    let vectors = VectorStore::at(&home);
    let report = embed_run(
        &home,
        ports.clock.as_ref(),
        &vectors,
        &embedder,
        &context,
        &records,
        args.limit,
    )?;
    Ok(Output::success(json!({
        "context": context.as_str(),
        "model": EMBED_MODEL,
        "embedded": report.embedded,
        "skipped": report.skipped,
        "remaining": report.remaining,
        "ms": report.ms,
        "embed_tokens": report.embed_tokens,
        "cost": serde_json::Value::Null,
    })))
}
