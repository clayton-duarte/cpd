//! The `holographic` doctor check (commands.md §14.1; docs/research/context-holographic.md,
//! `## Config and failure`): the engine resolves and answers `ping`, read-only.

use std::env;

use serde_json::json;

use super::check::{Check, Severity};
use cpd_core::config::Config;
use cpd_core::context::engine::{Bridge, ContextEngine};

/// Resolves the bridge and interpreter, runs `{"cmd":"ping"}` under
/// `context_engine_timeout_ms`, and reports the engine version. `disabled` when
/// `context_engine_enabled = false`; a missing bridge or interpreter fails with the
/// `engine_missing` hint. It reads no index and builds none.
pub fn holographic(config: &Config) -> Check {
    let name = "holographic";
    if !config.context_engine_enabled {
        return Check::pass(name, Severity::Error).found(json!({"disabled": true}));
    }
    let script = match config.context_engine_bridge_path() {
        Ok(script) => script,
        Err(e) => return Check::from_error(name, Severity::Error, &e),
    };
    let python = match config.context_engine_python_path() {
        Ok(python) => python,
        Err(e) => return Check::from_error(name, Severity::Error, &e),
    };
    let engine = match Bridge::new(true, script, python, config.context_engine_timeout()) {
        Ok(engine) => engine,
        Err(e) => return Check::from_error(name, Severity::Error, &e),
    };
    // `ping` opens no database; a path with an existing parent binds no file.
    match engine.ping(&env::temp_dir().join("cpd-holographic-ping.db")) {
        Ok(pong) => Check::pass(name, Severity::Error)
            .found(pong["version"].as_str().unwrap_or("unknown").to_owned()),
        Err(e) => Check::from_error(name, Severity::Error, &e),
    }
}
