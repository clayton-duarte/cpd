//! `known.md` of a distill step (contract §4.6 step 3): the curated records in force, narrowed to the
//! neighbours of the batch above [`KNOWN_FULL_MAX`] of them.

// Stub: distill.rs does not call it yet; remove this allow when it does.
#![allow(dead_code)]

use super::distill::State;
use super::render::Indexed;
use crate::error::CpdError;

/// Up to this many curated records in force, `known.md` lists them all.
pub(crate) const KNOWN_FULL_MAX: usize = 80;

/// The neighbours kept for each record of the batch above [`KNOWN_FULL_MAX`].
pub(crate) const KNOWN_K: usize = 8;

/// The text of `known.md` for `batch` over the records of `state`.
pub(crate) fn known_md(_state: &State, _batch: &[&Indexed]) -> Result<String, CpdError> {
    todo!()
}
