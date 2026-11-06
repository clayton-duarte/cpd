//! The shapes of a provider's cached list (contract §4.4 `swr_models`, docs/architecture/swr-models.md §2): the diff of
//! the last change, the catalog of the last good list and what one write of a provider did. The rows are read and
//! written by `swr::source::models`.

use serde::{Deserialize, Serialize};

use super::helper::ProviderFailure;

/// The last change: the diff of the write that made it (`swr_models.changes_json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    pub at: String,
    pub appeared: Vec<String>,
    pub went_away: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// `copilot-available` for pi's API list, `openai-models` for a `GET <baseUrl>/models`.
    pub kind: String,
    pub auth: String,
    pub base: Option<String>,
}

/// `swr_models.catalog_json`: the last good list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub pi_version: String,
    pub source: Source,
    pub ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known: Option<Vec<String>>,
}

/// A provider written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub provider: String,
    pub polled_at: String,
    pub count: usize,
    /// The diff of this write when it is not empty.
    pub changes: Option<Changes>,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Written(Written),
    Failed(ProviderFailure),
}
