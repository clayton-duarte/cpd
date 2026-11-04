//! Write-time neighbour curation (experimental, `experimental.write_curation`): the types its parts share.
//!
//! The parts are [`curate_vectors`](super::curate_vectors), [`curate_neighbours`](super::curate_neighbours),
//! [`curate_verdict`](super::curate_verdict), [`curate_backoff`](super::curate_backoff) and
//! [`curate_judge`](super::curate_judge).

use serde::{Deserialize, Serialize};

use crate::ids::{RecordId, RecordKind};

/// How a new record relates to a neighbour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Duplicate,
    Supersedes,
    Contradicts,
    Independent,
}

/// An in-force record as the pipeline sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: RecordId,
    pub seq: i64,
    pub kind: RecordKind,
    pub at: String,
    pub text: String,
}

/// What the judge answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judged {
    pub verdict: Verdict,
    pub quote: String,
    pub reason: String,
    pub topic: String,
    pub short: String,
}
