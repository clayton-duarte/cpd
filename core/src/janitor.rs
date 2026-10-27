//! `cpd janitor` (commands.md §29, contract §7.2): one pass over the merged and cancelled PRs that still have a
//! worktree on disk, then exit. STUB of the test Job `janitor-tests`: the implementation Job fills it in.

use std::path::PathBuf;

use serde_json::json;

use crate::env::Env;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::output::Output;

/// One pass: takes `locks/janitor.lock` without waiting (held by another pass: `{"ok":true,"skipped":"running"}`,
/// nothing touched) and cleans every pending PR. Output `{"ok":true,"outcomes":[{plan, pr, status, path, outcome,
/// reason?}]}` (commands.md §29). Appends the `cleanup_failed` Events (source `janitor`).
pub fn pass(_env: &mut Env) -> Result<Output, CpdError> {
    Ok(Output::success(json!({"outcomes": []})))
}

/// Whether a pass holds `locks/janitor.lock` (engine E16's probe): opens the file without creating it, lets go at once.
pub fn running(_home: &CpdHome) -> Result<bool, CpdError> {
    Ok(false)
}

/// The log of the passes, appended (as `context::distill::log_path`).
pub fn log_path(home: &CpdHome) -> PathBuf {
    home.root().join("run").join("janitor.log")
}
