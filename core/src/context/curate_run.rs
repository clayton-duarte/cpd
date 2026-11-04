//! `context curate` (commands.md §3.13a): the write-time neighbour curation of one record per run.

use std::collections::HashSet;

use clap::Args;
use serde_json::Value;
use time::OffsetDateTime;

use super::Select;
use super::distill::State;
use crate::classifier::ClassifierPort;
use crate::config::Config;
use crate::context::curate_judge::JudgePort;
use crate::embedding::EmbeddingPort;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::output::Output;
use crate::store::Store;

/// Records older than this are never picked without `--record`.
pub const WINDOW_HOURS: i64 = 24;
pub const DIMS: usize = 768;
/// Missing vectors embedded per run, newest first.
pub const FILL: usize = 25;

pub struct Models<'a> {
    pub classifier: &'a dyn ClassifierPort,
    pub embedding: &'a dyn EmbeddingPort,
    pub judge: &'a dyn JudgePort,
}

/// The oldest in-force record of kind decision, note, question or answer (gate != Skip), not in `checked`, written
/// within [`WINDOW_HOURS`] before `now` (RFC 3339 `at`); `None` when there is none.
pub fn next_record(
    _state: &State,
    _checked: &HashSet<RecordId>,
    _now: OffsetDateTime,
) -> Option<RecordId> {
    todo!()
}

pub fn curate_with(
    _store: &mut Store,
    _home: &CpdHome,
    _config: &Config,
    _context: &ContextId,
    _models: &Models,
    _now: OffsetDateTime,
    _only: Option<&RecordId>,
) -> Result<Value, CpdError> {
    todo!()
}

#[derive(Args, Debug)]
pub struct CurateArgs {
    #[command(flatten)]
    #[allow(dead_code)]
    select: Select,
    /// Curate this record, whatever its age
    #[arg(long, value_name = "id")]
    #[allow(dead_code)]
    record: Option<RecordId>,
}

/// `context curate` (§3.13a).
pub(super) fn curate(_args: CurateArgs) -> Result<Output, CpdError> {
    todo!()
}
