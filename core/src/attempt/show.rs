//! `attempt show` and `attempt list` (commands.md §6.3): read-only.

use rusqlite::OptionalExtension;
use serde_json::json;

use super::row::{self, Attempt};
use super::{Env, ListArgs, ShowArgs};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::home::Hook;
use crate::output::Output;

/// `attempt show` (§6.3), with the `events_cursor` of every snapshot command (§17).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let id = args.attempt;
    let mut env = Env::open()?;
    let (attempt, escalations, worker_file, cursor, footprint) = env.store.read(|conn| {
        let attempt = row::load(conn, &plan, &id)?;
        let escalations = row::escalations(conn, &plan, &id)?;
        let file: Option<Option<String>> = match &attempt.worker_session {
            Some(s) => conn
                .query_row("SELECT file FROM sessions WHERE id = ?1", [s], |r| r.get(0))
                .optional()?,
            None => None,
        };
        let job = crate::plan::jobs::load(conn, &plan)?
            .into_iter()
            .find(|j| j.key == attempt.job)
            .ok_or_else(|| {
                CpdError::bad_row(format!("Attempt {plan}/{id} has no Job {}", attempt.job))
            })?;
        let footprint = super::footprint::derive(&job, &attempt.actual_files)?;
        Ok((
            attempt,
            escalations,
            file.flatten(),
            highest_id(conn)?,
            footprint,
        ))
    })?;
    let check = env.home.hook_log(&plan, &id, Hook::PreCommit);
    let paths = json!({
        "spec": attempt.spec_path,
        "check": check.exists().then(|| check.display().to_string()),
        "log": env.home.driver_log(&plan, &id).display().to_string(),
        "worker_file": worker_file,
    });
    let mut shown = attempt.to_json();
    shown["footprint"] = footprint;
    Ok(Output::success(json!({
        "attempt": shown, "escalations": escalations, "paths": paths, "events_cursor": cursor,
    })))
}

/// `attempt list` (§6.3).
pub(super) fn list(args: ListArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let attempts = env
        .store
        .read(|conn| row::list(conn, &plan, args.job.as_ref(), args.active))?;
    let listed: Vec<_> = attempts.iter().map(summary).collect();
    Ok(Output::success(json!({"attempts": listed})))
}

fn summary(a: &Attempt) -> serde_json::Value {
    json!({
        "id": a.id, "job": a.job, "number": a.number, "tier": a.tier, "origin": a.origin, "status": a.status,
        "outcome": a.outcome, "cause": a.cause, "started_at": a.started_at, "ended_at": a.ended_at,
    })
}
