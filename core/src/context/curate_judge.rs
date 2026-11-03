//! Write-time curation: the Bifrost judge (stub; its Job fills it). The real one runs `node <pi adapter>/lib/judge.ts`
//! through the [`Runner`]; the helper resolves the key, which is never in argv, env, output or error.

use std::sync::Arc;

use super::curate::Judged;
use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::runner::Runner;
use crate::ids::{RecordId, RecordKind};

/// A record as the judge sees it.
#[derive(Clone, Debug)]
pub struct RecordView<'a> {
    pub id: &'a RecordId,
    pub kind: RecordKind,
    pub at: &'a str,
    pub topic: Option<&'a str>,
    pub text: &'a str,
}

/// One question to the judge; `neighbour: None` is a label request (topic and short only).
#[derive(Clone, Debug)]
pub struct JudgeRequest<'a> {
    pub new: RecordView<'a>,
    pub neighbour: Option<RecordView<'a>>,
}

pub trait JudgePort: Send + Sync {
    fn judge(&self, request: &JudgeRequest) -> Result<Judged, CpdError>;
}

pub struct BifrostJudge {
    _home: CpdHome,
    _config: Config,
    _runner: Arc<dyn Runner>,
}

impl BifrostJudge {
    pub fn new(home: &CpdHome, config: &Config, runner: Arc<dyn Runner>) -> Self {
        BifrostJudge {
            _home: home.clone(),
            _config: config.clone(),
            _runner: runner,
        }
    }
}

impl JudgePort for BifrostJudge {
    fn judge(&self, _request: &JudgeRequest) -> Result<Judged, CpdError> {
        todo!("BifrostJudge::judge")
    }
}
