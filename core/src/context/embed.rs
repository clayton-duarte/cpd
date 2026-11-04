//! `cpd context embed`: backfills one Context's in-force decisions and notes into `vectors.db`.
#![allow(dead_code)] // stubs until the implementation Job

use clap::Args;

use super::Select;
use super::curate_vectors::VectorStore;
use crate::embedding::EmbeddingPort;
use crate::error::CpdError;
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
}

/// Embeds the `records` (newest first, with their text) whose stored content hash differs, at most `limit`.
pub(super) fn embed_records(
    _store: &VectorStore,
    _embedder: &dyn EmbeddingPort,
    _context: &ContextId,
    _records: &[(RecordId, String)],
    _limit: usize,
) -> Result<EmbedReport, CpdError> {
    todo!("embed_records")
}

/// `context embed`.
pub(super) fn embed(_args: EmbedArgs) -> Result<Output, CpdError> {
    let _ = (&_args.select, _args.limit);
    todo!("context embed")
}
