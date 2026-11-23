//! Pending trusted, fenced native transcript staging boundary.
#![allow(dead_code)]

use crate::error::CpdError;
use crate::ids::SessionId;
use crate::store::Store;

pub(super) struct StageNativeArgs {
    pub session_id: SessionId,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StageResult {
    Staged {
        conversation_id: String,
        source_sha256: String,
    },
    Refused,
}

// Private transport seam. The real implementation must construct this request only
// after resolving and fencing the authoritative association; it is not an API.
pub(super) struct StageRequest {
    pub session_id: SessionId,
    pub source_bytes: Vec<u8>,
    pub source_sha256: String,
    pub forked_from: Option<SessionId>,
}

pub(super) trait StageHost {
    fn stage(&mut self, request: StageRequest) -> StageResult;
}

pub(super) fn stage_native(_args: StageNativeArgs) -> Result<StageResult, CpdError> {
    todo!("trusted acquisition and owned host channel")
}

pub(super) fn stage_native_with_host(
    _store: &Store,
    _args: StageNativeArgs,
    _host: &mut dyn StageHost,
) -> Result<StageResult, CpdError> {
    todo!("trusted acquisition and owned host channel")
}
