//! `context pending` (commands.md §3.11, hs-d159 P1): what a Context's active
//! Plans have started and not yet landed on the base. One read transaction
//! over `plans`, `prs`, `jobs` and `pull_requests`; no write, no `git`, no `gh`.

use std::fmt::Write as _;

use clap::Args;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::{ContextRow, Select};
use crate::env::Env;
use crate::error::CpdError;
use crate::ids::ContextId;
use crate::output::Output;

#[derive(Args, Debug)]
pub struct PendingArgs {
    #[command(flatten)]
    select: Select,
    /// Print the result as markdown instead of JSON
    #[arg(long)]
    markdown: bool,
}

/// `context pending` (§3.11).
pub(super) fn pending(args: PendingArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let plans = store.read(|conn| {
        ContextRow::require(conn, &context)?;
        plans(conn, &context)
    })?;
    if args.markdown {
        print!("{}", markdown(&context, &plans));
        return Ok(Output::Owned);
    }
    Ok(Output::success(json!({"context": context, "plans": plans})))
}

/// Every Plan of the Context not `done` and not archived, in creation order,
/// each with its unmerged PRs and their Jobs.
fn plans(conn: &Connection, context: &ContextId) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, phase, lead_session FROM plans
          WHERE context = ?1 AND phase <> 'done' AND archived_at IS NULL ORDER BY created_at, id",
    )?;
    let heads = stmt
        .query_map([context.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    heads
        .into_iter()
        .map(|(id, title, phase, lead_session)| {
            let prs = prs(conn, &id)?;
            Ok(json!({"id": id, "title": title, "phase": phase, "lead_session": lead_session, "prs": prs}))
        })
        .collect()
}

/// The Plan's PRs not `merged` and not `cancelled`, in key order.
fn prs(conn: &Connection, plan: &str) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.key, p.repo, p.base, p.status, p.lead_branch, g.number, g.url, g.state
           FROM prs p LEFT JOIN pull_requests g ON g.id = p.pull_request
          WHERE p.plan = ?1 AND p.status NOT IN ('merged', 'cancelled') ORDER BY p.key",
    )?;
    let rows = stmt
        .query_map([plan], |r| {
            let pull = match (r.get::<_, Option<i64>>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, Option<String>>(7)?) {
                (Some(number), Some(url), Some(state)) => json!({"number": number, "url": url, "state": state}),
                _ => Value::Null,
            };
            Ok((r.get::<_, String>(0)?, json!({
                "key": r.get::<_, String>(0)?, "repo": r.get::<_, String>(1)?, "base": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?, "lead_branch": r.get::<_, String>(4)?, "pull_request": pull,
            })))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(key, mut pr)| {
            pr["jobs"] = json!(jobs(conn, plan, &key)?);
            Ok(pr)
        })
        .collect()
}

/// The PR's Jobs not `cancelled`, in `seq` order. The PR is not merged, so a
/// `merged` Job is on the Lead Branch and not yet on the base.
fn jobs(conn: &Connection, plan: &str, pr: &str) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT key, status, title, touches FROM jobs
          WHERE plan = ?1 AND pr = ?2 AND status <> 'cancelled' ORDER BY seq",
    )?;
    let rows = stmt
        .query_map([plan, pr], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(key, status, title, touches)| {
            let touches: Value = serde_json::from_str(&touches).map_err(|e| {
                CpdError::new(
                    crate::error::ErrorCode::StoreFailed,
                    format!("The database holds an invalid row: Job {key} of Plan {plan} has touches {touches:?}: {e}."),
                )
            })?;
            Ok(json!({"key": key, "status": status, "title": title, "touches": touches}))
        })
        .collect()
}

/// The result as markdown (§3.11).
fn markdown(context: &ContextId, plans: &[Value]) -> String {
    if plans.is_empty() {
        return "No pending work.\n".to_owned();
    }
    let text = |v: &Value| v.as_str().expect("a string column").to_owned();
    let mut out = format!("# Pending work in {context}\n");
    for plan in plans {
        let _ = write!(out, "\n## {} ({}): {}\n", text(&plan["id"]), text(&plan["phase"]), text(&plan["title"]));
        for pr in plan["prs"].as_array().expect("prs") {
            let _ = write!(
                out,
                "\n### PR {} ({}, base {}): {}\n\n",
                text(&pr["key"]),
                text(&pr["repo"]),
                text(&pr["base"]),
                text(&pr["status"])
            );
            let pull = &pr["pull_request"];
            if pull.is_null() {
                out.push_str("no pull request\n\n");
            } else {
                let _ = write!(out, "#{} {} {}\n\n", pull["number"], text(&pull["state"]), text(&pull["url"]));
            }
            for job in pr["jobs"].as_array().expect("jobs") {
                let touches: Vec<String> =
                    job["touches"].as_array().expect("touches").iter().map(text).collect();
                let _ = writeln!(
                    out,
                    "- {} {}: {} ({})",
                    text(&job["key"]),
                    text(&job["status"]),
                    text(&job["title"]),
                    touches.join(", ")
                );
            }
        }
    }
    out
}
