//! The holographic engine rail (contract §4.6; `docs/research/context-holographic.md`,
//! `## Bridge` and `## Config and failure`).
//!
//! cpd reaches the holographic memory engine through `contrib/holographic/bridge.py`,
//! spawned per operation: JSON lines in on stdin, one JSON line out on stdout per
//! request, in order. `cpd.db` stays the source of truth; the engine is a derived
//! per-Context index under `context_engine_dir`.
//!
//! STUB: the red tests of Job `bridge-tests` live in
//! `cli/tests/flow/context_engine.rs`. This module only makes the branch build.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::error::CpdError;

/// A holographic engine, reached one JSON request line at a time.
pub trait ContextEngine {
    /// Sends `requests`, one JSON object per line, in order, closes stdin and
    /// returns the responses in order. `engine_failed` when the child exits
    /// non-zero, prints bad JSON, answers no line, or times out. The child is
    /// always killed and reaped.
    fn run(&self, db: &Path, requests: &[Value]) -> Result<Vec<Value>, CpdError>;

    /// `{"cmd":"ping"}`: the engine's health and version (`doctor`, first use).
    fn ping(&self, db: &Path) -> Result<Value, CpdError> {
        Ok(self.run(db, &[serde_json::json!({"cmd": "ping"})])?.remove(0))
    }

    /// `{"cmd":"stats"}`: fact count and the projection watermark.
    fn stats(&self, db: &Path) -> Result<Value, CpdError> {
        Ok(self.run(db, &[serde_json::json!({"cmd": "stats"})])?.remove(0))
    }

    /// `{"cmd":"reset"}`: delete every fact of this Context's DB.
    fn reset(&self, db: &Path) -> Result<Value, CpdError> {
        Ok(self.run(db, &[serde_json::json!({"cmd": "reset"})])?.remove(0))
    }
}

/// The spawning client: one bridge child per [`ContextEngine::run`].
pub struct Bridge {
    script: PathBuf,
    python: PathBuf,
    timeout: Duration,
}

impl Bridge {
    /// The resolved client. `engine_disabled` (exit 3) when `enabled` is false;
    /// `engine_missing` (exit 4) when the bridge or the interpreter is absent.
    pub fn new(enabled: bool, script: PathBuf, python: PathBuf, timeout: Duration) -> Result<Bridge, CpdError> {
        let _ = (enabled, script, python, timeout);
        todo!("bridge-tests: the spawning client is the bridge-impl Job")
    }

    /// The resolved bridge script.
    pub fn script(&self) -> &Path {
        &self.script
    }

    /// The resolved interpreter.
    pub fn python(&self) -> &Path {
        &self.python
    }

    /// The resolved deadline.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl ContextEngine for Bridge {
    fn run(&self, db: &Path, requests: &[Value]) -> Result<Vec<Value>, CpdError> {
        let _ = (db, requests);
        todo!("bridge-tests: the spawning client is the bridge-impl Job")
    }
}
