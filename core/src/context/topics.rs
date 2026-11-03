//! `context topics` (commands.md §3.3a): the curated topics in force, with their counts.

use serde_json::json;

use super::{ContextRow, TopicsArgs, curated};
use crate::env::Env;
use crate::error::CpdError;
use crate::output::Output;

pub(super) fn topics(args: TopicsArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let id = args.select.resolve(&mut store)?;
    let (row, counts) = store.read(|conn| {
        Ok((
            ContextRow::require(conn, &id)?,
            curated::topic_counts(conn, &id)?,
        ))
    })?;
    let topics: Vec<_> = counts
        .iter()
        .map(|(topic, count)| json!({"topic": topic, "count": count}))
        .collect();
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "topics": topics,
        "markdown": curated::topics_line(&counts),
    })))
}
