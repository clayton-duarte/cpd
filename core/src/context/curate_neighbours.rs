//! Write-time curation: neighbour lookup (forced ids, FTS5 lexical rank, cosine, fusion).

use super::curate::Candidate;
use std::collections::{BTreeMap, HashSet};

use rusqlite::Connection;

use crate::error::{CpdError, ErrorCode};
use crate::ids::RecordId;

pub const K: usize = 8;
pub const TOP: usize = 25;
pub const RRF_C: f64 = 60.0;

/// Terms of a text: lowercase alphanumeric runs.
fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn store_failed(e: rusqlite::Error) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("neighbour lexical rank: {e}"),
    )
}

/// In-force ids that must be judged: the supersedes targets first, then ids named in the text.
pub fn forced_ids(
    text: &str,
    supersedes: &[RecordId],
    record: &RecordId,
    in_force: &[Candidate],
) -> Vec<RecordId> {
    let live: HashSet<&RecordId> = in_force.iter().map(|c| &c.id).collect();
    let named = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter_map(|t| RecordId::parse(t).ok());
    let mut seen = HashSet::new();
    supersedes
        .iter()
        .cloned()
        .chain(named)
        .filter(|id| id != record && live.contains(id) && seen.insert(id.clone()))
        .collect()
}

/// BM25 rank over the in-force texts, using the `RARE` rarest terms of `text`.
pub fn lexical_rank(
    text: &str,
    in_force: &[Candidate],
    top: usize,
) -> Result<Vec<RecordId>, CpdError> {
    const RARE: usize = 20;
    let mut query: Vec<String> = terms(text);
    query.sort();
    query.dedup();
    if query.is_empty() || top == 0 {
        return Ok(Vec::new());
    }
    let docs: Vec<HashSet<String>> = in_force
        .iter()
        .map(|c| terms(&c.text).into_iter().collect())
        .collect();
    let freq = |t: &String| docs.iter().filter(|d| d.contains(t)).count();
    let mut counted: Vec<(usize, String)> = query.into_iter().map(|t| (freq(&t), t)).collect();
    counted.sort();
    counted.truncate(RARE);
    let expr = counted
        .iter()
        .map(|(_, t)| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ");

    let conn = Connection::open_in_memory().map_err(store_failed)?;
    conn.execute_batch("CREATE VIRTUAL TABLE n USING fts5(id UNINDEXED, text)")
        .map_err(store_failed)?;
    {
        let mut ins = conn
            .prepare("INSERT INTO n(id, text) VALUES (?1, ?2)")
            .map_err(store_failed)?;
        for c in in_force {
            ins.execute((c.id.to_string(), &c.text))
                .map_err(store_failed)?;
        }
    }
    let mut stmt = conn
        .prepare("SELECT id FROM n WHERE n MATCH ?1 ORDER BY bm25(n), id LIMIT ?2")
        .map_err(store_failed)?;
    let rows = stmt
        .query_map((expr, top as i64), |r| r.get::<_, String>(0))
        .map_err(store_failed)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(RecordId::parse(&row.map_err(store_failed)?)?);
    }
    Ok(out)
}

/// Cosine rank of `vectors` against `query`; vectors of another dimension are skipped.
pub fn semantic_rank(query: &[f32], vectors: &[(RecordId, Vec<f32>)], top: usize) -> Vec<RecordId> {
    let norm = |v: &[f32]| {
        v.iter()
            .map(|x| f64::from(*x) * f64::from(*x))
            .sum::<f64>()
            .sqrt()
    };
    let qn = norm(query);
    let mut scored: Vec<(f64, &RecordId)> = vectors
        .iter()
        .filter(|(_, v)| v.len() == query.len())
        .filter_map(|(id, v)| {
            let vn = norm(v);
            if qn == 0.0 || vn == 0.0 {
                return None;
            }
            let dot: f64 = query
                .iter()
                .zip(v)
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum();
            Some((dot / (qn * vn), id))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(top)
        .map(|(_, id)| id.clone())
        .collect()
}

/// Reciprocal rank fusion (constant [`RRF_C`]); ties break by id.
pub fn fuse(lists: &[Vec<RecordId>]) -> Vec<RecordId> {
    let mut score: BTreeMap<&RecordId, f64> = BTreeMap::new();
    for list in lists {
        for (i, id) in list.iter().enumerate() {
            *score.entry(id).or_default() += 1.0 / (RRF_C + (i + 1) as f64);
        }
    }
    let mut v: Vec<(&RecordId, f64)> = score.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v.into_iter().map(|(id, _)| id.clone()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Neighbour {
    pub id: RecordId,
    pub forced: bool,
}

/// Forced ids first, then the fused ranking, never the record itself, at most [`K`].
pub fn neighbours(
    record: &RecordId,
    forced: Vec<RecordId>,
    lexical: Vec<RecordId>,
    semantic: Option<Vec<RecordId>>,
) -> Vec<Neighbour> {
    let mut lists = vec![lexical];
    lists.extend(semantic);
    let mut seen = HashSet::new();
    let forced = forced
        .into_iter()
        .filter(|id| id != record && seen.insert(id.clone()))
        .map(|id| Neighbour { id, forced: true })
        .collect::<Vec<_>>();
    let ranked = fuse(&lists)
        .into_iter()
        .filter(|id| id != record && seen.insert(id.clone()))
        .map(|id| Neighbour { id, forced: false });
    forced.into_iter().chain(ranked).take(K).collect()
}
