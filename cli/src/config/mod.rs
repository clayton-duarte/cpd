//! Configuration and Tiers (contract §8, §9; commands.md §14.2, §14.3):
//! `config.toml`, `cpd2 config` and `cpd2 tiers`. Filled in by Job `rs-config`.

use crate::cli::Pending;
use crate::error::CpdError;
use crate::output::Output;

pub type ConfigArgs = Pending;
pub type TiersArgs = Pending;

/// `cpd2 config` (§14.2).
pub fn config(_args: ConfigArgs) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("config"))
}

/// `cpd2 tiers` (§14.3).
pub fn tiers(_args: TiersArgs) -> Result<Output, CpdError> {
    Err(CpdError::not_implemented("tiers"))
}
