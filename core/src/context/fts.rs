//! The in-memory FTS5 search seam: ranks `Doc`s for a typed query and drops the weak tail.

// Stubs until the implementation Job: nothing calls the seam yet.
#![allow(dead_code)]

use crate::error::CpdError;

/// One searchable record: its id and the curated topic and short ("" when uncurated) and the text.
pub(super) struct Doc {
    pub id: String,
    pub topic: String,
    pub short: String,
    pub text: String,
}

/// A ranked hit: higher `score` is better.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Scored {
    pub id: String,
    pub score: f64,
}

/// A hit scoring below `FLOOR` times the top score is dropped.
pub(super) const FLOOR: f64 = 0.35;

/// Ranks `docs` for `typed`, best first.
pub(super) fn rank(_docs: &[Doc], _typed: &str) -> Result<Vec<Scored>, CpdError> {
    todo!()
}

/// Drops the weak tail of `ranked`, keeping the `exempt` id.
pub(super) fn floor(_ranked: Vec<Scored>, _exempt: Option<&str>) -> Vec<Scored> {
    todo!()
}
