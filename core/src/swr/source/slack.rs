//! The `slack` source: a stub until its Workflow (swr.md §14).

use time::OffsetDateTime;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::error::CpdError;
use crate::swr::outbox::{Coalesce, Mutation};

pub struct Slack;

impl Source for Slack {
    fn system(&self) -> &'static str {
        "slack"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["slack"]
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
        // The red tests call `fetch` directly; `due` stays empty so E24 never reaches this stub (swr-slack.md §3, §6).
        todo!("swr-slack: run the helper with stdin, apply the posts, confirm own posts")
    }
    fn execute(&self, _cx: &mut Cx<'_>, _m: &Mutation) -> ExecOutcome {
        todo!("swr-slack: reaction, post and repost (swr-slack.md §4)")
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}
