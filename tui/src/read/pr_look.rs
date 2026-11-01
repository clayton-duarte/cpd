//! The words and colours of a pull request's state, derived once for the plan box and the Dispatch row.

use super::{PrState, UpdateState};
use crate::palette::{self, Meaning};

/// What the look of a pull request depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrFacts {
    pub state: PrState,
    pub draft: bool,
    /// Ready to merge (pro-d118: open, not draft, merge_state clean, base is the default branch), not GitHub's CLEAN alone.
    pub clean: bool,
    pub conflicting: bool,
    pub owned: bool,
    pub update: Option<UpdateState>,
    pub asked: bool,
}

/// The state word and, for an open conflicting PR, the conflict word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrLook {
    pub state: (&'static str, Meaning),
    pub conflict: Option<(&'static str, Meaning)>,
}

pub fn pr_look(facts: &PrFacts) -> PrLook {
    let state = match facts.state {
        PrState::Open if facts.draft => ("draft", Meaning::Planning),
        PrState::Open => ("open", palette::pr_open(facts.clean)),
        PrState::Closed if facts.asked => palette::PR_CLOSED_ASKED,
        PrState::Closed => ("closed", palette::pr_state(PrState::Closed)),
        PrState::Merged => ("merged", palette::pr_state(PrState::Merged)),
    };
    let conflict = (facts.state == PrState::Open && facts.conflicting).then_some(match facts.update {
        Some(UpdateState::Running) => palette::PR_UPDATING_RUNNING,
        Some(UpdateState::Queued) => palette::PR_UPDATING_QUEUED,
        None if facts.owned => palette::PR_CONFLICT_OWNED,
        None => palette::PR_CONFLICT,
    });
    PrLook { state, conflict }
}
