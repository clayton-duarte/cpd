//! The words and colours of a pull request's state, derived once for the plan box and the Dispatch row.

use super::{PrState, UpdateState};
use crate::palette::Meaning;

/// What the look of a pull request depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrFacts {
    pub state: PrState,
    pub draft: bool,
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

pub fn pr_look(_facts: &PrFacts) -> PrLook {
    todo!()
}
