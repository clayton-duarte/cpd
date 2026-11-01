//! `cpd context reindex`: rebuild the Context's holographic index
//! (contract §4.6; docs/research/context-holographic.md, `## Index and staleness`).
//!
//! The operator's explicit rebuild: reset the Context's engine DB and project every
//! record, in force or not. It writes only the derived index (the bridge's own
//! watermark); it changes no Context record and appends no Event.

use clap::Args;
use serde_json::json;

use super::Select;
use super::engine::{Bridge, ContextEngine};
use super::search::{self, check_ok};
use crate::env::Env;
use crate::error::CpdError;
use crate::output::Output;

/// `context reindex [--context <c>]`.
#[derive(Args, Debug)]
pub struct ReindexArgs {
    #[command(flatten)]
    select: Select,
}

/// `cpd context reindex` (docs/research/context-holographic.md, `## Index and staleness`).
pub(super) fn reindex(args: ReindexArgs) -> Result<Output, CpdError> {
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let snap = search::snapshot(&mut store, &context)?;
    let bridge = Bridge::new(
        config.context_engine_enabled,
        config.context_engine_bridge_path()?,
        config.context_engine_python_path()?,
        config.context_engine_reindex_timeout(),
    )?;
    let db = config
        .context_engine_dir(&home)
        .join(format!("{}.db", context.as_str()));
    let archive = search::index(&snap, &snap.records);
    let responses = bridge.run(&db, &[json!({"cmd": "reset"}), archive])?;
    for response in &responses {
        check_ok(response, "")?;
    }
    Ok(Output::success(json!({
        "context": {"id": snap.row.id, "title": snap.row.title, "jira": snap.row.jira},
        "records": snap.records.len(),
    })))
}
