//! `context show` (commands.md §3.3): a Context's records, or one record in
//! full, with the Markdown render.

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::{ContextRow, Record, ShowArgs, digest, render};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId, RecordKind};
use crate::output::Output;
use crate::store::Store;

/// `context show` (§3.3).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let Env { home, config, mut store } = Env::open()?;
    let id = args.select.resolve(&mut store)?;
    if args.digest {
        return show_digest(&home, &mut store, &id);
    }
    if let Some(record) = &args.record {
        return show_record(&mut store, &id, record);
    }
    let since = args.since.map_or(0, |s| i64::try_from(s).unwrap_or(i64::MAX));
    let (row, records, repos) = store.read(|conn| {
        let row = ContextRow::require(conn, &id)?;
        let mut stmt =
            conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        let records = stmt.query_map([id.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let repos = plan_repos(conn, &config.machine, &records)?;
        Ok((row, records, repos))
    })?;
    let markdown = render::markdown(&row.title, row.jira.as_deref(), &records, since, &repos);
    let records: Vec<Value> = records
        .iter()
        .filter(|(seq, _)| *seq > since)
        .map(|(seq, record)| {
            let mut v = record.to_json_listed();
            v["seq"] = json!(seq);
            if let Some(repos) = repos.get(&record.id.to_string()) {
                v["repos"] = json!(repos);
            }
            v
        })
        .collect();
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "records": records,
        "markdown": markdown,
    })))
}

/// `context show --digest` (§3.3): the Context's latest digest file, and no record.
fn show_digest(home: &CpdHome, store: &mut Store, context: &ContextId) -> Result<Output, CpdError> {
    let row = store.read(|conn| ContextRow::require(conn, context))?;
    let digest = digest::latest(home, context)?;
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "digest": digest.as_ref().map(|d| json!({"upto_seq": d.upto_seq, "file": d.file, "bytes": d.text.len()})),
        "markdown": digest.map(|d| d.text),
    })))
}

/// `context show --record <id>` (§3.3): that one record in full.
fn show_record(store: &mut Store, context: &ContextId, id: &RecordId) -> Result<Output, CpdError> {
    let (row, found) = store.read(|conn| {
        let row = ContextRow::require(conn, context)?;
        let found = conn
            .query_row(
                &format!("SELECT {} FROM context_records WHERE context = ?1 AND id = ?2", Record::COLUMNS),
                [context.as_str(), id.to_string().as_str()],
                Record::from_row,
            )
            .optional()?;
        Ok((row, found))
    })?;
    let (seq, record) = found.ok_or_else(|| {
        CpdError::new(ErrorCode::RecordNotFound, format!("Context {context} has no record {id}."))
            .with("context", context.as_str())
            .with("record", id.to_string())
    })?;
    let markdown = render::single(&row.title, row.jira.as_deref(), &record);
    let mut v = record.to_json();
    v["seq"] = json!(seq);
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "records": [v],
        "markdown": markdown,
    })))
}

/// The repos of the PRs that are not `cancelled` of each `plan` record's Plan
/// (commands.md §3.3), by record id: each repo once, in PR key order, from
/// this machine's database. A Plan of another machine has none.
pub(crate) fn plan_repos(
    conn: &rusqlite::Connection,
    machine: &crate::ids::MachineName,
    records: &[render::Indexed],
) -> Result<render::Repos, CpdError> {
    let mut stmt = conn.prepare("SELECT repo FROM prs WHERE plan = ?1 AND status <> 'cancelled' ORDER BY key")?;
    let mut out = render::Repos::new();
    for (_, record) in records {
        let Some(plan) = record.plan().filter(|_| record.kind() == RecordKind::Plan) else { continue };
        let mut repos: Vec<String> = Vec::new();
        if plan.machine == *machine {
            for repo in stmt.query_map([plan.id.as_str()], |r| r.get::<_, String>(0))? {
                let repo = repo?;
                if !repos.contains(&repo) {
                    repos.push(repo);
                }
            }
        }
        out.insert(record.id.to_string(), repos);
    }
    Ok(out)
}
