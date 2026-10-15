//! The engine, `cpd2 watch` (contract §3.7, §6.6; commands.md §12).
//! Filled in by Job `rs-watch`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

pub type Args = Pending;

pub fn run(_args: Args) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("watch"))
}
