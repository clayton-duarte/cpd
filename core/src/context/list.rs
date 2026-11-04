//! `context list` (commands.md §3.2): every Context with its record counts and
//! Plans, a read-only snapshot with its `events_cursor`.

use serde_json::json;

use super::ContextRow;
use crate::env::Env;
use crate::error::CpdError;
use crate::output::Output;

/// `context list` (§3.2).
pub(super) fn list() -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let (contexts, cursor) = store.read(|conn| {
        let mut contexts = Vec::new();
        let mut stmt = conn.prepare(&format!("SELECT {} FROM contexts WHERE deleted_at IS NULL ORDER BY id", ContextRow::COLUMNS))?;
        let rows = stmt.query_map([], ContextRow::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for row in rows {
            let records: i64 =
                conn.query_row("SELECT count(*) FROM context_records WHERE context = ?1", [&row.id], |r| r.get(0))?;
            let mut stmt =
                conn.prepare("SELECT id, title, phase, lead_session, archived_at FROM plans WHERE context = ?1 AND deleted_at IS NULL ORDER BY created_at, id")?;
            let plans = stmt
                .query_map([&row.id], |r| {
                    Ok(json!({"id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?,
                              "phase": r.get::<_, String>(2)?, "lead_session": r.get::<_, String>(3)?,
                              "archived_at": r.get::<_, Option<String>>(4)?}))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            contexts.push(json!({
                "id": row.id, "title": row.title, "owner": row.owner, "jira": row.jira, "archived_at": row.archived_at,
                "records": records, "plans": plans,
            }));
        }
        let cursor: i64 = conn.query_row("SELECT coalesce(max(id), 0) FROM events", [], |r| r.get(0))?;
        Ok((contexts, cursor))
    })?;
    Ok(Output::success(
        json!({"contexts": contexts, "events_cursor": cursor}),
    ))
}
