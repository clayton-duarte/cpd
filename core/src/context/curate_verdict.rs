//! Write-time curation: the kind gate, the kev-4b screen and the judge confirmation rule.

use super::curate::{Judged, Verdict};
use crate::ids::{RecordId, RecordKind};

/// The confidence kev-4b needs to accept an independent pair.
pub const GATE: f32 = 0.80;

/// What the pipeline does with a record of a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Skip,
    Neighbours,
}

pub fn gate(kind: RecordKind) -> Gate {
    match kind {
        RecordKind::Decision | RecordKind::Note => Gate::Neighbours,
        _ => Gate::Skip,
    }
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

pub fn screen(kev: Result<Screen, ()>) -> Screened {
    match kev {
        Ok(k) if k.verdict == Verdict::Independent && k.confidence >= GATE => {
            Screened::AcceptIndependent
        }
        Ok(k) => Screened::Escalate(Some(k)),
        Err(()) => Screened::Escalate(None),
    }
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

pub fn confirm(new: Side, neighbour: Side, kev: Option<Screen>, judged: Judged) -> Outcome {
    let independent = |why| Outcome::Independent { why };
    if judged.verdict == Verdict::Independent {
        return independent(None);
    }
    if kev.is_some_and(|k| k.verdict != judged.verdict) {
        return independent(Some(Why::Disagreed));
    }
    let quote = judged.quote.trim();
    if quote.is_empty() || !(new.text.contains(quote) || neighbour.text.contains(quote)) {
        return independent(Some(Why::QuoteNotFound));
    }
    let (record, target, reason) = match judged.verdict {
        Verdict::Contradicts => {
            return Outcome::Contradicts {
                neighbour: neighbour.id.clone(),
                quote: judged.quote,
                reason: judged.reason,
            };
        }
        Verdict::Duplicate if new.seq > neighbour.seq => (new.id, neighbour.id, Reason::Duplicate),
        Verdict::Duplicate if new.seq < neighbour.seq => (neighbour.id, new.id, Reason::Duplicate),
        Verdict::Supersedes if new.seq > neighbour.seq => {
            (neighbour.id, new.id, Reason::Superseded)
        }
        _ => {
            return independent(Some(Why::Rejected(
                "the retired record must be the later one (duplicate) or the older one (superseded)"
                    .into(),
            )));
        }
    };
    Outcome::Retire {
        record: record.clone(),
        target: target.clone(),
        reason,
        topic: judged.topic,
        short: judged.short,
    }
}
