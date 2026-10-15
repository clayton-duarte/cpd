//! Escalations (contract §3.5; commands.md §8): `ask` and `answer`.
//! Filled in by Job `rs-ask`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

pub type AskArgs = Pending;
pub type AnswerArgs = Pending;

/// `cpd2 ask` (§8.1).
pub fn ask(_args: AskArgs) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("ask"))
}

/// `cpd2 answer` (§8.2).
pub fn answer(_args: AnswerArgs) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("answer"))
}
