//! The `plan` view (commands.md §13.2, §17): not implemented yet; its Job
//! replaces this module.

use crate::error::CpdError;
use crate::output::Output;

pub(super) fn run() -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("tui view plan"))
}
