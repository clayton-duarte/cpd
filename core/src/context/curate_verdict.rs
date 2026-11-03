//! Write-time curation: the kind gate, the kev-4b screen and the judge confirmation rule.

use super::curate::{Judged, Verdict};
use crate::ids::{RecordId, RecordKind};

/// The confidence kev-4b needs to accept an independent pair.
pub const GATE: f32 = 0.80;

/// What the pipeline does with a record of a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Skip,
    LabelOnly,
    Neighbours,
}

pub fn gate(_kind: RecordKind) -> Gate {
    todo!()
}

/// kev-4b's answer for one pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub verdict: Verdict,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screened {
    AcceptIndependent,
    Escalate(Option<Screen>),
}

pub fn screen(_kev: Result<Screen, ()>) -> Screened {
    todo!()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Duplicate,
    Superseded,
}

/// Why a pair was made independent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    Disagreed,
    QuoteNotFound,
    Rejected(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Independent {
        why: Option<Why>,
    },
    Retire {
        record: RecordId,
        target: RecordId,
        reason: Reason,
        topic: String,
        short: String,
    },
    Contradicts {
        neighbour: RecordId,
        quote: String,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Side<'a> {
    pub id: &'a RecordId,
    pub seq: i64,
    pub text: &'a str,
}

pub fn confirm(_new: Side, _neighbour: Side, _kev: Option<Screen>, _judged: Judged) -> Outcome {
    todo!()
}
