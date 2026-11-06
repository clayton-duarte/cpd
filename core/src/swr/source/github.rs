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
