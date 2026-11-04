//! `known.md` of a distill step (contract §4.6 step 3): the curated records in force, narrowed to the
//! neighbours of the batch above [`KNOWN_FULL_MAX`] of them.

use std::collections::{BTreeSet, HashSet};

use super::curate::Candidate;
use super::curate_neighbours::{TOP, forced_ids, lexical_rank, neighbours};
use super::distill::State;
use super::render::Indexed;
use crate::error::CpdError;
use crate::ids::RecordId;

/// Up to this many curated records in force, `known.md` lists them all.
pub(crate) const KNOWN_FULL_MAX: usize = 80;

/// The neighbours kept for each record of the batch above [`KNOWN_FULL_MAX`].
pub(crate) const KNOWN_K: usize = 8;

/// The text of `known.md` for `batch` over the records of `state`: every topic, then the curated records in
/// force, all of them up to [`KNOWN_FULL_MAX`], else only the neighbours of the batch.
pub(crate) fn known_md(state: &State, batch: &[&Indexed]) -> Result<String, CpdError> {
    let curated: Vec<(&Indexed, &str, &str)> = state
        .in_force()
        .into_iter()
        .filter_map(|r| {
            let (topic, short) = state.curated.get(&r.1.id)?;
            Some((r, topic.as_str(), short.as_str()))
        })
        .collect();
    let topics: BTreeSet<&str> = curated.iter().map(|(_, t, _)| *t).collect();
    let mut out: String = topics.iter().map(|t| format!("{t}\n")).collect();
    out.push('\n');
    let keep: Option<HashSet<RecordId>> = (curated.len() > KNOWN_FULL_MAX)
        .then(|| neighbour_ids(&curated, batch))
        .transpose()?;
    for ((_, record), topic, short) in &curated {
        if keep.as_ref().is_none_or(|k| k.contains(&record.id)) {
            out.push_str(&format!("[{}] {topic} | {short}\n", record.id));
        }
    }
    Ok(out)
}

/// The ids of the union of the neighbours of each record of `batch` among `curated`.
fn neighbour_ids(
    curated: &[(&Indexed, &str, &str)],
    batch: &[&Indexed],
) -> Result<HashSet<RecordId>, CpdError> {
    let candidates: Vec<Candidate> = curated
        .iter()
        .map(|((seq, r), topic, short)| Candidate {
            id: r.id.clone(),
            seq: *seq,
            kind: r.kind(),
            at: r.at.clone(),
            text: format!("{topic} {short} {}", r.text),
        })
        .collect();
    let mut keep = HashSet::new();
    for (_, record) in batch {
        let supersedes: Vec<RecordId> = record.supersedes().to_vec();
        let forced = forced_ids(&record.text, &supersedes, &record.id, &candidates);
        let lexical = lexical_rank(&record.text, &candidates, TOP)?;
        keep.extend(
            neighbours(&record.id, forced, lexical, None)
                .into_iter()
                .take(KNOWN_K)
                .map(|n| n.id),
        );
    }
    Ok(keep)
}
