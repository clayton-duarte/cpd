//! The non-blocking pull request poll (E5): one thread runs the poll; the tick never waits for it.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use cpd_core::clock::Clock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ports::Ports;

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
    /// [`PollRun::start_on`] the system's clock.
    #[cfg(test)]
    pub(super) fn start(job: impl FnOnce() -> Result<Value, Failure> + Send + 'static) -> PollRun {
        PollRun::start_on(Arc::new(cpd_core::clock::SystemClock), job)
    }

    /// Runs `job` on a thread of its own; the poll ended when `clock` says so.
    pub(super) fn start_on(clock: Arc<dyn Clock>, job: impl FnOnce() -> Result<Value, Failure> + Send + 'static) -> PollRun {
        let (tx, rx) = channel();
        let ended_by = Arc::clone(&clock);
        // The job reads `Ports::current()`: this thread's scope is not the poll thread's.
        let ports = Ports::current();
        let spawned = std::thread::Builder::new().name("cpd-pr-poll".to_owned()).spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| Ports::scope(&ports, job))).unwrap_or_else(|payload| {
                let text = payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned());
                Err(Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("The pull request poll panicked: {}.", text.unwrap_or_else(|| "no message".to_owned())))))
            });
            let _ = tx.send(Finished { result, ended: ended_by.now() });
        });
        if let Err(e) = spawned {
            // The thread never ran: the failure is the poll's result, as for a panic.
            let (tx, rx) = channel();
            let failure = Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start the pull request poll thread: {e}.")));
            let _ = tx.send(Finished { result: Err(failure), ended: clock.now() });
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
