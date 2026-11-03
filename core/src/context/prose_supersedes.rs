//! The record ids a text names after `supersedes`, `overrules` or `replaces`.

use crate::ids::RecordId;

/// The ids named after a supersession verb, in text order, each once. Stub: finds none.
#[allow(dead_code)]
pub(crate) fn prose_supersession_targets(_text: &str) -> Vec<RecordId> {
    Vec::new()
}
