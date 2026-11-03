//! Write-time curation: neighbour lookup (stub; its Job fills it).
#![allow(unused_variables)]

use super::curate::Candidate;
use crate::error::CpdError;
use crate::ids::RecordId;

pub const K: usize = 8;
pub const TOP: usize = 25;
pub const RRF_C: f64 = 60.0;

pub fn forced_ids(
    text: &str,
    supersedes: &[RecordId],
    record: &RecordId,
    in_force: &[Candidate],
) -> Vec<RecordId> {
    todo!()
}

pub fn lexical_rank(
    text: &str,
    in_force: &[Candidate],
    top: usize,
) -> Result<Vec<RecordId>, CpdError> {
    todo!()
}

pub fn semantic_rank(query: &[f32], vectors: &[(RecordId, Vec<f32>)], top: usize) -> Vec<RecordId> {
    todo!()
}

pub fn fuse(lists: &[Vec<RecordId>]) -> Vec<RecordId> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Neighbour {
    pub id: RecordId,
    pub forced: bool,
}

pub fn neighbours(
    record: &RecordId,
    forced: Vec<RecordId>,
    lexical: Vec<RecordId>,
    semantic: Option<Vec<RecordId>>,
) -> Vec<Neighbour> {
    todo!()
}
