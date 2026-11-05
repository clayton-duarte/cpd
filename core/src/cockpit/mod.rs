//! `cpd cockpit serve` (commands.md §11a, contract §5.5): the cockpit's snapshot and event stream, in process.
//!
//! The loop is driven by an input iterator, not by the clock or a thread, so a test runs it frame by frame:
//! an [`Input::Tick`] is one `POLL_MS` of virtual time (the loop polls the [`ChangeSource`] once and closes a batch
//! whose first change is `COALESCE_MS` or more old), an [`Input::Line`] is one line of stdin, and the end of the
//! iterator is end of file of stdin: serving ends `Ok(())` and writes nothing more (a batch still open is dropped).
//! Every frame is one line of `out`, flushed when produced. This file holds stubs only: the implementation is
//! the next Job's.

use std::io::Write;

use serde_json::Value;

use crate::env::Env;
use crate::error::CpdError;

/// The interval of the `data_version` poll (contract §5.5).
pub const POLL_MS: u64 = 10;
/// The window in which changes merge into one snapshot (contract §5.5).
pub const COALESCE_MS: u64 = 16;

/// One step of the serving loop.
pub enum Input {
    /// A line of stdin, without its newline.
    Line(String),
    /// `POLL_MS` of virtual time has passed: poll the [`ChangeSource`] once.
    Tick,
}

/// What reports that the store changed: the real one reads `PRAGMA data_version`; a test scripts it.
pub trait ChangeSource {
    /// `Some(new data_version)` when the store changed since the last poll, else `None`.
    fn poll(&mut self) -> Option<i64>;
}

/// A frame of stdout (commands.md §11a "Frames out").
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Hello,
    Snapshot {
        seq: u64,
        panes: Value,
    },
    Event(Value),
    PlanShow {
        id: String,
        value: Value,
    },
    Error {
        id: Option<String>,
        code: String,
        message: String,
    },
}

impl Frame {
    /// The frame as one line of JSON with its newline.
    pub fn to_line(&self) -> String {
        todo!("cockpit::Frame::to_line")
    }
}

/// Serves until `input` ends. `Err` is a store error that ends the process (its last frame is an `error` frame).
pub fn serve(
    _env: &mut Env,
    _input: &mut dyn Iterator<Item = Input>,
    _out: &mut dyn Write,
    _changes: &mut dyn ChangeSource,
) -> Result<(), CpdError> {
    todo!("cockpit::serve")
}

#[cfg(test)]
mod tests;
