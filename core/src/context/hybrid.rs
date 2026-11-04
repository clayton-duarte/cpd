//! The hybrid seam of `cpd context search`: the semantic list and reciprocal rank fusion (stubs).
#![allow(dead_code)]

use super::fts::Scored;

/// A record is a semantic hit only when its cosine similarity with the query is at least this.
pub(super) const SEM_FLOOR: f64 = 0.60;
/// At most this many semantic hits.
pub(super) const SEM_TOP: usize = 25;

/// Cosine list of `vectors` against `query`, best first; other dimensions and cosines below [`SEM_FLOOR`] are dropped.
pub(super) fn semantic_list(query: &[f32], vectors: &[(String, Vec<f32>)]) -> Vec<Scored> {
    let _ = (query, vectors);
    todo!("semantic_list")
}

/// Reciprocal rank fusion of the lexical and semantic lists; `first` is moved to the front when present.
pub(super) fn rrf(lexical: &[Scored], semantic: &[Scored], first: Option<&str>) -> Vec<Scored> {
    let _ = (lexical, semantic, first);
    todo!("rrf")
}
