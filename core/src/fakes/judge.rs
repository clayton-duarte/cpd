//! An in-process [`JudgePort`] for the tests of the curation pipeline: scripted answers, no process, no thread.

use std::sync::Mutex;
use std::time::Duration;

use crate::clock::ManualClock;

use crate::context::curate::Judged;
use crate::context::curate_judge::{JudgePort, JudgeRequest, RecordView};
use crate::error::{CpdError, ErrorCode};
use crate::ids::{RecordId, RecordKind};

/// Virtual time a fake port spends per call: advances `clock` by `step` on each call.
#[derive(Clone)]
pub struct Tick {
    pub clock: ManualClock,
    pub step: Duration,
}

impl std::fmt::Debug for Tick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tick({:?})", self.step)
    }
}

impl Tick {
    pub fn fire(tick: &Option<Tick>) {
        if let Some(t) = tick {
            t.clock.advance(t.step);
        }
    }
}

/// An owned copy of a [`RecordView`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedRecord {
    pub id: RecordId,
    pub kind: RecordKind,
    pub at: String,
    pub topic: Option<String>,
    pub text: String,
}

impl OwnedRecord {
    fn of(view: &RecordView<'_>) -> OwnedRecord {
        OwnedRecord {
            id: view.id.clone(),
            kind: view.kind,
            at: view.at.to_owned(),
            topic: view.topic.map(str::to_owned),
            text: view.text.to_owned(),
        }
    }
}

/// An owned copy of a [`JudgeRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedRequest {
    pub new: OwnedRecord,
    pub neighbour: Option<OwnedRecord>,
}

/// A [`JudgePort`] that answers each call from its script, in order, and records the requests.
#[derive(Debug, Default)]
pub struct FakeJudge {
    replies: Mutex<Vec<Result<Judged, CpdError>>>,
    always: Option<Judged>,
    calls: Mutex<Vec<OwnedRequest>>,
    tick: Option<Tick>,
}

impl FakeJudge {
    /// A fake with scripted replies, used in order; a call past the script is an `internal` error.
    pub fn new(replies: Vec<Result<Judged, CpdError>>) -> FakeJudge {
        let mut replies = replies;
        replies.reverse();
        FakeJudge {
            replies: Mutex::new(replies),
            always: None,
            calls: Mutex::new(Vec::new()),
            tick: None,
        }
    }

    /// A fake whose every call answers `judged`.
    pub fn answering(judged: Judged) -> FakeJudge {
        FakeJudge {
            replies: Mutex::new(Vec::new()),
            always: Some(judged),
            calls: Mutex::new(Vec::new()),
            tick: None,
        }
    }

    /// Each call advances the fake clock by `step`.
    pub fn advancing(mut self, clock: &ManualClock, step: Duration) -> FakeJudge {
        self.tick = Some(Tick {
            clock: clock.clone(),
            step,
        });
        self
    }

    /// Every request received, in order, as an owned copy.
    pub fn calls(&self) -> Vec<OwnedRequest> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl JudgePort for FakeJudge {
    fn judge(&self, request: &JudgeRequest) -> Result<Judged, CpdError> {
        Tick::fire(&self.tick);
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(OwnedRequest {
                new: OwnedRecord::of(&request.new),
                neighbour: request.neighbour.as_ref().map(OwnedRecord::of),
            });
        }
        let next = self.replies.lock().ok().and_then(|mut r| r.pop());
        match (next, &self.always) {
            (Some(reply), _) => reply,
            (None, Some(judged)) => Ok(judged.clone()),
            (None, None) => Err(CpdError::new(
                ErrorCode::Internal,
                "The fake judge has no scripted reply left.",
            )),
        }
    }
}
