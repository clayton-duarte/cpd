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
use crate::ids::{ContextId, PlanId};
use crate::plan::display::Display;
use crate::plan::jobs::{self as plan_jobs, Job};
use crate::plan::fingerprint::load_workflows;
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
/// each with its plan-level explore Jobs and its unmerged PRs with their Jobs.
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
            let plan_id = PlanId::parse(&id)?;
            let display = Display::load(conn, &plan_id)?;
            let all_jobs = plan_jobs::load(conn, &plan_id)?;
            Ok(json!({
                "id": id, "title": title, "phase": phase, "lead_session": lead_session,
                "jobs": explore_jobs(&display, &all_jobs), "workflows": workflows(conn, &id, &display, &all_jobs)?,
            }))
        })
        .collect()
}

/// The Plan's explore Jobs (`jobs.pr` NULL, commands.md §3.12) not `cancelled`, in `seq` order: beside `prs`, each
/// `{key, kind, status, tier, title, touches}` with `waiting_on` only when it cannot start (`status` is then `waiting`,
/// the same display rule as a PR Job). `tier` is the displayed Tier, `touches` the parsed JSON (`[]` when null).
fn explore_jobs(display: &Display, all: &[Job]) -> Vec<Value> {
    all.iter()
        .filter(|job| job.workflow.is_none() && job.status != "cancelled")
        .map(|job| {
            let blockers = display.job_blockers(all, job);
            let touches = job.spec.as_ref().map(|spec| spec.touches.clone()).unwrap_or_else(|| json!([]));
            let mut value = json!({
                "key": job.key.clone(),
                "kind": job.kind.clone(),
                "status": if blockers.is_empty() { job.status.clone() } else { "waiting".to_owned() },
                "tier": display.job_tier(job).unwrap_or_default(),
                "title": job.title.clone(),
                "touches": touches,
            });
            add_waiting_on(&mut value, blockers);
            value
        })
        .collect()
}

/// The Plan's PRs not `merged` and not `cancelled`, in key order. A PR that cannot start yet has `waiting_on`: the keys of
/// what it waits for, and so has each of its `queued` Jobs that cannot start, whose `status` is then `waiting` (hs-d521,
/// hs-d523: a display status, the stored one is `queued`); see [`Display`].
fn workflows(conn: &Connection, plan: &str, display: &Display, all_jobs: &[Job]) -> Result<Vec<Value>, CpdError> {
    let plan_id = PlanId::parse(plan)?;
    let all_prs = load_workflows(conn, &plan_id)?;
    let mut stmt = conn.prepare(
        "SELECT p.key, p.repo, p.base, p.status, p.lead_branch, g.number, g.url, g.state
           FROM workflows p LEFT JOIN pull_requests g ON g.id = p.pull_request
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
        .map(|(key, mut workflow)| {
            workflow["jobs"] = json!(jobs(conn, plan, &key, display, all_jobs)?);
            if let Some(row) = all_prs.iter().find(|p| p.key.as_str() == key) {
                add_waiting_on(&mut workflow, display.workflow_blockers(row));
            }
            Ok(workflow)
        })
        .collect()
}

/// The PR's Jobs not `cancelled`, in `seq` order. The PR is not merged, so a
/// `merged` Job is on the Lead Branch and not yet on the base.
fn jobs(conn: &Connection, plan: &str, pr: &str, display: &Display, all: &[Job]) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT key, status, title, touches FROM jobs
          WHERE plan = ?1 AND workflow = ?2 AND status <> 'cancelled' ORDER BY seq",
    )?;
    let rows = stmt
        .query_map([plan, workflow], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(key, status, title, touches)| {
            // A stub draft has no Spec yet, so no touched files.
            let touches: Value = touches
                .map(|text| {
                    serde_json::from_str(&text).map_err(|e| {
                        CpdError::bad_row(format!("Job {key} of Plan {plan} has touches {text:?}: {e}"))
                    })
                })
                .transpose()?
                .unwrap_or_else(|| json!([]));
            let mut job = json!({"key": key, "status": status, "title": title, "touches": touches});
            if let Some(row) = all.iter().find(|j| j.key == key) {
                let blockers = display.job_blockers(all, row);
                if !blockers.is_empty() {
                    job["status"] = json!("waiting");
                }
                add_waiting_on(&mut job, blockers);
            }
            Ok(job)
        })
        .collect()
}

/// `waiting_on` of a Job or PR, only when it waits for something.
fn add_waiting_on(item: &mut Value, blockers: Vec<String>) {
    if !blockers.is_empty() {
        item["waiting_on"] = json!(blockers);
    }
}

/// What `waiting_on` of a Job or PR says, joined: none for work that waits for nothing.
fn waiting_on(item: &Value) -> Option<String> {
    let keys: Vec<&str> = item["waiting_on"].as_array()?.iter().filter_map(Value::as_str).collect();
    Some(keys.join(", "))
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
        for workflow in plan["workflows"].as_array().expect("workflows") {
            let _ = write!(
                out,
                "\n### PR {} ({}, base {}): {}\n\n",
                text(&workflow["key"]),
                text(&workflow["repo"]),
                text(&workflow["base"]),
                text(&workflow["status"])
            );
            if let Some(keys) = waiting_on(workflow) {
                let _ = write!(out, "waiting on {keys}\n\n");
            }
            let pull = &workflow["pull_request"];
            if pull.is_null() {
                out.push_str("no pull request\n\n");
            } else {
                let _ = write!(out, "#{} {} {}\n\n", pull["number"], text(&pull["state"]), text(&pull["url"]));
            }
            for job in workflow["jobs"].as_array().expect("jobs") {
                let touches: Vec<String> =
                    job["touches"].as_array().expect("touches").iter().map(text).collect();
                let status = match waiting_on(job) {
                    Some(keys) => format!("{} on {keys}", text(&job["status"])),
                    None => text(&job["status"]),
                };
                let _ = writeln!(
                    out,
                    "- {} {status}: {} ({})",
                    text(&job["key"]),
                    text(&job["title"]),
                    touches.join(", ")
                );
            }
        }
        let jobs = plan["jobs"].as_array().expect("jobs");
        if !jobs.is_empty() {
            let _ = write!(out, "\n### Jobs\n\n");
            for job in jobs {
                let touches: Vec<String> = job["touches"].as_array().expect("touches").iter().map(text).collect();
                let status = match waiting_on(job) {
                    Some(keys) => format!("{} on {keys}", text(&job["status"])),
                    None => text(&job["status"]),
                };
                let _ = writeln!(
                    out,
                    "- {} {} {status}: {} ({})",
                    text(&job["key"]),
                    text(&job["kind"]),
                    text(&job["title"]),
                    touches.join(", ")
                );
            }
        }
    }
    out
}
