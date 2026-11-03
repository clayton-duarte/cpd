//! The `[experimental]` table of `config.toml` (contract §8.1, §8.2). STUB: red-test scaffolding only.

use serde::Serialize;

use crate::error::CpdError;
use crate::home::CpdHome;

/// The four experimental features' switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Experimental {
    pub durable_workers: bool,
    pub write_curation: bool,
    pub failure_triage: bool,
    pub turn_question_guard: bool,
}

/// One experimental feature, named by its `[experimental]` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    DurableWorkers,
    WriteCuration,
    FailureTriage,
    TurnQuestionGuard,
}

/// Sets one key of `[experimental]` atomically. STUB.
pub fn set(_home: &CpdHome, _feature: Feature, _on: bool) -> Result<(), CpdError> {
    unimplemented!("experimental::set")
}
