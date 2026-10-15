//! The Attempt driver (contract §3.4): `attempt run` (A1–A9), the driver lock
//! and its hand-over, and `job dispatch`. Filled in by Job `rs-driver`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

/// The arguments of `cpd2 attempt run` (commands.md §6.4).
pub type RunArgs = Pending;
/// The arguments of `cpd2 job dispatch` (commands.md §5.5).
pub type DispatchArgs = Pending;

/// `cpd2 attempt run` (internal).
pub fn run(_args: RunArgs) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("attempt run"))
}

/// `cpd2 job dispatch`.
pub fn dispatch(_args: DispatchArgs) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("job dispatch"))
}
