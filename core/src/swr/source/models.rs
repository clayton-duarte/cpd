//! The `models` source: a stub until its Workflow (swr.md §14).

use time::OffsetDateTime;

use serde_json::Value;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::error::CpdError;
use crate::models::{Answer, Changes, Outcome};
use crate::store::Store;
use crate::swr::outbox::{Coalesce, Mutation};

pub struct Models;

impl Source for Models {
    fn system(&self) -> &'static str {
        "models"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["models"]
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

/// Writes the rows of one validated helper answer, one `Store::write` per provider (swr-models.md §2.1).
pub fn apply_answer(
    _store: &mut Store,
    _answer: &Answer,
    _now: OffsetDateTime,
) -> Result<Vec<Outcome>, CpdError> {
    todo!("swr-models.md §2.1")
}

/// `cpd models show`: `{providers, warnings}` read from the rows; writes nothing (swr-models.md §5).
pub fn show(
    _store: &mut Store,
    _provider: Option<&str>,
    _now: OffsetDateTime,
) -> Result<Value, CpdError> {
    todo!("swr-models.md §5")
}

/// The rows whose `changes_json.at` is within `models::WINDOW` of `now` and whose announced part is not empty.
pub fn changed(
    _store: &mut Store,
    _now: OffsetDateTime,
) -> Result<Vec<(String, Changes)>, CpdError> {
    todo!("swr-models.md §5")
}
