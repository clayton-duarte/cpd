//! The `model` domain (contract §8.3 "Models"): the four Tier models `O`, `S`, `H`, `G`, each a model ref
//! `<provider>/<model-id>`; one provider per resolved table. STUB: the bodies are `todo!()`.

use std::path::Path;

use crate::error::CpdError;

/// The four resolved model refs of a policy.
///
/// The resolved policy hands them out through [`Models::of`] (not a field of `Policy`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Models {
    pub o: String,
    pub s: String,
    pub h: String,
    pub g: String,
}

impl Models {
    /// The models of a resolved policy.
    pub fn of(_policy: &super::Policy) -> Models {
        todo!("the model domain is not implemented")
    }
}

/// The provider of a model ref: the text before its first `/`.
pub fn provider_of(_model_ref: &str) -> &str {
    todo!("the model domain is not implemented")
}

/// Refuses a table whose four refs do not share one provider: `policy_invalid`, `details.file` the
/// file that set the winning `[model]`, `details.key` `model`, the message naming the keys and providers.
pub fn check_one_provider(_models: &Models, _file: &Path) -> Result<(), CpdError> {
    todo!("the model domain is not implemented")
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
