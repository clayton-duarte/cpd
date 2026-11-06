//! The `github` source: a stub until its Workflow (swr.md §14).

use time::OffsetDateTime;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::error::CpdError;
use crate::swr::outbox::{Coalesce, Mutation};

pub struct Github;

impl Source for Github {
    fn system(&self) -> &'static str {
        "github"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["github"]
    }
    fn due(&self, _cx: &mut Cx<'_>, _now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        Ok(Vec::new())
    }
    fn fetch(
        &self,
        _cx: &mut Cx<'_>,
        _keys: &[QueryKey],
        _poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        Ok(())
    }
    fn execute(&self, _cx: &mut Cx<'_>, _m: &Mutation) -> ExecOutcome {
        ExecOutcome::Failed {
            error: "not implemented".into(),
        }
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}

/// The real `github` source (swr-github.md §4). Stub: every body is `todo!()` until the swr-github Workflow.
#[derive(Default)]
pub struct GithubSource {
    /// The policy's `pr.excused_labels` (pro-d447): a PR carrying one has its failing `danger` check excused.
    pub excused_labels: Vec<String>,
}

impl GithubSource {
    /// One engine pass: plan, one `gh api graphql` call through the Runner, commit key by key (swr-github.md §4).
    pub fn pass(&self, _cx: &mut Cx<'_>, _now: OffsetDateTime) -> Result<(), CpdError> {
        todo!("swr-github: pass")
    }
}

impl Source for GithubSource {
    fn system(&self) -> &'static str {
        "github"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["github"]
    }
    fn due(&self, _cx: &mut Cx<'_>, _now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        todo!("swr-github: due")
    }
    fn fetch(
        &self,
        _cx: &mut Cx<'_>,
        _keys: &[QueryKey],
        _poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        todo!("swr-github: fetch")
    }
    fn execute(&self, _cx: &mut Cx<'_>, _m: &Mutation) -> ExecOutcome {
        todo!("swr-github: execute")
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        todo!("swr-github: coalesce")
    }
}
