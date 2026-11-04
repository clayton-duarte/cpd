//! E9, write-time curation tick: starts `cpd context curate` per Context (contract §3.7 E9). Stub.

use time::OffsetDateTime;

use super::engine::Engine;
use cpd_core::ids::ContextId;

pub(super) fn run(engine: &mut Engine, contexts: &[ContextId], now: OffsetDateTime) {
    let _ = (&engine.curating, &engine.curate_backoff, contexts, now);
}
