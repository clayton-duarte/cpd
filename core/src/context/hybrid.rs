//! The hybrid seam of `cpd context search`: the semantic list and reciprocal rank fusion.

use std::collections::BTreeMap;

use super::curate_neighbours::{RRF_C, cosine};
use super::fts::Scored;

/// A record is a semantic hit only when its cosine similarity with the query is at least this.
pub(super) const SEM_FLOOR: f64 = 0.60;
/// At most this many semantic hits.
pub(super) const SEM_TOP: usize = 25;

/// Cosine list of `vectors` against `query`, best first; other dimensions and cosines below [`SEM_FLOOR`] are dropped.
pub(super) fn semantic_list(query: &[f32], vectors: &[(String, Vec<f32>)]) -> Vec<Scored> {
    let mut list: Vec<Scored> = vectors
        .iter()
        .filter_map(|(id, v)| {
            let score = cosine(query, v)?;
            (score >= SEM_FLOOR).then(|| Scored {
                id: id.clone(),
                score,
            })
        })
        .collect();
    list.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    list.truncate(SEM_TOP);
    list
}

/// Reciprocal rank fusion of the lexical and semantic lists; `first` is moved to the front when present.
pub(super) fn rrf(lexical: &[Scored], semantic: &[Scored], first: Option<&str>) -> Vec<Scored> {
    let mut score: BTreeMap<&str, f64> = BTreeMap::new();
    for list in [lexical, semantic] {
        for (i, hit) in list.iter().enumerate() {
            *score.entry(hit.id.as_str()).or_default() += 1.0 / (RRF_C + (i + 1) as f64);
        }
    }
    let mut fused: Vec<Scored> = score
        .into_iter()
        .map(|(id, score)| Scored {
            id: id.to_owned(),
            score,
        })
        .collect();
    fused.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    if let Some(first) = first
        && let Some(at) = fused.iter().position(|s| s.id == first)
    {
        let hit = fused.remove(at);
        fused.insert(0, hit);
    }
    fused
}
