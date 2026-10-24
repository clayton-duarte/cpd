//! The non-blocking PR poll (E5): one thread runs the poll; the tick never waits for it.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

use serde_json::Value;

use cpd_core::error::{CpdError, ErrorCode};

use super::report::Failure;

/// A poll that ended: its result and when it ended.
pub(super) struct Finished {
    pub result: Result<Value, Failure>,
    pub ended: Instant,
}

/// One running poll.
pub(super) struct PollRun {
    result: Option<Receiver<Finished>>,
}

impl PollRun {
    pub(super) fn start(job: impl FnOnce() -> Result<Value, Failure> + Send + 'static) -> PollRun {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("cpd-pr-poll".to_owned()).spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(job)).unwrap_or_else(|payload| {
                let text = payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned());
                Err(Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("The PR poll panicked: {}.", text.unwrap_or_else(|| "no message".to_owned())))))
            });
            let _ = tx.send(Finished { result, ended: Instant::now() });
        });
        if let Err(e) = spawned {
            // The thread never ran: the failure is the poll's result, as for a panic.
            let (tx, rx) = channel();
            let failure = Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start the PR poll thread: {e}.")));
            let _ = tx.send(Finished { result: Err(failure), ended: Instant::now() });
            return PollRun { result: Some(rx) };
        }
        PollRun { result: Some(rx) }
    }

    /// Non-blocking; `Some` once, when the job has ended.
    pub(super) fn try_take(&mut self) -> Option<Finished> {
        match self.result.as_ref()?.try_recv() {
            Ok(finished) => {
                self.result = None;
                Some(finished)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.result = None;
                None
            }
        }
    }

    /// Blocks until the job ended; `None` when the result was already taken.
    pub(super) fn wait(&mut self) -> Option<Finished> {
        self.result.take()?.recv().ok()
    }
}
