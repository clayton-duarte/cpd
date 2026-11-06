//! The Source trait and the registry (swr.md §11). The four sources are stubs until their Workflows.

use std::time::Duration;

use time::OffsetDateTime;

use super::outbox::{Coalesce, Mutation};
use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ports::Ports;
use crate::store::Store;

pub mod github;
pub mod github_write;
pub mod jira;
#[cfg(test)]
mod jira_adopt_tests;
#[cfg(test)]
mod jira_tests;
pub mod models;
pub mod slack;
#[cfg(test)]
mod slack_tests;
#[cfg(test)]
mod slack_write_tests;

#[cfg(test)]
mod github_poll_tests;
#[cfg(test)]
mod github_tests;
#[cfg(test)]
mod github_write_tests;
#[cfg(test)]
mod models_tests;

/// A query key `<system>:<kind>:<id>` (swr.md §4).
pub type QueryKey = String;

/// What `Source::execute` reports for one claimed mutation (swr.md §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecOutcome {
    Done,
    Retry { after: Duration },
    Failed { error: String },
}

/// What a source reads and writes through: the store, the ports (Runner, Clock), the home and the config.
pub struct Cx<'a> {
    pub store: &'a mut Store,
    pub ports: &'a Ports,
    pub home: &'a CpdHome,
    pub config: &'a Config,
}

/// One datasource (swr.md §11).
pub trait Source: Sync {
    fn system(&self) -> &'static str;
    fn endpoints(&self) -> &'static [&'static str];
    /// The keys that are stale or invalidated, from the rows and the in-memory fresh-until instants.
    fn due(&self, cx: &mut Cx<'_>, now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError>;
    /// At most one call per endpoint per pass.
    fn fetch(
        &self,
        cx: &mut Cx<'_>,
        keys: &[QueryKey],
        poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError>;
    fn execute(&self, cx: &mut Cx<'_>, m: &Mutation) -> ExecOutcome;
    fn coalesce(&self, op_kind: &str) -> Coalesce;
}

/// The registered sources: a new source is one line here.
pub fn registry() -> &'static [&'static dyn Source] {
    &[&github::Github, &slack::Slack, &jira::Jira, &models::Models]
}
