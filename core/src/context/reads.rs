//! Read log of Context records (decision pro-d10 item 5): stubs, the implementation is another Job's.

use crate::error::CpdError;
use crate::ids::{ContextId, RecordId, SessionId};
use crate::store::Store;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    Pushed,
    Search,
    Show,
    Shadow,
}

#[allow(dead_code)]
impl Channel {
    pub(crate) fn as_str(self) -> &'static str {
        todo!()
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Read {
    pub(crate) record: RecordId,
    pub(crate) rank: Option<u32>,
    pub(crate) score: Option<f64>,
}

#[allow(dead_code)]
pub(crate) fn search_reads(_hits: &[(RecordId, f64)]) -> Vec<Read> {
    todo!()
}

#[allow(dead_code)]
pub(crate) fn log_reads_for(
    _store: &mut Store,
    _context: &ContextId,
    _session: &SessionId,
    _channel: Channel,
    _reads: &[Read],
    _at: &str,
) -> Result<usize, CpdError> {
    todo!()
}

#[allow(dead_code)]
pub(crate) fn mark_used_for(
    _store: &mut Store,
    _context: &ContextId,
    _session: &SessionId,
    _text: &str,
    _exclude: &[RecordId],
) -> Result<usize, CpdError> {
    todo!()
}

#[allow(dead_code)]
pub(crate) fn env_session(_store: &mut Store) -> Option<SessionId> {
    todo!()
}

#[allow(dead_code)]
pub(crate) fn log_reads(
    _store: &mut Store,
    _context: &ContextId,
    _channel: Channel,
    _reads: &[Read],
) {
    todo!()
}

#[allow(dead_code)]
pub(crate) fn mark_used(
    _store: &mut Store,
    _context: &ContextId,
    _text: &str,
    _exclude: &[RecordId],
) {
    todo!()
}
