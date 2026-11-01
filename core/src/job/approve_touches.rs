//! `job approve-touches` (commands.md §5.9a, contract §2.2 "Job kinds", §7.5; hs-n641): the Lead lets a Job change a
//! path outside its Spec `touches`. Sets `jobs.touches_approved` to the union of its old value and the paths; not a Locked
//! field, in no fingerprint. The Event is `job-edited` with `fields: ["touches_approved"]`.
//!
//! Stub: `guard-tests` writes the red tests and names this entry and its flags; `guard-impl` implements the command (and
//! the migration that adds `jobs.touches_approved`).

use super::{ApproveTouchesArgs, Env};
use crate::error::CpdError;
use crate::output::Output;

pub(super) fn approve(args: ApproveTouchesArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    approve_in(&mut env, args)
}

/// [`approve`] on an open `env`: what a test hands in.
pub(super) fn approve_in(_env: &mut Env, _args: ApproveTouchesArgs) -> Result<Output, CpdError> {
    unimplemented!("job approve-touches is guard-impl's (hs-n641): the red tests of guard-tests pin it")
}
