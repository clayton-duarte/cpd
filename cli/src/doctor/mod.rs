//! `cpd2 doctor` (commands.md §14.1). Filled in by Job `rs-doctor`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

pub type Args = Pending;

pub fn run(_args: Args) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("doctor"))
}
