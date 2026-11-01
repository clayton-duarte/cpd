//! An in-process [`ContextEngine`] for the commands' tests: scripted answers, no process.
//!
//! The real [`Bridge`](crate::context::engine::Bridge) is tested against fake bridge
//! scripts in `cli/tests/flow/context_engine.rs`; this fake lets a test of the search and
//! reindex mapping answer without spawning anything, and reads the requests they sent.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::context::engine::ContextEngine;
use crate::error::CpdError;

/// One scripted run: either the responses, or the failure to return.
#[derive(Clone, Debug)]
pub enum Reply {
    Answers(Vec<Value>),
    Fails(CpdError),
}

/// A [`ContextEngine`] that answers each `run` from its script and records the requests.
#[derive(Debug, Default)]
pub struct FakeContextEngine {
    replies: Mutex<Vec<Reply>>,
    calls: Mutex<Vec<(PathBuf, Vec<Value>)>>,
}

impl FakeContextEngine {
    /// A fake with scripted replies, used in order; a run past the script answers nothing.
    pub fn new(replies: Vec<Reply>) -> FakeContextEngine {
        FakeContextEngine {
            replies: Mutex::new(replies),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// A fake whose every `run` answers these responses.
    pub fn answering(responses: Vec<Value>) -> FakeContextEngine {
        FakeContextEngine {
            replies: Mutex::new(vec![Reply::Answers(responses)]),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The `(db, requests)` of every run, in order.
    pub fn calls(&self) -> Vec<(PathBuf, Vec<Value>)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    /// Every request sent, in order, whatever its run.
    pub fn requests(&self) -> Vec<Value> {
        self.calls()
            .into_iter()
            .flat_map(|(_, requests)| requests)
            .collect()
    }
}

impl ContextEngine for FakeContextEngine {
    fn run(&self, db: &Path, requests: &[Value]) -> Result<Vec<Value>, CpdError> {
        self.calls
            .lock()
            .map(|mut calls| calls.push((db.to_path_buf(), requests.to_vec())))
            .unwrap_or_default();
        let reply = self.replies.lock().ok().and_then(|mut replies| {
            if replies.len() > 1 {
                Some(replies.remove(0))
            } else {
                replies.first().cloned()
            }
        });
        match reply {
            Some(Reply::Answers(responses)) => Ok(responses),
            Some(Reply::Fails(error)) => Err(error),
            None => Ok(Vec::new()),
        }
    }
}
