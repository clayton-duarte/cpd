//! `context show` (commands.md §3.3): a Context's records, or one record in
//! full, with the Markdown render.

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::{ContextRow, Record, ShowArgs, curated, render};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, MachineName, RecordId, RecordKind};
use crate::output::Output;
use crate::store::Store;

/// `context show` (§3.3).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let Env {
        config, mut store, ..
    } = Env::open()?;
    let id = args.select.resolve(&mut store)?;
    if let Some(topic) = &args.topic {
        return show_topic(&mut store, &id, topic);
    }
    if let Some(record) = &args.record {
        return show_record(&mut store, &id, record, &config.machine);
    }
    let since = args
        .since
        .map_or(0, |s| i64::try_from(s).unwrap_or(i64::MAX));
    let (row, records, repos, retired, done, topics) = store.read(|conn| {
        let row = ContextRow::require(conn, &id)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq",
            Record::COLUMNS
        ))?;
        let records = stmt
            .query_map([id.as_str()], Record::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let repos = plan_repos(conn, &config.machine, &records)?;
        let retired = retired_labels(conn, &id)?;
        let (done, topics) = if args.all || args.since.is_some() {
            Default::default()
        } else {
            (
                done_plans(conn, &config.machine, &records)?,
                curated::topic_counts(conn, &id)?,
            )
        };
        Ok((row, records, repos, retired, done, topics))
    })?;
    let bounded = (!args.all && args.since.is_none()).then(|| {
        render::bounded(
            &row.title,
            row.jira.as_deref(),
            &records,
            &repos,
            &retired,
            &done,
            &topics,
        )
    });
    let markdown = match &bounded {
        Some(b) => b.markdown.clone(),
        None => render::markdown_retired(
            &row.title,
            row.jira.as_deref(),
            &records,
            since,
            &repos,
            &retired,
        ),
    };
    let records: Vec<Value> = records
        .iter()
        .filter(|(seq, _)| match &bounded {
            Some(b) => b.shown.contains(seq),
            None => *seq > since,
        })
        .map(|(seq, record)| {
            let mut v = record.to_json_listed();
            v["seq"] = json!(seq);
            if let Some(repos) = repos.get(&record.id.to_string()) {
                v["repos"] = json!(repos);
            }
            v
        })
        .collect();
    let mut out = json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "records": records,
        "markdown": markdown,
    });
    if let Some(b) = &bounded {
        out["omitted"] = json!({
            "superseded_decisions": b.omitted.superseded_decisions,
            "answered_questions": b.omitted.answered_questions,
            "superseded_notes": b.omitted.superseded_notes,
            "older_notes": b.omitted.older_notes,
            "done_plans": b.omitted.done_plans,
        });
    }
    Ok(Output::success(out))
}

/// The Plans of `records` that are done on this machine (`plans.done_at` set), by [`render::plan_key`]; a Plan of
/// another machine or with no row here is not.
fn done_plans(
    conn: &rusqlite::Connection,
    machine: &crate::ids::MachineName,
    records: &[render::Indexed],
) -> Result<std::collections::HashSet<String>, CpdError> {
    let mut stmt = conn.prepare("SELECT done_at IS NOT NULL FROM plans WHERE id = ?1")?;
    let mut done = std::collections::HashSet::new();
    for (_, record) in records {
        let Some(plan) = record.plan().filter(|p| p.machine == *machine) else {
            continue;
        };
        if stmt
            .query_row([plan.id.as_str()], |r| r.get::<_, bool>(0))
            .optional()?
            .unwrap_or(false)
        {
            done.insert(render::plan_key(plan));
        }
    }
    Ok(done)
}

/// The approved retirements of `context`, as `render` labels them: `<reason> <target>` by record id.
fn retired_labels(
    conn: &rusqlite::Connection,
    context: &ContextId,
) -> Result<render::Retired, CpdError> {
    let mut stmt = conn.prepare("SELECT record, reason, target FROM record_retirements WHERE context = ?1 AND status = 'approved'")?;
    let rows = stmt.query_map([context.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            format!("{} {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// `context show --topic <t>` (§3.3): every curated record in force of the topic, oldest first.
fn show_topic(store: &mut Store, context: &ContextId, topic: &str) -> Result<Output, CpdError> {
    let (row, found) = store.read(|conn| {
        Ok((
            ContextRow::require(conn, context)?,
            curated::by_topic(conn, context, topic)?,
        ))
    })?;
    if found.is_empty() {
        let topics = store.read(|conn| curated::topics(conn, context))?;
        return Err(CpdError::new(
            ErrorCode::TopicNotFound,
            format!("Context {context} has no curated record of topic {topic}."),
        )
        .with("context", context.as_str())
        .with("topic", topic)
        .with("topics", topics));
    }
    let markdown = found
        .iter()
        .map(|(id, short)| format!("- [{id}] {short}"))
        .collect::<Vec<_>>()
        .join("\n");
    let records: Vec<Value> = found
        .iter()
        .map(|(id, short)| json!({"id": id, "short": short}))
        .collect();
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "topic": topic,
        "records": records,
        "markdown": markdown,
    })))
}

/// `context show --record <id>` (§3.3): that one record in full.
fn show_record(
    store: &mut Store,
    context: &ContextId,
    id: &RecordId,
    machine: &MachineName,
) -> Result<Output, CpdError> {
    let (row, found) = store.read(|conn| {
        let row = ContextRow::require(conn, context)?;
        let found = conn
            .query_row(
                &format!(
                    "SELECT {} FROM context_records WHERE context = ?1 AND id = ?2",
                    Record::COLUMNS
                ),
                [context.as_str(), id.to_string().as_str()],
                Record::from_row,
            )
            .optional()?;
        Ok((row, found))
    })?;
    let (seq, record) = found.ok_or_else(|| {
        let err = if id.machine() == machine {
            CpdError::new(
                ErrorCode::RecordNotFound,
                format!("Context {context} has no record {id}."),
            )
        } else {
            CpdError::new(
                ErrorCode::RecordNotFound,
                format!(
                    "Context {context} has no record {id}. {id} belongs to machine {}; this machine is {}, and Contexts are machine-local, so another machine's records cannot be read here. The authoritative contract is in CONTEXT.md and docs/contract.md.",
                    id.machine(),
                    machine
                ),
            )
            .with("record_machine", id.machine().as_str())
            .with("local_machine", machine.as_str())
        };
        err.with("context", context.as_str())
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

/// The repos of the Workflows that are not `cancelled` of each `plan` record's Plan
/// (commands.md §3.3), by record id: each repo once, in Workflow key order, from
/// this machine's database. A Plan of another machine has none.
pub(crate) fn plan_repos(
    conn: &rusqlite::Connection,
    machine: &crate::ids::MachineName,
    records: &[render::Indexed],
) -> Result<render::Repos, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT repo FROM workflows WHERE plan = ?1 AND status <> 'cancelled' ORDER BY key",
    )?;
    let mut out = render::Repos::new();
    for (_, record) in records {
        let Some(plan) = record.plan().filter(|_| record.kind() == RecordKind::Plan) else {
            continue;
        };
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
