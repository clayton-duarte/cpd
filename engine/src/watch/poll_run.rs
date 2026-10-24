//! The non-blocking PR poll (E5): one thread runs the poll; the tick never waits for it.
#![allow(dead_code)]

use std::time::Instant;

use serde_json::Value;

use super::report::Failure;

/// A poll that ended: its result and when it ended.
pub(super) struct Finished {
    pub result: Result<Value, Failure>,
    pub ended: Instant,
}

/// One running poll.
pub(super) struct PollRun {}

impl PollRun {
    pub(super) fn start(job: impl FnOnce() -> Result<Value, Failure> + Send + 'static) -> PollRun {
        let _ = job;
        todo!()
    }

    /// Non-blocking; `Some` once, when the job has ended.
    pub(super) fn try_take(&mut self) -> Option<Finished> {
        todo!()
    }

    /// Blocks until the job ended; `None` when the result was already taken.
    pub(super) fn wait(&mut self) -> Option<Finished> {
        todo!()
    }
}
